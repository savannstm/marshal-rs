use wasm_bindgen_test::wasm_bindgen_test;

/// `[1, "two", 3.0, [4], {9 => 10}]`: a Fixnum, an encoding-tagged Str, a Float, a nested Array and a Hash.
const MIXED_ARRAY: &[u8] = b"\x04\x08[\x0ai\x06I\"\x08two\x06:\x06ETf\x063[\x06i\x09{\x06i\x0ai\x0b";

#[wasm_bindgen_test]
fn value_round_trips_through_the_wasm_boundary() {
    let value = marshal_rs_wasm::load(MIXED_ARRAY).unwrap();
    let round_tripped = marshal_rs_wasm::dump(value).unwrap();
    assert_eq!(round_tripped, MIXED_ARRAY);
}

#[wasm_bindgen_test]
fn a_malformed_stream_throws() {
    assert!(marshal_rs_wasm::load(b"\x04\x090").is_err());
}

use js_sys::{JSON, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};

fn get(object: &JsValue, key: &str) -> JsValue {
    Reflect::get(object, &key.into()).unwrap()
}

#[wasm_bindgen_test]
fn a_file_of_several_streams_loads_and_dumps_as_an_array() {
    let bytes = b"\x04\x08i\x06\x04\x08[\x06T\x04\x080";
    let values = marshal_rs_wasm::load_many(bytes).unwrap();
    assert_eq!(values.len(), 3);
    assert_eq!(marshal_rs_wasm::dump_many(values).unwrap(), bytes);
    assert!(marshal_rs_wasm::load_many(b"\x04\x08i\x06\x04").is_err());
}

#[wasm_bindgen_test]
fn errors_carry_a_code_and_an_offset() {
    let error = marshal_rs_wasm::load(b"\x04\x080x").unwrap_err();
    assert_eq!(get(&error, "code").as_string().as_deref(), Some("trailing_data"));
    assert_eq!(get(&error, "offset").as_f64(), Some(3.0));

    let error = marshal_rs_wasm::load(b"\x04\x09").unwrap_err();
    assert_eq!(get(&error, "code").as_string().as_deref(), Some("invalid_header"));

    let not_an_envelope = JSON::parse(r#"{"__value":[1]}"#).unwrap();
    let error = marshal_rs_wasm::dump(not_an_envelope).unwrap_err();
    assert_eq!(get(&error, "code").as_string().as_deref(), Some("invalid_envelope"));
}

#[wasm_bindgen_test]
fn binary_payloads_are_uint8arrays_both_ways() {
    // "\xff\xfe" as binary bytes
    let value = marshal_rs_wasm::load(b"\x04\x08\"\x07\xff\xfe").unwrap();
    let payload = get(&value, "__value");
    assert!(payload.is_instance_of::<Uint8Array>());
    assert_eq!(payload.unchecked_into::<Uint8Array>().to_vec(), [0xff, 0xfe]);
    assert_eq!(marshal_rs_wasm::dump(value).unwrap(), b"\x04\x08\"\x07\xff\xfe");

    // a base64 string is accepted too
    let from_text = JSON::parse(r#"{"__type":"bytes","__value":"//4="}"#).unwrap();
    assert_eq!(marshal_rs_wasm::dump(from_text).unwrap(), b"\x04\x08\"\x07\xff\xfe");
}

#[wasm_bindgen_test]
fn envelope_keys_may_come_in_any_order() {
    let value = JSON::parse(r#"{"__value":[1,2],"__type":"array"}"#).unwrap();
    assert_eq!(marshal_rs_wasm::dump(value).unwrap(), b"\x04\x08[\x07i\x06i\x07");
}

#[wasm_bindgen_test]
fn marshal_dump_wrappers_round_trip() {
    let bytes = b"\x04\x08U:\x08Fooo:\x08Bar\x00";
    let value = marshal_rs_wasm::load(bytes).unwrap();
    assert_eq!(get(&value, "__type").as_string().as_deref(), Some("user_marshal"));
    assert_eq!(
        get(&get(&value, "__value"), "__class").as_string().as_deref(),
        Some("Bar")
    );
    assert_eq!(marshal_rs_wasm::dump(value).unwrap(), bytes);
}

#[wasm_bindgen_test]
fn a_name_that_is_not_utf8_round_trips() {
    let bytes = b"\x04\x08:\x07\xff\xfe";
    let value = marshal_rs_wasm::load(bytes).unwrap();
    assert_eq!(marshal_rs_wasm::dump(value).unwrap(), bytes);
}

#[wasm_bindgen_test]
fn nesting_up_to_the_limit_fits_in_the_wasm_stack() {
    let depth = 250;
    let mut bytes = b"\x04\x08".to_vec();
    for _ in 0..depth {
        bytes.extend_from_slice(b"[\x06");
    }
    bytes.push(b'0');
    let value = marshal_rs_wasm::load(&bytes).unwrap();
    assert_eq!(marshal_rs_wasm::dump(value).unwrap(), bytes);
    // one past the limit is an error, not a trap
    let mut too_deep = b"\x04\x08".to_vec();
    for _ in 0..300 {
        too_deep.extend_from_slice(b"[\x06");
    }
    too_deep.push(b'0');
    assert!(marshal_rs_wasm::load(&too_deep).is_err());
}
