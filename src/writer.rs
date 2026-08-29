//! The Marshal token writer: low-level, allocation-free byte emission.
//!
//! [`Writer`] mirrors [`crate::reader::Reader`]: it emits each wire-format
//! primitive but does no symbol/object-link deduplication - that's the
//! `alloc`-tier [`mod@crate::dump`] module's job.

use crate::wire::{self, Tag};
use thiserror::Error;

/// Destination for emitted bytes. Implemented for a fixed `&mut [u8]`
/// ([`SliceSink`], allocation-free) and, under `alloc`, for
/// `alloc::vec::Vec<u8>` directly.
#[allow(clippy::missing_errors_doc)]
pub trait Sink {
    type Error;
    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum WriteError {
    #[error("output buffer capacity exceeded")]
    BufferFull,
}

/// A [`Sink`] backed by a caller-provided fixed-capacity slice - no
/// allocation, suitable for `no_std` embedding.
pub struct SliceSink<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> SliceSink<'a> {
    #[must_use]
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0 }
    }

    #[must_use]
    pub fn written(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

impl Sink for SliceSink<'_> {
    type Error = WriteError;

    #[inline]
    fn write(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        let end = self.len.checked_add(bytes.len()).ok_or(WriteError::BufferFull)?;
        let dst = self.buf.get_mut(self.len..end).ok_or(WriteError::BufferFull)?;
        dst.copy_from_slice(bytes);
        self.len = end;
        Ok(())
    }
}

#[cfg(feature = "alloc")]
impl Sink for alloc::vec::Vec<u8> {
    type Error = core::convert::Infallible;

    #[inline]
    fn write(&mut self, bytes: &[u8]) -> Result<(), core::convert::Infallible> {
        self.extend_from_slice(bytes);
        Ok(())
    }
}

/// A thin wrapper adapting `std::io::Write` to [`Sink`].
#[cfg(feature = "std")]
pub struct IoSink<W>(pub W);

#[cfg(feature = "std")]
impl<W: std::io::Write> Sink for IoSink<W> {
    type Error = std::io::Error;

    #[inline]
    fn write(&mut self, bytes: &[u8]) -> Result<(), std::io::Error> {
        self.0.write_all(bytes)
    }
}

/// A low-level Marshal byte-stream emitter over any [`Sink`].
pub struct Writer<S: Sink> {
    sink: S,
}

#[allow(
    clippy::missing_errors_doc,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]
impl<S: Sink> Writer<S> {
    pub const fn new(sink: S) -> Self {
        Self { sink }
    }

    pub fn into_inner(self) -> S {
        self.sink
    }

    #[inline]
    pub fn write_header(&mut self) -> Result<(), S::Error> {
        self.sink.write(&[wire::MAJOR_VERSION, wire::MINOR_VERSION])
    }

    #[inline]
    pub fn write_byte(&mut self, byte: u8) -> Result<(), S::Error> {
        self.sink.write(core::slice::from_ref(&byte))
    }

