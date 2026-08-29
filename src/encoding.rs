//! Ruby's declared string/regexp text encodings, represented as a compact
//! `u8` id instead of the encoding name string.
//!
//! This crate never transcodes: [`crate::arena::Kind::Str`]/
//! [`crate::arena::Kind::Regexp`] bytes are exactly what was on the wire, in
//! whatever encoding they were declared as. Converting them is the
//! embedder's job.
//!
//! IDs here are this crate's own fixed table, not Ruby's internal encoding
//! index (which isn't stable across Ruby versions). A name outside
//! [`ENCODING_NAMES`] still round-trips correctly via [`ENCODING_CUSTOM`].

/// Escape id: this value's encoding name isn't in [`ENCODING_NAMES`] - look
/// it up in `Arena::custom_encodings` instead.
pub const ENCODING_CUSTOM: u8 = u8::MAX;

/// `ASCII-8BIT`'s id - Ruby's "binary, no declared text encoding" tag, and
/// what a `Kind::Bytes` value (no `E`/`encoding` ivar at all) implicitly
/// means.
///
/// Also the zero value a fresh [`crate::arena::Node`] starts with, so an
/// untagged `Kind::Regexp`'s encoding byte defaults to it for free.
pub const ENCODING_ASCII_8BIT: u8 = 0;

/// `US-ASCII`'s id - what Ruby's `:E => false` ivar means.
pub const ENCODING_US_ASCII: u8 = 66;

/// `UTF-8`'s id - what Ruby's `:E => true` ivar means.
pub const ENCODING_UTF_8: u8 = 74;

/// Fixed name<->id table, generated from `Encoding.list` on Ruby 4.0 (a
/// strict superset of every encoding present back to at least Ruby 1.9.2 -
/// verified directly).
///
/// Append new names to the end only - never reorder or remove an entry - so
/// an id already stored in an `Arena` (or produced by another process
/// linking this crate) keeps meaning the same encoding.
pub const ENCODING_NAMES: [&[u8]; 103] = [
    b"ASCII-8BIT",
    b"Big5",
    b"Big5-HKSCS",
    b"Big5-UAO",
    b"CESU-8",
    b"CP50220",
    b"CP50221",
    b"CP51932",
    b"CP850",
    b"CP852",
    b"CP855",
    b"CP949",
    b"CP950",
    b"CP951",
    b"EUC-JIS-2004",
    b"EUC-JP",
    b"EUC-KR",
    b"EUC-TW",
    b"Emacs-Mule",
    b"GB12345",
    b"GB18030",
    b"GB1988",
    b"GB2312",
    b"GBK",
    b"IBM037",
    b"IBM437",
    b"IBM720",
    b"IBM737",
    b"IBM775",
    b"IBM852",
    b"IBM855",
    b"IBM857",
    b"IBM860",
    b"IBM861",
    b"IBM862",
    b"IBM863",
    b"IBM864",
    b"IBM865",
    b"IBM866",
    b"IBM869",
    b"ISO-2022-JP",
    b"ISO-2022-JP-2",
    b"ISO-2022-JP-KDDI",
    b"ISO-8859-1",
    b"ISO-8859-10",
    b"ISO-8859-11",
    b"ISO-8859-13",
    b"ISO-8859-14",
    b"ISO-8859-15",
    b"ISO-8859-16",
    b"ISO-8859-2",
    b"ISO-8859-3",
    b"ISO-8859-4",
    b"ISO-8859-5",
    b"ISO-8859-6",
    b"ISO-8859-7",
    b"ISO-8859-8",
    b"ISO-8859-9",
    b"KOI8-R",
    b"KOI8-U",
    b"MacJapanese",
    b"SJIS-DoCoMo",
    b"SJIS-KDDI",
    b"SJIS-SoftBank",
    b"Shift_JIS",
    b"TIS-620",
    b"US-ASCII",
    b"UTF-16",
    b"UTF-16BE",
    b"UTF-16LE",
    b"UTF-32",
    b"UTF-32BE",
    b"UTF-32LE",
    b"UTF-7",
    b"UTF-8",
    b"UTF8-DoCoMo",
    b"UTF8-KDDI",
    b"UTF8-MAC",
    b"UTF8-SoftBank",
    b"Windows-1250",
    b"Windows-1251",
    b"Windows-1252",
    b"Windows-1253",
    b"Windows-1254",
    b"Windows-1255",
    b"Windows-1256",
    b"Windows-1257",
    b"Windows-1258",
    b"Windows-31J",
    b"Windows-874",
    b"eucJP-ms",
    b"macCentEuro",
    b"macCroatian",
    b"macCyrillic",
    b"macGreek",
    b"macIceland",
    b"macRoman",
    b"macRomania",
    b"macThai",
    b"macTurkish",
    b"macUkraine",
    b"stateless-ISO-2022-JP",
    b"stateless-ISO-2022-JP-KDDI",
];

/// Looks up a Ruby encoding name's id in [`ENCODING_NAMES`]. `None` means
/// "not a known name" - callers fall back to [`ENCODING_CUSTOM`] plus
/// storing the raw name themselves.
#[allow(clippy::cast_possible_truncation)]
#[must_use]
pub fn encoding_id(name: &[u8]) -> Option<u8> {
    ENCODING_NAMES.iter().position(|&n| n == name).map(|i| i as u8)
}

const _: () = assert!(ENCODING_NAMES.len() < ENCODING_CUSTOM as usize);

/// The name a known id was assigned from, or `None` for [`ENCODING_CUSTOM`]
/// (look up the side table instead) or any other id outside the table.
#[must_use]
pub fn encoding_name(id: u8) -> Option<&'static [u8]> {
    ENCODING_NAMES.get(id as usize).copied()
}
