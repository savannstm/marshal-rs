//! Streams the fuzz targets found. Each must load and dump back to the same bytes.
#![cfg(feature = "alloc")]

use marshal_rs::{dump, load};

/// `[bignum, I e:D u"\xc3\xa9" with encoding "Bogus", link to object 3]`. The `u` value sits inside an `I` wrap and an
/// `e` extension, so Ruby numbers it after its ivars: object 2 is the encoding name, object 3 the `u` value, and the
/// last element links to it.
const EXTENDED_USER_DEFINED_LINK: &[u8] = &[
    0x04, 0x08, 0x5b, 0x08, 0x6c, 0x2d, 0x07, 0x16, 0x16, 0x16, 0x00, 0x49, 0x65, 0x3a, 0x06, 0x44, 0x75, 0x3b, 0x00,
    0x07, 0xc3, 0xa9, 0x06, 0x3a, 0x0d, 0x65, 0x6e, 0x63, 0x6f, 0x64, 0x69, 0x6e, 0x67, 0x22, 0x0a, 0x42, 0x6f, 0x67,
    0x75, 0x73, 0x40, 0x08,
];

#[test]
fn an_extended_user_defined_value_is_numbered_after_its_ivars() {
    let arena = load(EXTENDED_USER_DEFINED_LINK).expect("the stream loads");

    assert_eq!(dump(&arena), EXTENDED_USER_DEFINED_LINK);
}

/// A stream with an instance variable named `""`, which the reader accepts. The envelope reader refuses an empty
/// ivar name, so the serializer must refuse the value too rather than write JSON it can't read back.
#[cfg(feature = "serde")]
#[test]
fn an_empty_instance_variable_name_is_refused_on_serialize() {
    let data: &'static [u8] = include_bytes!("fixtures/fuzz-ser-empty-ivar.bin");
    let arena = load(data).expect("the stream loads");

    assert!(serde_json::to_string(&arena).is_err());
}
