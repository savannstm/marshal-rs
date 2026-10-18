//! Loaded arenas are serialized to a `JsValue` within the same call, and `from_value` yields an
//! `Arena<'static>`, so `into_owned` is never needed.
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]

use js_sys::Reflect;
use marshal_rs::{
    Arena, ReadError,
    ser::{BinaryMode, Envelope},
};
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &str = r#"
/** A Marshal value: `nil`, a boolean and a fixnum are bare JS values, everything else an envelope. */
export type MarshalValue = null | boolean | number | MarshalEnvelope;

/** A name: a string, or its bytes when they are not valid UTF-8 (Ruby 1.8 allowed any). A name whose
 * encoding tag is not the default one is `{ __name, __encoding }` instead. */
export type MarshalName = string | Uint8Array | { __name: string | Uint8Array; __encoding: string | Uint8Array };

export interface MarshalEnvelope {
    __type:
        | "bignum" | "float" | "bytes" | "str" | "symbol" | "regexp" | "array" | "hash"
        | "struct" | "object" | "class" | "module" | "user_marshal" | "data";
    __class?: MarshalName;
    __flags?: ("old_module" | "user_class" | "user_defined")[];
    __extensions?: MarshalName[];
    __ivars?: [MarshalName, MarshalValue][];
    __encoding?: string | Uint8Array;
    /** Input only: on a text `__value` (`str`, `float`, `symbol`, `class`, `module`), says the text is base64 of binary bytes.
     * `load` never sets it: binary text comes back as a `Uint8Array`. */
    __base64?: boolean;
    /** Payload by `__type`; `bytes` and binary regexp sources are `Uint8Array`s. */
    __value?: string | Uint8Array | MarshalValue | MarshalValue[] | [MarshalValue, MarshalValue][] | [Uint8Array, number];
    __members?: [MarshalName, MarshalValue][];
    __default?: MarshalValue;
}

/** What `load`, `loadMany` and `dump` throw. */
export interface MarshalError extends Error {
    /** `unexpected_eof`, `invalid_header`, `invalid_tag`, `symbol_table_full`, `unknown_symbol_link`,
     * `unknown_object_link`, `length_overflow`, `negative_length`, `trailing_data`, `empty_name`, `unsupported`, or
     * `invalid_envelope` when `dump` is given something that is not an envelope. */
    code: string;
    /** Byte offset into the failing stream, for `unexpected_eof`, `invalid_tag`, `trailing_data`, `empty_name` and `negative_length`. */
    offset?: number;
}
"#;

/// An `Error` carrying a machine-readable `code` and, where there is one, the byte `offset`.
fn marshal_error(code: &str, offset: Option<usize>, message: &str) -> JsValue {
    let error = js_sys::Error::new(message);
    // Setting a property on a fresh `Error` cannot fail.
    let _ = Reflect::set(&error, &"code".into(), &code.into());
    if let Some(offset) = offset {
        #[allow(clippy::cast_precision_loss)]
        let _ = Reflect::set(&error, &"offset".into(), &JsValue::from_f64(offset as f64));
    }
    error.into()
}

fn read_error(error: &ReadError) -> JsValue {
    let (code, offset) = match *error {
        ReadError::UnexpectedEof { offset } => ("unexpected_eof", Some(offset)),
        ReadError::InvalidHeader => ("invalid_header", None),
        ReadError::InvalidTag { offset, .. } => ("invalid_tag", Some(offset)),
        ReadError::SymbolTableFull => ("symbol_table_full", None),
        ReadError::UnknownSymbolLink { .. } => ("unknown_symbol_link", None),
        ReadError::UnknownObjectLink { .. } => ("unknown_object_link", None),
        ReadError::LengthOverflow => ("length_overflow", None),
        ReadError::NegativeLength { offset, .. } => ("negative_length", Some(offset)),
        ReadError::TrailingData { offset } => ("trailing_data", Some(offset)),
        ReadError::EmptyName { offset } => ("empty_name", Some(offset)),
        ReadError::Unsupported(_) => ("unsupported", None),
    };
    marshal_error(code, offset, &error.to_string())
}

fn envelope_error(error: impl core::fmt::Display) -> JsValue {
    marshal_error("invalid_envelope", None, &error.to_string())
}

/// Serializes `arena` to JS: plain objects (not `Map`s) for envelopes, `Uint8Array`s for binary payloads.
fn to_js(arena: &Arena<'_>) -> Result<JsValue, JsValue> {
    // Plain objects, not `Map`s, so consumers can write `value.__type`.
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    Envelope {
        arena,
        binary: BinaryMode::Bytes,
    }
    .serialize(&serializer)
    .map_err(envelope_error)
}

fn from_js(value: JsValue) -> Result<Arena<'static>, JsValue> {
    serde_wasm_bindgen::from_value(value).map_err(envelope_error)
}

/// Parses a Marshal byte stream into its JSON-shaped envelope (see [`marshal_rs::ser`]).
///
/// # Errors
///
/// Throws a `MarshalError` if `bytes` isn't exactly one well-formed Marshal 4.8 stream.
#[wasm_bindgen(unchecked_return_type = "MarshalValue")]
pub fn load(bytes: &[u8]) -> Result<JsValue, JsValue> {
    let arena = marshal_rs::load(bytes).map_err(|error| read_error(&error))?;
    to_js(&arena)
}

/// Parses every Marshal stream concatenated in `bytes` - such as an RPG Maker save file - into an
/// array of envelopes.
///
/// # Errors
///
/// Throws a `MarshalError` if any of the streams isn't well formed.
#[wasm_bindgen(js_name = loadMany, unchecked_return_type = "MarshalValue[]")]
pub fn load_many(bytes: &[u8]) -> Result<Vec<JsValue>, JsValue> {
    marshal_rs::load_many(bytes)
        .map_err(|error| read_error(&error))?
        .iter()
        .map(to_js)
        .collect()
}

/// Serializes a JSON-shaped envelope value (see [`marshal_rs::ser`]) back to Marshal bytes.
///
/// # Errors
///
/// Throws a `MarshalError` (`invalid_envelope`) if `value` isn't shaped like a `marshal-rs` envelope.
#[wasm_bindgen]
pub fn dump(#[wasm_bindgen(unchecked_param_type = "MarshalValue")] value: JsValue) -> Result<Vec<u8>, JsValue> {
    Ok(marshal_rs::dump(&from_js(value)?))
}

/// Serializes envelopes back to Marshal bytes and concatenates them: the inverse of `loadMany`.
///
/// # Errors
///
/// Throws a `MarshalError` (`invalid_envelope`) if any of the values isn't shaped like an envelope.
#[wasm_bindgen(js_name = dumpMany)]
pub fn dump_many(
    #[wasm_bindgen(unchecked_param_type = "MarshalValue[]")] values: Vec<JsValue>,
) -> Result<Vec<u8>, JsValue> {
    let mut out = Vec::new();
    for value in values {
        out.extend_from_slice(&marshal_rs::dump(&from_js(value)?));
    }
    Ok(out)
}
