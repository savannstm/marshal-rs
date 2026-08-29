//! The Marshal tokenizer: a stackless, pull-based reader over a byte slice.
//!
//! [`Reader`] never recurses and never allocates on its own - it hands back
//! one [`Token`] per call, borrowing directly from the input buffer. Nested
//! structure (arrays, hashes, objects, wrapped values) is *sequenced* by the
//! caller, which knows from each token's declared length how many further
//! `next()` calls are needed.
//!
//! `Ivar` is the one two-step token: its instance-variable pairs come
//! *after* the wrapped value in the byte stream, so consume it by calling
//! [`Reader::next`] for the wrapped value (fully, however deep), then
//! [`Reader::next_ivar_count`] followed by that many `next()`-pairs.

use crate::wire::{self, Tag};
use thiserror::Error;

/// A byte span into the input buffer, used by [`SymbolTable`] implementations
/// to record where a symbol's bytes live without copying them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span {
    pub offset: u32,
    pub len: u32,
}

/// Storage for the Marshal symbol table (the ordered list of distinct symbols
/// seen so far, which `SymLink` back-references index into).
///
/// Implemented by a caller-supplied fixed slice ([`FixedSymbolTable`]) for
/// genuinely allocation-free use, and - under the `alloc` feature - for
/// `alloc::vec::Vec<Span>` directly, so the `alloc`-tier loader doesn't need
/// to size anything up front.
#[allow(clippy::missing_errors_doc)]
pub trait SymbolTable {
    fn push(&mut self, span: Span) -> Result<u32, ReadError>;
    fn get(&self, idx: u32) -> Option<Span>;
}

/// A [`SymbolTable`] backed by a caller-provided fixed-capacity slice.
///
/// Returns [`ReadError::SymbolTableFull`] once the slice is exhausted; the
/// caller controls memory, so a full table is a normal, recoverable
/// condition, not a panic.
pub struct FixedSymbolTable<'b> {
    slots: &'b mut [Span],
    len: usize,
}

impl<'b> FixedSymbolTable<'b> {
    #[must_use]
    pub fn new(slots: &'b mut [Span]) -> Self {
        Self { slots, len: 0 }
    }
}

#[allow(clippy::cast_possible_truncation)]
impl SymbolTable for FixedSymbolTable<'_> {
    #[inline]
    fn push(&mut self, span: Span) -> Result<u32, ReadError> {
        let Some(slot) = self.slots.get_mut(self.len) else {
            return Err(ReadError::SymbolTableFull);
        };
        *slot = span;
        self.len += 1;
        Ok((self.len - 1) as u32)
    }

    #[inline]
    fn get(&self, idx: u32) -> Option<Span> {
        self.slots.get(idx as usize).copied()
    }
}

#[allow(clippy::cast_possible_truncation)]
#[cfg(feature = "alloc")]
impl SymbolTable for alloc::vec::Vec<Span> {
    #[inline]
    fn push(&mut self, span: Span) -> Result<u32, ReadError> {
        let idx = self.len() as u32;
        Self::push(self, span);
        Ok(idx)
    }

    #[inline]
    fn get(&self, idx: u32) -> Option<Span> {
        self.as_slice().get(idx as usize).copied()
    }
}

