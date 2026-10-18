//! A stackless, pull-based Marshal tokenizer over a byte slice.
//!
//! Each [`Reader::next`] call yields one [`Token`]; the caller sequences nested values using their declared lengths.
//! An `Ivar` wrapper's pairs come after the wrapped value, so read the value first, then [`Reader::next_ivar_count`].

use crate::wire::{self, Tag};
use thiserror::Error;

/// A byte span into the input buffer, used by [`SymbolTable`] implementations
/// to record where a symbol's bytes live without copying them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span {
    pub offset: u32,
    pub len: u32,
}

/// Storage for the symbol table that `SymLink` back-references index into.
/// Implemented for a fixed slice ([`FixedSymbolTable`]) and, under `alloc`, for `Vec<Span>`.
#[allow(clippy::missing_errors_doc)]
pub trait SymbolTable {
    fn push(&mut self, span: Span) -> Result<u32, ReadError>;
    fn get(&self, idx: u32) -> Option<Span>;
}

/// A [`SymbolTable`] over a caller-provided slice. Returns [`ReadError::SymbolTableFull`] when it runs out.
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
    // Plain `self.get` would resolve to this trait method (u32 index), so the slice is needed.
    // Stable clippy doesn't know this nightly lint, so `unknown_lints` covers that case.
    #[allow(unknown_lints, clippy::unnecessary_as_slice)]
    fn get(&self, idx: u32) -> Option<Span> {
        self.as_slice().get(idx as usize).copied()
    }
}

/// A single Marshal wire-format event, borrowing its byte payloads from the
/// buffer the [`Reader`] was constructed with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ReadError {
    #[error("unexpected end of input at byte {offset}")]
    UnexpectedEof { offset: usize },
    #[error("not a Marshal 4.8 byte stream (bad 2-byte header)")]
    InvalidHeader,
    /// A byte that is no type tag, or a known tag where a symbol must be (such as a nested `I`).
    #[error("unexpected type tag {byte:#04x} at byte {offset}")]
    InvalidTag { byte: u8, offset: usize },
    #[error("symbol table capacity exceeded")]
    SymbolTableFull,
    #[error("symbol link {idx} has no matching symbol")]
    UnknownSymbolLink { idx: u32 },
    #[error("object link {idx} has no matching object")]
    UnknownObjectLink { idx: u32 },
    #[error("declared length exceeds remaining input")]
    LengthOverflow,
    /// A length or count is negative. Ruby reads a negative instance-variable count as no pairs and loads such a
    /// stream; no valid stream has one, so this crate refuses it.
    #[error("negative length {value} at byte {offset}")]
    NegativeLength { value: i32, offset: usize },
    /// A complete value was read, but bytes follow it - see `load_many` for files that hold several.
    #[error("unexpected data after the value, at byte {offset}")]
    TrailingData { offset: usize },
    /// A construct the format allows but this crate does not implement (e.g. a symbol with a non-encoding ivar).
    #[error("unsupported construct: {0}")]
    Unsupported(&'static str),
    /// A class or module name is empty. Ruby cannot look one up, so it refuses such a stream too.
    #[error("empty class or module name at byte {offset}")]
    EmptyName { offset: usize },
}

/// How a symbol declared its encoding. Ruby writes a symbol with a non-ASCII name inside `TYPE_IVAR`, with the
/// encoding as its one instance variable; an ASCII or binary symbol is written bare.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolEncoding<'a> {
    /// No instance variable: US-ASCII for an ASCII name, ASCII-8BIT otherwise.
    Bare,
    /// `E => true`.
    Utf8,
    /// `E => false`.
    UsAscii,
    /// `encoding => "<name>"`, with the name written here. That string is an object like any other, so it takes a
    /// slot in the object-link table (right after whatever object the symbol belongs to).
    Named(&'a [u8]),
    /// `encoding => @n`: the name is the string an earlier object, `n` in the object-link table, holds.
    NamedLink(u32),
}

