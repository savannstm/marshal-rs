#![cfg(feature = "alloc")]

use marshal_rs::{
    ENCODING_ASCII_8BIT, ENCODING_CUSTOM, ENCODING_US_ASCII, ENCODING_UTF_8, default_symbol_encoding, encoding_id,
    encoding_name,
};

#[test]
fn well_known_constants_match_their_names() {
    assert_eq!(encoding_id(b"UTF-8"), Some(ENCODING_UTF_8));
    assert_eq!(encoding_name(ENCODING_UTF_8), Some(&b"UTF-8"[..]));
    assert_eq!(encoding_id(b"ASCII-8BIT"), Some(ENCODING_ASCII_8BIT));
    assert_eq!(encoding_id(b"US-ASCII"), Some(ENCODING_US_ASCII));
    assert_eq!(encoding_id(b"NoSuchEncoding"), None);
}

#[test]
fn every_table_id_round_trips_through_its_name() {
    for id in 0..=u8::MAX {
        if let Some(name) = encoding_name(id) {
            assert_eq!(encoding_id(name), Some(id));
        }
    }
    assert_eq!(encoding_name(ENCODING_CUSTOM), None);
}

#[test]
fn untagged_bytes_are_utf8_only_when_they_are_non_ascii_utf8() {
    assert_eq!(default_symbol_encoding(b"plain"), ENCODING_ASCII_8BIT);
    assert_eq!(default_symbol_encoding("é".as_bytes()), ENCODING_UTF_8);
    assert_eq!(default_symbol_encoding(b"\xff"), ENCODING_ASCII_8BIT);
}