/// A single Marshal wire-format event, borrowing its byte payloads from the
/// buffer the [`Reader`] was constructed with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token<'a> {
    Nil,
    True,
    False,
    Fixnum(i32),
    Bignum {
        negative: bool,
        magnitude_le: &'a [u8],
    },
    /// Original float text, verbatim (`"inf"`, `"-0.0"`, ...).
    Float(&'a [u8]),
    Str(&'a [u8]),
    /// A symbol's bytes - already resolved whether it was a fresh `Symbol`
    /// or a `SymLink` back-reference; the caller never sees link indices for
    /// symbols.
    Symbol(&'a [u8]),
    /// An object-link back-reference: an index into the caller's own
    /// positional value table (the `Reader` does not track object identity).
    Link(u32),
    Regexp {
        source: &'a [u8],
        options: u8,
    },
    Class(&'a [u8]),
    Module(&'a [u8]),
    OldModule(&'a [u8]),
    /// Followed by `len` values.
    BeginArray(u32),
    /// Followed by `len` key/value pairs, then one extra default-value token
    /// if `has_default`.
    BeginHash {
        len: u32,
        has_default: bool,
    },
    /// Followed by `len` (symbol, value) member pairs.
    BeginStruct {
        class: &'a [u8],
        len: u32,
    },
    /// Followed by `len` (symbol, value) instance-variable pairs, inline
    /// (unlike [`Token::BeginIvar`], `Object`'s ivars are not a separate
    /// wrapper).
    BeginObject {
        class: &'a [u8],
        len: u32,
    },
    /// Self-contained: the `_dump`-produced bytes, not a recursive value.
    UserDefined {
        class: &'a [u8],
        data: &'a [u8],
    },
    /// Followed by one wrapped value (the `_dump_data`/`marshal_dump` result).
    BeginUserMarshal {
        class: &'a [u8],
    },
    BeginData {
        class: &'a [u8],
    },
    /// A built-in-type subclass instance; followed by one wrapped value.
    BeginUClass {
        class: &'a [u8],
    },
    /// A `Module#extend`ed object; followed by one wrapped value.
    BeginExtended {
        module: &'a [u8],
    },
    /// Followed by one wrapped value; once that value (and everything it
    /// contains) is fully consumed, call [`Reader::next_ivar_count`].
    BeginIvar,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum ReadError {
    #[error("unexpected end of input at byte {offset}")]
    UnexpectedEof { offset: usize },
    #[error("not a Marshal 4.8 byte stream (bad 2-byte header)")]
    InvalidHeader,
    #[error("unknown type tag {byte:#04x} at byte {offset}")]
    InvalidTag { byte: u8, offset: usize },
    #[error("symbol table capacity exceeded")]
    SymbolTableFull,
    #[error("symbol link {idx} has no matching symbol")]
    UnknownSymbolLink { idx: u32 },
    #[error("object link {idx} has no matching object")]
    UnknownObjectLink { idx: u32 },
    #[error("declared length exceeds remaining input")]
    LengthOverflow,
    /// A construct the format allows but this crate does not implement
    /// (e.g. an encoding-bearing symbol, `TYPE_IVAR` immediately wrapping a
    /// `TYPE_SYMBOL`) - vanishingly rare in real-world Marshal streams.
    #[error("unsupported construct: {0}")]
    Unsupported(&'static str),
}

/// A stackless pull-parser over a Marshal 4.8 byte stream.
pub struct Reader<'a, 'b> {
    buf: &'a [u8],
    pos: usize,
    symbols: &'b mut dyn SymbolTable,
}

#[allow(
    clippy::missing_errors_doc,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
impl<'a, 'b> Reader<'a, 'b> {
    /// Validates the 2-byte `\x04\x08` header and returns a reader positioned
    /// just after it.
    pub fn new(buf: &'a [u8], symbols: &'b mut dyn SymbolTable) -> Result<Self, ReadError> {
        let Some(&[major, minor]) = buf.first_chunk::<2>() else {
            return Err(ReadError::InvalidHeader);
        };
        if major != wire::MAJOR_VERSION || minor != wire::MINOR_VERSION {
            return Err(ReadError::InvalidHeader);
        }
        Ok(Self { buf, pos: 2, symbols })
    }

    #[inline]
    #[must_use]
    pub const fn position(&self) -> usize {
        self.pos
    }

    #[inline]
    const fn eof(&self) -> ReadError {
        ReadError::UnexpectedEof { offset: self.pos }
    }

    #[inline]
    fn read_byte(&mut self) -> Result<u8, ReadError> {
        let byte = *self.buf.get(self.pos).ok_or_else(|| self.eof())?;
        self.pos += 1;
        Ok(byte)
    }

    #[inline]
    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], ReadError> {
        let end = self.pos.checked_add(len).ok_or(ReadError::LengthOverflow)?;
        let bytes = self.buf.get(self.pos..end).ok_or_else(|| self.eof())?;
        self.pos = end;
        Ok(bytes)
    }

    #[inline]
    fn read_int(&mut self) -> Result<i32, ReadError> {
        let lead = self.read_byte()? as i8;
        let tail_len = wire::packed_int_tail_len(lead);
        let tail = self.read_bytes(tail_len)?;
        Ok(wire::decode_int(lead, tail))
    }

    /// Reads a non-negative length, rejecting anything that couldn't
    /// possibly fit in the remaining input - so a corrupt or hostile length
    /// can never reach a `Vec::with_capacity` call downstream.
    #[inline]
    fn read_len(&mut self) -> Result<u32, ReadError> {
        let n = self.read_int()?;
        if n < 0 || n as usize > self.buf.len().saturating_sub(self.pos) {
            return Err(ReadError::LengthOverflow);
        }
        Ok(n as u32)
    }

    /// Reads a table index (a symlink or object-link back-reference).
    /// Unlike [`Reader::read_len`], this has no relationship to the
    /// remaining input size - a small file can legitimately link back to
    /// entry 0 from its very last byte - so only non-negativity is
    /// checked here; out-of-range indices are caught where they're
    /// resolved against the actual table.
    #[inline]
    fn read_index(&mut self) -> Result<u32, ReadError> {
        let n = self.read_int()?;
        if n < 0 {
            return Err(ReadError::LengthOverflow);
        }
        Ok(n as u32)
    }

    #[inline]
    fn read_chunk(&mut self) -> Result<&'a [u8], ReadError> {
        let len = self.read_len()?;
        self.read_bytes(len as usize)
    }

    fn read_tag(&mut self) -> Result<Tag, ReadError> {
        let offset = self.pos;
        let byte = self.read_byte()?;
        Tag::from_byte(byte).ok_or(ReadError::InvalidTag { byte, offset })
    }

    /// Reads a `Symbol` or `SymLink` tag (Ruby's "unique"), returning its
    /// resolved bytes either way. Used for class/module/struct names and
    /// hash/object member keys, all of which are always symbols.
    fn read_unique(&mut self) -> Result<&'a [u8], ReadError> {
        match self.read_tag()? {
            Tag::Symbol => {
                let bytes = self.read_chunk()?;
                let span = Span {
                    offset: (self.pos - bytes.len()) as u32,
                    len: bytes.len() as u32,
                };
                self.symbols.push(span)?;
                Ok(bytes)
            }
            Tag::SymLink => {
                let idx = self.read_index()?;
                let span = self.symbols.get(idx).ok_or(ReadError::UnknownSymbolLink { idx })?;
                Ok(&self.buf[span.offset as usize..(span.offset + span.len) as usize])
            }
            Tag::Ivar => Err(ReadError::Unsupported("symbol carrying its own instance variables")),
            other => Err(ReadError::InvalidTag {
                byte: other.byte(),
                offset: self.pos - 1,
            }),
        }
    }

    /// Reads the ivar-count + pairs that follow an [`Token::BeginIvar`]'s
    /// fully-resolved wrapped value. The caller then reads that many
    /// (symbol, value) pairs itself via [`Reader::next`].
    pub fn next_ivar_count(&mut self) -> Result<u32, ReadError> {
        self.read_len()
    }

    /// Reads the next token. The caller is expected to know, from tokens
    /// already read, exactly how many further calls are due - there is no
    /// "end of stream" sentinel beyond plain byte exhaustion.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Token<'a>, ReadError> {
        match self.read_tag()? {
            Tag::Nil => Ok(Token::Nil),
            Tag::True => Ok(Token::True),
            Tag::False => Ok(Token::False),
            Tag::Fixnum => Ok(Token::Fixnum(self.read_int()?)),
            Tag::Link => Ok(Token::Link(self.read_index()?)),
            Tag::Symbol | Tag::SymLink => {
                self.pos -= 1;
                Ok(Token::Symbol(self.read_unique()?))
            }
            Tag::Bignum => {
                let sign = self.read_byte()?;
                let negative = sign != wire::SIGN_POSITIVE;
                let word_len = self.read_len()? as usize;
                let byte_len = word_len.checked_mul(2).ok_or(ReadError::LengthOverflow)?;
                let magnitude_le = self.read_bytes(byte_len)?;
                Ok(Token::Bignum { negative, magnitude_le })
            }
            Tag::Float => Ok(Token::Float(self.read_chunk()?)),
            Tag::String => Ok(Token::Str(self.read_chunk()?)),
            Tag::Regexp => {
                let source = self.read_chunk()?;
                let options = self.read_byte()?;
                Ok(Token::Regexp { source, options })
            }
            Tag::Class => Ok(Token::Class(self.read_chunk()?)),
            Tag::Module => Ok(Token::Module(self.read_chunk()?)),
            Tag::ModuleOld => Ok(Token::OldModule(self.read_chunk()?)),
            Tag::Array => Ok(Token::BeginArray(self.read_len()?)),
            Tag::Hash => Ok(Token::BeginHash {
                len: self.read_len()?,
                has_default: false,
            }),
            Tag::HashDefault => Ok(Token::BeginHash {
                len: self.read_len()?,
                has_default: true,
            }),
            Tag::Struct => {
                let class = self.read_unique()?;
                let len = self.read_len()?;
                Ok(Token::BeginStruct { class, len })
            }
            Tag::Object => {
                let class = self.read_unique()?;
                let len = self.read_len()?;
                Ok(Token::BeginObject { class, len })
            }
            Tag::UserDef => {
                let class = self.read_unique()?;
                let data = self.read_chunk()?;
                Ok(Token::UserDefined { class, data })
            }
            Tag::UserMarshal => Ok(Token::BeginUserMarshal {
                class: self.read_unique()?,
            }),
            Tag::Data => Ok(Token::BeginData {
                class: self.read_unique()?,
            }),
            Tag::UClass => Ok(Token::BeginUClass {
                class: self.read_unique()?,
            }),
            Tag::Extended => Ok(Token::BeginExtended {
                module: self.read_unique()?,
            }),
            Tag::Ivar => Ok(Token::BeginIvar),
        }
    }
}
