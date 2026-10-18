//! Regression tests for what fuzzing (`fuzz/`) found: hostile input that used to overflow the stack,
//! hang or panic, and must be an error (or a normal result) instead.
#![cfg(feature = "alloc")]

#[cfg(feature = "serde")]
use marshal_rs::Arena;
use marshal_rs::{ValueRef, dump, load};

const HEADER: [u8; 2] = [4, 8];

/// `[[[...[nil]...]]]`, `depth` arrays deep. Two bytes a level: a 200 KB file is 100k levels.
fn nested_arrays(depth: usize) -> Vec<u8> {
    let mut bytes = HEADER.to_vec();
    for _ in 0..depth {
        bytes.extend_from_slice(b"[\x06");
    }
    bytes.push(b'0');
    bytes
}

/// An array whose only element is itself - valid Marshal, it's what `a = []; a << a` dumps to.
fn cyclic_array() -> Vec<u8> {
    let mut bytes = HEADER.to_vec();
    bytes.extend_from_slice(b"[\x06@\x00");
    bytes
}

/// `levels` arrays, each holding the next one twice (the second time as a link). Tiny on the wire,
/// `2^levels` values when walked as a tree.
#[cfg(feature = "serde")]
fn doubling_arrays(levels: u8) -> Vec<u8> {
    fn encode(level: u8, levels: u8, out: &mut Vec<u8>) {
        if level == levels {
            out.push(b'0');
            return;
        }
        out.extend_from_slice(b"[\x07");
        encode(level + 1, levels, out);
        if level + 1 == levels {
            // The child is a nil, which isn't an object and can't be linked to.
            out.push(b'0');
        } else {
            // Links count objects in the order they started: this array is object `level`, its
            // child `level + 1`. Small indices are stored as `index + 5`.
            out.extend_from_slice(&[b'@', level + 1 + 5]);
        }
    }

    let mut bytes = HEADER.to_vec();
    encode(0, levels, &mut bytes);
    bytes
}

