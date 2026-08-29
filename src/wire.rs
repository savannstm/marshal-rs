//! Marshal wire-format constants: type tags and the packed-integer codec.
//!
//! This module is `no_std` and allocation-free - it operates purely on byte
//! slices and fixed-size stack buffers, so it forms the base of the `core`
//! tier that can be embedded in freestanding C/C++ code.

/// Marshal format version this crate reads and writes (4.8).
pub const MAJOR_VERSION: u8 = 4;
pub const MINOR_VERSION: u8 = 8;

/// A validated Marshal wire-format type tag.
///
/// Constructed only via [`Tag::from_byte`], so a `Tag` value is always one of
/// the 25 bytes Ruby's Marshal format defines - unlike v2, which
/// `transmute`d an arbitrary input byte into an enum with unrelated variants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Tag {
    Nil = b'0',
    True = b'T',
    False = b'F',
    Fixnum = b'i',
    Extended = b'e',
    UClass = b'C',
    Object = b'o',
    Data = b'd',
    UserDef = b'u',
    UserMarshal = b'U',
    Float = b'f',
    Bignum = b'l',
    String = b'"',
    Regexp = b'/',
    Array = b'[',
    Hash = b'{',
    HashDefault = b'}',
    Struct = b'S',
    ModuleOld = b'M',
    Class = b'c',
    Module = b'm',
    Symbol = b':',
    SymLink = b';',
    Ivar = b'I',
    Link = b'@',
}

impl Tag {
    #[inline]
    #[must_use]
    pub const fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            b'0' => Self::Nil,
            b'T' => Self::True,
            b'F' => Self::False,
            b'i' => Self::Fixnum,
            b'e' => Self::Extended,
            b'C' => Self::UClass,
            b'o' => Self::Object,
            b'd' => Self::Data,
            b'u' => Self::UserDef,
            b'U' => Self::UserMarshal,
            b'f' => Self::Float,
            b'l' => Self::Bignum,
            b'"' => Self::String,
            b'/' => Self::Regexp,
            b'[' => Self::Array,
            b'{' => Self::Hash,
            b'}' => Self::HashDefault,
            b'S' => Self::Struct,
            b'M' => Self::ModuleOld,
            b'c' => Self::Class,
            b'm' => Self::Module,
            b':' => Self::Symbol,
            b';' => Self::SymLink,
            b'I' => Self::Ivar,
            b'@' => Self::Link,
            _ => return None,
        })
    }

    #[inline]
    #[must_use]
    pub const fn byte(self) -> u8 {
        self as u8
    }
}

/// Bignum positive sign byte.
pub const SIGN_POSITIVE: u8 = b'+';
/// Bignum negative sign byte.
pub const SIGN_NEGATIVE: u8 = b'-';

/// Regexp option bits, as packed into the single flags byte after a Regexp's
/// source string.
pub const REGEXP_IGNORE_CASE: u8 = 1 << 0;
pub const REGEXP_EXTENDED: u8 = 1 << 1;
pub const REGEXP_MULTILINE: u8 = 1 << 2;

/// Decode a Marshal packed integer given its already-consumed leading byte
/// and up to 4 immediately-following bytes (`tail`).
///
/// `tail` must contain at least as many bytes as the leading byte demands -
/// callers are expected to have already bounds-checked via
/// [`packed_int_tail_len`].
#[allow(clippy::cast_sign_loss)]
#[inline]
#[must_use]
pub fn decode_int(lead: i8, tail: &[u8]) -> i32 {
    match lead {
        0 => 0,
        1..=4 => {
            let size = lead as usize;
            let mut buf = [0u8; 4];
            buf[..size].copy_from_slice(&tail[..size]);
            i32::from_le_bytes(buf)
        }
        -4..=-1 => {
            let size = lead.unsigned_abs() as usize;
            let mut buf = [0xffu8; 4];
            buf[..size].copy_from_slice(&tail[..size]);
            i32::from_le_bytes(buf)
        }
        _ => {
            if lead > 0 {
                i32::from(lead) - 5
            } else {
                i32::from(lead) + 5
            }
        }
    }
}

/// Number of extra bytes [`decode_int`] needs beyond the leading byte, given
/// that leading byte.
#[allow(clippy::cast_sign_loss)]
#[inline]
#[must_use]
pub const fn packed_int_tail_len(lead: i8) -> usize {
    match lead {
        1..=4 => lead as usize,
        -4..=-1 => (-lead) as usize,
        _ => 0,
    }
}

/// Encode a Marshal packed integer into `buf` (which must be at least 5
/// bytes), returning the slice actually written.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]
#[inline]
#[must_use]
pub fn encode_int(value: i32, buf: &mut [u8; 5]) -> &[u8] {
    if value == 0 {
        buf[0] = 0;
        return &buf[..1];
    }
    if (1..123).contains(&value) {
        buf[0] = (value + 5) as u8;
        return &buf[..1];
    }
    if (-123..0).contains(&value) {
        buf[0] = (value - 5) as u8;
        return &buf[..1];
    }

    let mut v = value;
    let mut i = 1usize;
    loop {
        buf[i] = (v & 0xff) as u8;
        v >>= 8;
        if v == 0 {
            buf[0] = i as u8;
            break;
        }
        if v == -1 {
            buf[0] = (-(i as i32)) as u8;
            break;
        }
        i += 1;
    }
    &buf[..=i]
}