/// A stackless pull-parser over a Marshal 4.8 byte stream.
pub struct Reader<'a, 'b> {
    buf: &'a [u8],
    pos: usize,
    symbols: &'b mut dyn SymbolTable,
    last_symbol: u32,
    last_encoding: SymbolEncoding<'a>,
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
        // Lengths are packed `i32`s and symbol spans use `u32` offsets, so a larger buffer could not be
        // addressed correctly.
        if buf.len() > i32::MAX as usize {
            return Err(ReadError::LengthOverflow);
        }
        Ok(Self {
            buf,
            pos: 2,
            symbols,
            last_symbol: 0,
            last_encoding: SymbolEncoding::Bare,
        })
    }

    #[inline]
    #[must_use]
    pub const fn position(&self) -> usize {
        self.pos
    }

    /// Symbol-table index of the symbol the last token resolved: a `Symbol` token, or the class, module or
    /// name inside one. A back-reference reuses an earlier index, so a per-symbol value can be found by index
    /// instead of by comparing bytes. Meaningless before the first such token.
    #[inline]
    #[must_use]
    pub const fn last_symbol(&self) -> u32 {
        self.last_symbol
    }

    /// How the symbol the last token resolved declared its encoding (see [`Reader::last_symbol`]). A
    /// back-reference to a symbol read earlier is [`SymbolEncoding::Bare`] here: the encoding belongs to the
    /// first occurrence.
    #[inline]
    #[must_use]
    pub const fn last_symbol_encoding(&self) -> SymbolEncoding<'a> {
        self.last_encoding
    }

    /// Bytes not yet consumed.
    #[inline]
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.buf.len() - self.pos
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

    /// Reads a non-negative length that must fit in the remaining input, so a hostile length never reaches `with_capacity`.
    #[inline]
    fn read_len(&mut self) -> Result<u32, ReadError> {
        let offset = self.pos;
        let n = self.read_int()?;
        if n < 0 {
            return Err(ReadError::NegativeLength { value: n, offset });
        }
        if n as usize > self.buf.len().saturating_sub(self.pos) {
            return Err(ReadError::LengthOverflow);
        }
        Ok(n as u32)
    }

    /// Reads a symlink or object-link index. Only non-negativity is checked here; range is checked on resolution.
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

    /// A class or module name written as a plain chunk (`c`, `m`, `M`). It must not be empty.
    fn read_path(&mut self) -> Result<&'a [u8], ReadError> {
        let offset = self.pos;
        let name = self.read_chunk()?;
        if name.is_empty() {
            return Err(ReadError::EmptyName { offset });
        }
        Ok(name)
    }

    /// A class or module name written as a symbol or symlink, at a class position. It must not be empty.
    fn read_class(&mut self) -> Result<&'a [u8], ReadError> {
        let offset = self.pos;
        let name = self.read_unique()?;
        if name.is_empty() {
            return Err(ReadError::EmptyName { offset });
        }
        Ok(name)
    }

    fn read_tag(&mut self) -> Result<Tag, ReadError> {
        let offset = self.pos;
        let byte = self.read_byte()?;
        Tag::from_byte(byte).ok_or(ReadError::InvalidTag { byte, offset })
    }

    /// Reads a `Symbol` or `SymLink` tag, returning its bytes. A non-ASCII symbol's `TYPE_IVAR` encoding
    /// wrapper is read here too; see [`Reader::last_symbol_encoding`].
    fn read_unique(&mut self) -> Result<&'a [u8], ReadError> {
        self.last_encoding = SymbolEncoding::Bare;
        let tag = self.read_tag()?;
        if tag != Tag::Ivar {
            return self.read_symbol_body(tag);
        }
        let inner = self.read_tag()?;
        let bytes = self.read_symbol_body(inner)?;
        let symbol = self.last_symbol;
        let mut encoding = SymbolEncoding::Bare;
        for _ in 0..self.read_len()? {
            let name_tag = self.read_tag()?;
            let name = self.read_symbol_body(name_tag)?;
            encoding = match (self.read_tag()?, name) {
                (Tag::True, b"E") => SymbolEncoding::Utf8,
                (Tag::False, b"E") => SymbolEncoding::UsAscii,
                (Tag::String, b"encoding") => SymbolEncoding::Named(self.read_chunk()?),
                (Tag::Link, b"encoding") => SymbolEncoding::NamedLink(self.read_index()?),
                // Any other flag-like value is dropped. Anything else is an object that would take a slot in
                // the object-link table, which this reader does not track.
                (Tag::True | Tag::False | Tag::Nil, _) => encoding,
                _ => {
                    return Err(ReadError::Unsupported(
                        "symbol with an instance variable other than its encoding",
                    ));
                }
            };
        }
        self.last_symbol = symbol;
        self.last_encoding = encoding;
        Ok(bytes)
    }

    /// Reads the rest of a `Symbol`/`SymLink` whose `tag` was already read.
    fn read_symbol_body(&mut self, tag: Tag) -> Result<&'a [u8], ReadError> {
        match tag {
            Tag::Symbol => {
                let bytes = self.read_chunk()?;
                let span = Span {
                    offset: (self.pos - bytes.len()) as u32,
                    len: bytes.len() as u32,
                };
                self.last_symbol = self.symbols.push(span)?;
                Ok(bytes)
            }
            Tag::SymLink => {
                let idx = self.read_index()?;
                let span = self.symbols.get(idx).ok_or(ReadError::UnknownSymbolLink { idx })?;
                self.last_symbol = idx;
                Ok(&self.buf[span.offset as usize..(span.offset + span.len) as usize])
            }
            other => Err(ReadError::InvalidTag {
                byte: other.byte(),
                offset: self.pos - 1,
            }),
        }
    }

    /// Reads a symbol where the format requires one (a member or instance-variable name), resolving
    /// a `SymLink` and accepting the encoding wrapper Ruby puts around non-ASCII names.
    pub fn next_symbol(&mut self) -> Result<&'a [u8], ReadError> {
        self.read_unique()
    }

    /// Reads the ivar-count + pairs that follow an [`Token::BeginIvar`]'s
    /// fully-resolved wrapped value. The caller then reads that many
    /// (symbol, value) pairs itself via [`Reader::next`].
    pub fn next_ivar_count(&mut self) -> Result<u32, ReadError> {
        self.read_len()
    }

    /// Reads the next token. There is no end-of-stream token; the caller tracks how many are due.
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
                // Any byte but `+` is negative, as Ruby 1.8.7 and 1.9.3 read it; Ruby 4 rejects other bytes instead.
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
            Tag::Class => Ok(Token::Class(self.read_path()?)),
            Tag::Module => Ok(Token::Module(self.read_path()?)),
            Tag::ModuleOld => Ok(Token::OldModule(self.read_path()?)),
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
                let class = self.read_class()?;
                let len = self.read_len()?;
                Ok(Token::BeginStruct { class, len })
            }
            Tag::Object => {
                let class = self.read_class()?;
                let len = self.read_len()?;
                Ok(Token::BeginObject { class, len })
            }
            Tag::UserDef => {
                let class = self.read_class()?;
                let data = self.read_chunk()?;
                Ok(Token::UserDefined { class, data })
            }
            Tag::UserMarshal => Ok(Token::BeginUserMarshal {
                class: self.read_class()?,
            }),
            Tag::Data => Ok(Token::BeginData {
                class: self.read_class()?,
            }),
            Tag::UClass => Ok(Token::BeginUClass {
                class: self.read_class()?,
            }),
            Tag::Extended => Ok(Token::BeginExtended {
                module: self.read_class()?,
            }),
            // A symbol wrapped in its encoding is one token, not a wrapper around one.
            Tag::Ivar
                if matches!(
                    self.buf.get(self.pos).copied().and_then(Tag::from_byte),
                    Some(Tag::Symbol | Tag::SymLink)
                ) =>
            {
                self.pos -= 1;
                Ok(Token::Symbol(self.read_unique()?))
            }
            Tag::Ivar => Ok(Token::BeginIvar),
        }
    }
}
