#![allow(clippy::cast_possible_wrap)]

use marshal_rs::wire::{Tag, decode_int, encode_int, packed_int_tail_len};

fn roundtrip(value: i32) {
    let mut buf = [0u8; 5];
    let encoded = encode_int(value, &mut buf);
    let lead = encoded[0] as i8;
    let tail = &encoded[1..];
    assert_eq!(tail.len(), packed_int_tail_len(lead));
    assert_eq!(decode_int(lead, tail), value, "value={value} encoded={encoded:?}");
}

#[test]
fn packed_int_roundtrip() {
    for value in [
        0,
        1,
        -1,
        122,
        -123,
        123,
        -124,
        255,
        -255,
        256,
        i32::from(i16::MAX),
        i32::from(i16::MIN),
        i32::MAX,
        i32::MIN,
        1 << 20,
        -(1 << 20),
    ] {
        roundtrip(value);
    }
}

#[test]
fn tag_from_byte_rejects_unknown() {
    assert!(Tag::from_byte(b'!').is_none());
    assert_eq!(Tag::from_byte(b'0'), Some(Tag::Nil));
}
