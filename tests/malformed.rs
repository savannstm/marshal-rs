//! Regression tests for what fuzzing (`fuzz/`) found: hostile input that used to overflow the stack,
//! hang or panic, and must be an error (or a normal result) instead.
#![cfg(feature = "serde")]

use marshal_rs::{dump, load, Arena, ValueRef};

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

#[test]
fn deep_input_does_not_serialize_into_a_stack_overflow() {
    let bytes = nested_arrays(200_000);
    let arena = load(&bytes).unwrap();
    assert!(serde_json::to_string(&arena).is_err());
}

#[test]
fn cyclic_input_does_not_serialize_forever() {
    let bytes = cyclic_array();
    let arena = load(&bytes).unwrap();
    assert!(serde_json::to_string(&arena).is_err());
}

#[test]
fn shared_values_do_not_serialize_into_an_exponential_tree() {
    let bytes = doubling_arrays(60);
    let arena = load(&bytes).unwrap();
    assert!(serde_json::to_string(&arena).is_err());
}

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
#[test]
fn json_flags_that_need_a_class_are_rejected_without_one() {
    for flag in ["user_class", "data", "user_marshal", "user_defined"] {
        let json = format!(r#"{{"__type":"bytes","__value":[1,2],"__flags":["{flag}"]}}"#);
        assert!(
            serde_json::from_str::<Arena<'static>>(&json).is_err(),
            "{flag} without a __class"
        );
    }
}

/// `user_defined` reads a blob, which only a bytes value has.
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

#[test]
fn huge_decimal_strings_are_rejected() {
    let digits = "9".repeat(marshal_rs::bignum::MAX_CONVERTIBLE_DIGITS + 1);
    assert!(marshal_rs::bignum::decimal_to_le_bytes(&digits).is_none());
    let json = format!(r#"{{"__type":"bignum","__value":"{digits}"}}"#);
    assert!(serde_json::from_str::<Arena<'static>>(&json).is_err());
}
