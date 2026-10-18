//! The envelope through a binary format, which writes bytes as byte strings rather than base64 text. JSON never takes
//! those paths: a binary payload, a name that isn't UTF-8, and the error messages that name what a value should have been.

#![cfg(feature = "serde")]

use marshal_rs::{
    Arena, ValueRef, dump,
    ser::{BinaryMode, Envelope},
};

fn to_cbor(arena: &Arena<'_>) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::into_writer(
        &Envelope {
            arena,
            binary: BinaryMode::Bytes,
        },
        &mut out,
    )
    .expect("CBOR serialization failed");

    out
}

fn from_cbor(bytes: &[u8]) -> Arena<'static> {
    ciborium::from_reader(bytes).expect("CBOR deserialization failed")
}

/// Round-trips `arena` through CBOR, and checks that it dumps to the same Marshal bytes afterwards.
fn assert_round_trips(arena: &Arena<'_>) {
    let back = from_cbor(&to_cbor(arena));

    assert_eq!(dump(&back), dump(arena));
}

#[test]
fn binary_payloads_survive_cbor() {
    let mut arena = Arena::builder();
    let id = arena.push_bytes(vec![0, 1, 0xff]);
    arena.set_root(id);

    let back = from_cbor(&to_cbor(&arena));

    assert_eq!(ValueRef::root(&back).as_bytes(), Some(&[0u8, 1, 0xff][..]));
}

#[test]
fn text_that_is_not_utf8_survives_cbor_with_its_encoding() {
    let mut arena = Arena::builder();
    let id = arena.push_str_with_encoding_name(vec![0x82, 0xa0], b"Shift_JIS");
    arena.set_root(id);

    assert_round_trips(&arena);
}

#[test]
fn class_and_object_names_that_are_not_utf8_survive_cbor() {
    let mut arena = Arena::builder();
    let class = arena.push_class(vec![0xff, b'A']).expect("class name");
    let object = arena
        .push_object(vec![0xfe, b'B'], &[(b"@n".to_vec(), class)])
        .expect("object");
    arena.set_root(object);

    assert_round_trips(&arena);
}

#[test]
fn a_value_of_the_wrong_type_is_an_error_that_says_what_was_expected() {
    // A class name that is a number, not a name.
    let not_a_name = r#"{"__type":"object","__class":5,"__members":[]}"#;
    assert!(serde_json::from_str::<Arena<'static>>(not_a_name).is_err());

    // A text payload that is a number, not text or bytes.
    let not_text = r#"{"__type":"str","__value":5}"#;
    assert!(serde_json::from_str::<Arena<'static>>(not_text).is_err());

    // A bytes payload that is a boolean, not base64 or an array of bytes.
    let not_bytes = r#"{"__type":"bytes","__value":true}"#;
    assert!(serde_json::from_str::<Arena<'static>>(not_bytes).is_err());
}

#[test]
fn yaml_binary_values_arrive_as_owned_bytes() {
    // `!!binary` is YAML's byte string tag. `AAH/` is [0, 1, 0xff].
    let payload: Arena<'static> =
        yaml_serde::from_str("__type: bytes\n__value: !!binary AAH/\n").expect("YAML deserialization failed");
    assert_eq!(ValueRef::root(&payload).as_bytes(), Some(&[0u8, 1, 0xff][..]));
}

#[test]
fn a_name_given_as_a_cbor_number_is_rejected_through_the_binary_path() {
    let mut out = Vec::new();
    ciborium::into_writer(
        &serde_json::json!({"__type": "object", "__class": 5, "__members": []}),
        &mut out,
    )
    .expect("CBOR serialization failed");

    assert!(ciborium::from_reader::<Arena<'static>, _>(out.as_slice()).is_err());
}

/// The same bytes read through an owned `ciborium::Value`, so the loader works from a value tree rather than the input.
#[test]
fn owned_cbor_byte_strings_load_as_names_and_payloads() {
    let mut arena = Arena::builder();
    let class = arena.push_class(vec![0xff, b'A']).expect("class name");
    let payload = arena.push_bytes(vec![0, 1, 0xff]);
    let object = arena
        .push_object(vec![0xfe, b'B'], &[(b"@n".to_vec(), class), (b"@p".to_vec(), payload)])
        .expect("object");
    arena.set_root(object);

    let value: ciborium::Value = ciborium::from_reader(&to_cbor(&arena)[..]).expect("CBOR parses");
    let back: Arena<'static> = value.deserialized().expect("the owned value loads");

    assert_eq!(dump(&back), dump(&arena));
}

/// An envelope nested past the limit is refused on load, before its contents are read, so a hostile file can't drive
/// the loader into unbounded recursion.
#[test]
fn an_envelope_nested_past_the_limit_is_refused_on_load() {
    // 256 levels take about 1.9 MB of stack in a debug build, and more under ASan: more than the 2 MB default.
    let handle = std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(refuses_an_envelope_nested_past_the_limit)
        .expect("spawn the test thread");
    if let Err(payload) = handle.join() {
        std::panic::resume_unwind(payload);
    }
}

fn refuses_an_envelope_nested_past_the_limit() {
    let text = |s: &str| ciborium::Value::Text(s.to_owned());

    let mut value = ciborium::Value::Array(Vec::new());
    for _ in 0..300 {
        value = ciborium::Value::Map(vec![
            (text("__type"), text("array")),
            (text("__value"), ciborium::Value::Array(vec![value])),
        ]);
    }

    let Err(error) = value.deserialized::<Arena<'static>>() else {
        panic!("a 300-deep envelope must not load");
    };

    assert!(error.to_string().contains("nested too deeply"), "{error}");
}