#[test]
fn deep_input_dumps_without_overflowing_the_stack() {
    let bytes = nested_arrays(200_000);
    let arena = load(&bytes).expect("a deep stream loads");
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn cyclic_input_dumps_back_to_itself() {
    let bytes = cyclic_array();
    let arena = load(&bytes).unwrap();
    assert_eq!(dump(&arena), bytes);
}

#[cfg(feature = "serde")]
#[test]
fn deep_input_does_not_serialize_into_a_stack_overflow() {
    let bytes = nested_arrays(200_000);
    let arena = load(&bytes).unwrap();
    assert!(serde_json::to_string(&arena).is_err());
}

#[cfg(feature = "serde")]
#[test]
fn cyclic_input_does_not_serialize_forever() {
    let bytes = cyclic_array();
    let arena = load(&bytes).unwrap();
    assert!(serde_json::to_string(&arena).is_err());
}

#[cfg(feature = "serde")]
#[test]
fn shared_values_do_not_serialize_into_an_exponential_tree() {
    let bytes = doubling_arrays(60);
    let arena = load(&bytes).unwrap();
    assert!(serde_json::to_string(&arena).is_err());
}

#[cfg(feature = "serde")]
#[test]
fn ordinary_nesting_still_serializes() {
    let bytes = nested_arrays(20);
    let arena = load(&bytes).unwrap();
    let json = serde_json::to_string(&arena).unwrap();
    let back: Arena<'static> = serde_json::from_str(&json).unwrap();
    assert_eq!(dump(&back), nested_arrays(20));
}

/// A value claiming a class-carrying wrapper without a class used to index the symbol table with
/// `u32::MAX` when dumped.
#[cfg(feature = "serde")]
#[test]
fn json_flags_that_need_a_class_are_rejected_without_one() {
    for flag in ["user_class", "user_defined"] {
        let json = format!(r#"{{"__type":"bytes","__value":[1,2],"__flags":["{flag}"]}}"#);
        assert!(
            serde_json::from_str::<Arena<'static>>(&json).is_err(),
            "{flag} without a __class"
        );
    }
}

/// `user_defined` reads a blob, which only a bytes value has.
#[cfg(feature = "serde")]
#[test]
fn json_user_defined_array_is_rejected() {
    let json = r#"{"__type":"array","__value":[],"__class":"Foo","__flags":["user_defined"]}"#;
    assert!(serde_json::from_str::<Arena<'static>>(json).is_err());
}

#[test]
fn comparing_a_cyclic_array_with_itself_terminates() {
    let bytes = cyclic_array();
    let arena = load(&bytes).unwrap();
    let root = ValueRef::root(&arena);
    // `lookup` compares keys structurally.
    let mut with_hash = HEADER.to_vec();
    with_hash.extend_from_slice(b"{\x06[\x06@\x000"); // { [self] => nil }
    let hash = load(&with_hash).unwrap();
    let _ = ValueRef::root(&hash).lookup(root);
}

/// A bignum of `len` bytes (a whole number of words).
fn bignum_of(len: usize) -> Vec<u8> {
    assert!(len % 2 == 0 && (256..=65535).contains(&(len / 2)));
    let words = (len / 2) as u16;
    let mut bytes = HEADER.to_vec();
    bytes.extend_from_slice(b"l+\x02");
    bytes.extend_from_slice(&words.to_le_bytes());
    bytes.extend(core::iter::repeat(0xFF).take(len));
    bytes
}

/// Converting a bignum to decimal is quadratic: 64 KiB used to take half a minute.
#[cfg(feature = "serde")]
#[test]
fn huge_bignums_do_not_convert_to_decimal() {
    let bytes = bignum_of(64 << 10);
    let arena = load(&bytes).unwrap();
    let started = std::time::Instant::now();

    assert!(ValueRef::root(&arena).as_bigint_decimal().is_none());
    assert!(serde_json::to_string(&arena).is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(2));

    // The bytes are still there, and dump back unchanged.
    assert!(ValueRef::root(&arena).as_bignum_bytes().is_some());
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn bignums_within_the_limit_still_convert() {
    let bytes = bignum_of(4096);
    let arena = load(&bytes).unwrap();
    let decimal = ValueRef::root(&arena).as_bigint_decimal().unwrap();
    assert!(decimal.len() > 9_000);
    assert_eq!(marshal_rs::bignum::decimal_to_le_bytes(&decimal).unwrap().1.len(), 4096);
}

#[cfg(feature = "serde")]
#[test]
fn huge_decimal_strings_are_rejected() {
    let digits = "9".repeat(marshal_rs::bignum::MAX_CONVERTIBLE_DIGITS + 1);
    assert!(marshal_rs::bignum::decimal_to_le_bytes(&digits).is_none());
    let json = format!(r#"{{"__type":"bignum","__value":"{digits}"}}"#);
    assert!(serde_json::from_str::<Arena<'static>>(&json).is_err());
}

#[test]
fn nested_length_claims_cannot_reserve_more_than_the_input() {
    // Each level claims 50,000 elements, which fits the bytes left on its own, but together they
    // claim far more than the file holds.
    let mut bytes = HEADER.to_vec();
    for _ in 0..1000 {
        bytes.extend_from_slice(b"[\x02\x50\xC3");
    }
    bytes.extend(core::iter::repeat_n(b'0', 100_000));
    assert!(load(&bytes).is_err());
}

#[cfg(feature = "serde")]
#[test]
fn non_utf8_names_round_trip_byte_exact() {
    let mut arena = Arena::builder();
    let symbol = arena.push_symbol(vec![0xff, 0xfe]);
    arena.set_root(symbol);
    let json = serde_json::to_string(&arena).unwrap();
    let back: Arena<'static> = serde_json::from_str(&json).unwrap();
    assert_eq!(dump(&back), dump(&arena));
}

// What follows is hand-written on purpose: no Ruby writes these. Everything Ruby does write is in
// `ruby_fixtures.rs`.

#[test]
fn a_wrong_version_is_rejected() {
    assert!(load(b"\x04\x090").is_err());
}

#[test]
fn a_declared_length_past_the_end_is_an_error() {
    // Declared length 0x10, only 4 bytes actually present.
    assert!(load(b"\x04\x08\"\x10\xf0(\x8c(").is_err());
}

#[test]
fn bytes_after_the_value_are_an_error_for_load() {
    assert!(matches!(
        load(b"\x04\x080x"),
        Err(marshal_rs::ReadError::TrailingData { offset: 3 })
    ));
    assert!(load(b"\x04\x080").is_ok());
}

#[test]
fn markers_on_values_they_cannot_apply_to_are_rejected() {
    assert!(load(b"\x04\x08C:\x08Fooo:\x08Bar\x00").is_err());
    assert!(load(b"\x04\x08e:\x06Ai\x06").is_err());
    assert!(load(b"\x04\x08e:\x06A0").is_err());
}

#[test]
fn wrappers_that_lead_back_to_each_other_load_and_dump_without_hanging() {
    for bytes in [
        &b"\x04\x08U:\x08Foo@\x00"[..],
        // U:Foo wrapping U:Bar, which wraps a link back to U:Foo.
        b"\x04\x08U:\x08FooU:\x08Bar@\x00",
        // [U:Foo nil, U:Bar @1]: a wrapper whose body is an earlier wrapper.
        b"\x04\x08[\x07U:\x08Foo0U:\x08Bar@\x06",
    ] {
        let arena = load(bytes).unwrap();
        let _ = ValueRef::root(&arena).kind();
        assert_eq!(dump(&arena), bytes);
    }
}

/// Found by fuzzing (`fuzz/`): `M` with an empty name loaded, then could not be serialized back. Ruby refuses
/// such a class or module too.
#[test]
fn empty_class_and_module_names_are_an_error() {
    let cases: [&[u8]; 4] = [
        b"\x04\x08M\x00",
        b"\x04\x08c\x00",
        b"\x04\x08m\x00",
        b"\x04\x08o:\x00\x00",
    ];
    for bytes in cases {
        assert!(
            matches!(load(bytes), Err(marshal_rs::ReadError::EmptyName { .. })),
            "{bytes:?}"
        );
    }
    // An empty symbol is a valid name everywhere else.
    assert!(load(b"\x04\x08:\x00").is_ok());
}

/// Found by fuzzing (`fuzz/`): a bignum with high zero bytes loaded, then its JSON form dumped a different length.
/// Ruby never writes the padding, so it is dropped on load.
#[test]
fn padded_bignums_dump_without_their_padding() {
    let padded = dump(&load(b"\x04\x08l-\x08\xf9\x49\x4a\x01\x00\x00").unwrap());
    assert_eq!(padded, b"\x04\x08l-\x07\xf9\x49\x4a\x01");
    // Zero is never negative, and its magnitude is empty.
    assert_eq!(dump(&load(b"\x04\x08l-\x00").unwrap()), b"\x04\x08l+\x00");
    assert_eq!(dump(&load(b"\x04\x08l+\x02\x00\x00").unwrap()), b"\x04\x08l+\x00");
}

/// Ruby 1.8.7 and 1.9.3 read any sign byte other than `+` as negative, so that is the rule here. Ruby 4 rejects
/// such a byte and 2.7.8 reads it as positive; a damaged sign is not an error on this side.
#[test]
fn any_sign_byte_but_plus_is_negative() {
    for sign in [b'-', b'?', 0] {
        let bytes = [4, 8, b'l', sign, 0x06, 0x01, 0x00];
        assert_eq!(dump(&load(&bytes).unwrap()), b"\x04\x08l-\x06\x01\x00");
    }
}
