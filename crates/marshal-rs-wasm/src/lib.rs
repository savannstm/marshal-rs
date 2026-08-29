//! A loaded `Arena` is serialized straight to a `JsValue` inside the same
//! call, so its borrow never needs to outlive it; going the other way,
//! `serde_wasm_bindgen::from_value` never hands out borrowed data (a
//! `JsValue` has no linear-memory-backed byte slice to borrow from), so
//! deserializing already produces an `Arena<'static>` for free - no
//! `into_owned` needed anywhere in this crate.

use marshal_rs::Arena;
use serde::Serialize;
use wasm_bindgen::prelude::*;

fn to_js_error(error: impl core::fmt::Display) -> JsError {
    JsError::new(&error.to_string())
}

/// Parses a Marshal byte stream into its JSON-shaped envelope (see [`marshal_rs::ser`]).
///
/// # Errors
///
/// Throws if `bytes` isn't a well-formed Marshal 4.8 stream.
#[wasm_bindgen]
pub fn load(bytes: &[u8]) -> Result<JsValue, JsError> {
    let arena = marshal_rs::load(bytes).map_err(to_js_error)?;
    // The envelope is built via `serializer.serialize_map` (`marshal_rs::ser`), which
    // `serde-wasm-bindgen` turns into a JS `Map` unless told otherwise - plain objects are what
    // consumers actually want here (`value.__type`, not `value.get("__type")`).
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    arena.serialize(&serializer).map_err(to_js_error)
}

/// Serializes a JSON-shaped envelope value (see [`marshal_rs::ser`]) back to Marshal bytes.
///
/// # Errors
///
/// Throws if `value` isn't shaped like a `marshal-rs` envelope.
#[wasm_bindgen]
pub fn dump(value: JsValue) -> Result<Vec<u8>, JsError> {
    let arena: Arena<'static> = serde_wasm_bindgen::from_value(value).map_err(to_js_error)?;
    Ok(marshal_rs::dump(&arena))
}