    #[inline]
    pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), S::Error> {
        self.sink.write(bytes)
    }

    #[inline]
    pub fn write_tag(&mut self, tag: Tag) -> Result<(), S::Error> {
        self.write_byte(tag.byte())
    }

    #[inline]
    pub fn write_int(&mut self, value: i32) -> Result<(), S::Error> {
        let mut buf = [0u8; 5];
        let encoded = wire::encode_int(value, &mut buf);
        self.write_bytes(encoded)
    }

    /// Writes a length as a packed int. Lengths beyond `i32::MAX` cannot
    /// occur from real Marshal payloads (the format itself cannot represent
    /// them).
    #[inline]
    pub fn write_len(&mut self, len: u32) -> Result<(), S::Error> {
        self.write_int(len as i32)
    }

    #[inline]
    pub fn write_chunk(&mut self, bytes: &[u8]) -> Result<(), S::Error> {
        self.write_len(bytes.len() as u32)?;
        self.write_bytes(bytes)
    }

    #[inline]
    pub fn write_nil(&mut self) -> Result<(), S::Error> {
        self.write_tag(Tag::Nil)
    }

    #[inline]
    pub fn write_bool(&mut self, value: bool) -> Result<(), S::Error> {
        self.write_tag(if value { Tag::True } else { Tag::False })
    }

    #[inline]
    pub fn write_fixnum(&mut self, value: i32) -> Result<(), S::Error> {
        self.write_tag(Tag::Fixnum)?;
        self.write_int(value)
    }

    /// `magnitude_le` is the bignum's absolute value, little-endian bytes.
    /// Marshal packs the magnitude as 16-bit little-endian words, so an odd
    /// byte length is padded with a trailing zero.
    pub fn write_bignum(&mut self, negative: bool, magnitude_le: &[u8]) -> Result<(), S::Error> {
        self.write_tag(Tag::Bignum)?;
        self.write_byte(if negative {
            wire::SIGN_NEGATIVE
        } else {
            wire::SIGN_POSITIVE
        })?;
        let word_len = magnitude_le.len().div_ceil(2);
        self.write_len(word_len as u32)?;
        self.write_bytes(magnitude_le)?;
        if magnitude_le.len() % 2 == 1 {
            self.write_byte(0)?;
        }
        Ok(())
    }

    #[inline]
    pub fn write_float(&mut self, ascii: &[u8]) -> Result<(), S::Error> {
        self.write_tag(Tag::Float)?;
        self.write_chunk(ascii)
    }

    #[inline]
    pub fn write_string_bytes(&mut self, bytes: &[u8]) -> Result<(), S::Error> {
        self.write_tag(Tag::String)?;
        self.write_chunk(bytes)
    }

    #[inline]
    pub fn write_regexp(&mut self, source: &[u8], options: u8) -> Result<(), S::Error> {
        self.write_tag(Tag::Regexp)?;
        self.write_chunk(source)?;
        self.write_byte(options)
    }

    #[inline]
    pub fn write_class_name(&mut self, bytes: &[u8]) -> Result<(), S::Error> {
        self.write_tag(Tag::Class)?;
        self.write_chunk(bytes)
    }

    #[inline]
    pub fn write_module_name(&mut self, bytes: &[u8], old: bool) -> Result<(), S::Error> {
        self.write_tag(if old { Tag::ModuleOld } else { Tag::Module })?;
        self.write_chunk(bytes)
    }

    #[inline]
    pub fn write_symbol_new(&mut self, bytes: &[u8]) -> Result<(), S::Error> {
        self.write_tag(Tag::Symbol)?;
        self.write_chunk(bytes)
    }

    #[inline]
    pub fn write_symbol_link(&mut self, idx: u32) -> Result<(), S::Error> {
        self.write_tag(Tag::SymLink)?;
        self.write_len(idx)
    }

    #[inline]
    pub fn write_object_link(&mut self, idx: u32) -> Result<(), S::Error> {
        self.write_tag(Tag::Link)?;
        self.write_len(idx)
    }

    #[inline]
    pub fn write_array_header(&mut self, len: u32) -> Result<(), S::Error> {
        self.write_tag(Tag::Array)?;
        self.write_len(len)
    }

    #[inline]
    pub fn write_hash_header(&mut self, len: u32, has_default: bool) -> Result<(), S::Error> {
        self.write_tag(if has_default { Tag::HashDefault } else { Tag::Hash })?;
        self.write_len(len)
    }

    #[inline]
    pub fn write_ivar_wrap_tag(&mut self) -> Result<(), S::Error> {
        self.write_tag(Tag::Ivar)
    }

    #[inline]
    pub fn write_extended_tag(&mut self) -> Result<(), S::Error> {
        self.write_tag(Tag::Extended)
    }

    #[inline]
    pub fn write_uclass_tag(&mut self) -> Result<(), S::Error> {
        self.write_tag(Tag::UClass)
    }
}
