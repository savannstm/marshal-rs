use wasm_bindgen_test::wasm_bindgen_test;

/// `[1, "two", 3.0, [4], {9 => 10}]`, from `tests/roundtrip.rs`'s `array_mixed_contents` -
/// exercises Fixnum, an encoding-tagged Str, Float, a nested Array, and a Hash all at once.
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
