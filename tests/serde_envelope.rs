//! The envelope's less common shapes from the JSON side: nesting limits, keys that are not part of the format,
//! buffering when `__type` is not first, and names that are malformed or carry an encoding.

#![cfg(feature = "serde")]

use marshal_rs::{Arena, ValueRef};

fn from_json(json: &str) -> Result<Arena<'static>, serde_json::Error> {
    serde_json::from_str(json)
}

#[test]
fn an_envelope_nested_past_the_limit_is_rejected() {
    // Well past `MAX_NESTING` (256), with a valid innermost envelope so only the depth is wrong.
    let depth = 300;
    let json = format!(
        "{}{}{}",
        r#"{"__type":"array","__value":["#.repeat(depth),
        r#"{"__type":"array","__value":[]}"#,
        "]}".repeat(depth)
    );

    assert!(from_json(&json).is_err());
}

#[test]
fn an_envelope_buffers_nested_envelopes_whose_type_comes_last() {
    // `__type` is last, so the outer envelope is buffered and replayed, and its nested envelope goes through the same path.
    let out_of_order = r#"{"__value":[{"__type":"array","__value":[]}],"__type":"array"}"#;
    let in_order = r#"{"__type":"array","__value":[{"__type":"array","__value":[]}]}"#;

    let arena = from_json(out_of_order).unwrap();

    assert_eq!(serde_json::to_string(&arena).unwrap(), in_order);
}

#[test]
fn unknown_envelope_keys_are_ignored() {
    let arena = from_json(r#"{"__type":"array","__note":{"any":[1,2]},"__value":[]}"#).unwrap();

    assert_eq!(
        serde_json::to_string(&arena).unwrap(),
        r#"{"__type":"array","__value":[]}"#
    );
}

#[test]
fn the_old_module_flag_is_read_and_written() {
    // A module that Ruby wrote in the old `M` form.
    let arena = from_json(r#"{"__type":"module","__flags":["old_module"],"__value":"Mod"}"#).unwrap();
    let json = serde_json::to_string(&arena).unwrap();

    assert!(json.contains(r#""__flags":["old_module"]"#), "{json}");
}

#[test]
fn a_str_payload_may_be_base64_text() {
    let arena = from_json(r#"{"__type":"str","__base64":true,"__value":"AQI="}"#).unwrap();

    assert_eq!(ValueRef::root(&arena).as_bytes(), Some(&[1u8, 2][..]));
    assert!(from_json(r#"{"__type":"str","__base64":true,"__value":"!"}"#).is_err());
}

#[test]
fn a_name_object_needs_its_name() {
    // A name object holds `__name` or `__base64`; an encoding alone names nothing.
    assert!(from_json(r#"{"__type":"object","__class":{"__encoding":"UTF-8"},"__members":[]}"#).is_err());
    assert!(from_json(r#"{"__type":"object","__class":{"__name":"A","__oops":1},"__members":[]}"#).is_err());
}

#[test]
fn a_non_utf8_class_name_with_an_encoding_is_written_as_base64_and_encoding() {
    // Read back as bytes, the name has to come out in the same tagged shape.
    let json = r#"{"__type":"object","__class":{"__base64":"/w==","__encoding":"Shift_JIS"},"__members":[]}"#;
    let arena = from_json(json).unwrap();

    assert_eq!(serde_json::to_string(&arena).unwrap(), json);
}
