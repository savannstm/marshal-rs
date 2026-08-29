//! Streaming serde round-trip: Marshal bytes -> `Arena` -> JSON -> `Arena` ->
//! Marshal bytes.

use marshal_rs::{Arena, ValueRef, dump, load};

fn roundtrip_via_json(bytes: &[u8]) -> Vec<u8> {
    let arena = load(bytes).expect("should load");
    let json = serde_json::to_string(&arena).expect("should serialize");
    let arena2: Arena<'static> = serde_json::from_str(&json).expect("should deserialize");
    dump(&arena2)
}

#[test]
fn trivial_values_are_bare_json() {
    let arena = load(b"\x04\x080").unwrap();
    assert_eq!(serde_json::to_string(&arena).unwrap(), "null");

    let arena = load(b"\x04\x08T").unwrap();
    assert_eq!(serde_json::to_string(&arena).unwrap(), "true");

    let arena = load(b"\x04\x08i\x0A").unwrap();
    assert_eq!(serde_json::to_string(&arena).unwrap(), "5");
}

#[test]
fn nil_roundtrips() {
    assert_eq!(roundtrip_via_json(b"\x04\x080"), b"\x04\x080");
}

#[test]
fn string_roundtrips() {
    let bytes: &[u8] = b"\x04\x08I\"\x11Short string\x06:\x06ET";
    assert_eq!(roundtrip_via_json(bytes), bytes);
}

#[test]
fn array_roundtrips() {
    let bytes: &[u8] = b"\x04\x08[\x0ai\x06I\"\x08two\x06:\x06ETf\x063[\x06i\x09{\x06i\x0ai\x0b";
    assert_eq!(roundtrip_via_json(bytes), bytes);
}

#[test]
fn hash_with_default_roundtrips() {
    let bytes: &[u8] = b"\x04\x08}\0I\"\x0cdefault\x06:\x06ET";
    assert_eq!(roundtrip_via_json(bytes), bytes);
}

#[test]
fn object_roundtrips() {
    let bytes: &[u8] = b"\x04\x08o:\x11CustomObject\x06:\x0a@dataI\"\x10object data\x06:\x06ET";
    let arena = load(bytes).unwrap();
    let json = serde_json::to_string(&arena).unwrap();
    assert!(json.contains("\"__type\":\"object\""));
    assert!(json.contains("\"CustomObject\""));

    let arena2: Arena<'static> = serde_json::from_str(&json).unwrap();
    let v = ValueRef::root(&arena2);
    assert_eq!(v.class_name(), Some(&b"CustomObject"[..]));
    assert_eq!(v.get("@data").unwrap().as_str(), Some("object data"));
    assert_eq!(dump(&arena2), bytes);
}

#[test]
fn user_class_array_roundtrips() {
    let bytes: &[u8] = b"\x04\x08C:\x0cMyArray[\x08i\x06i\x07i\x08";
    assert_eq!(roundtrip_via_json(bytes), bytes);
}

#[test]
fn bignum_roundtrips() {
    let bytes: &[u8] = b"\x04\x08l+\n\0\0\0\0\0\0\0\0\x02\0";
    assert_eq!(roundtrip_via_json(bytes), bytes);
}

/// A non-UTF-8-tagged string can't survive a JSON round-trip as plain text
/// (JSON strings must be UTF-8), so its `__value` falls back to a byte
/// array - and `__encoding` is what lets the exact original encoding name
/// (not just "not UTF-8") come back on the other side.
#[test]
fn non_utf8_string_roundtrips() {
    // GBK-encoded "\xba\xba\xd7\xd6\xc4\xda" -> "汉字内".
    let bytes: &[u8] = b"\x04\x08I\"\x0b\xBA\xBA\xD7\xD6\xC4\xDA\x06:\rencoding\"\x08GBK";
    let arena = load(bytes).unwrap();
    let json = serde_json::to_string(&arena).unwrap();
    assert!(json.contains("\"__encoding\":\"GBK\""));
    // Not a JSON string (the bytes don't validate as UTF-8) - a byte array.
    assert!(json.contains("\"__value\":[186,186,215,214,196,218]"));

    let arena2: Arena<'static> = serde_json::from_str(&json).unwrap();
    let v = ValueRef::root(&arena2);
    assert_eq!(v.as_bytes(), Some(&b"\xBA\xBA\xD7\xD6\xC4\xDA"[..]));
    assert_eq!(v.encoding_name(), Some(&b"GBK"[..]));
    assert_eq!(dump(&arena2), bytes);
}

#[test]
fn regexp_with_encoding_roundtrips() {
    let bytes: &[u8] = b"\x04\x08I/\x0caboba.*\x03\x06:\x06EF";
    let arena = load(bytes).unwrap();
    let json = serde_json::to_string(&arena).unwrap();
    assert!(json.contains("\"__encoding\":\"US-ASCII\""));

    let arena2: Arena<'static> = serde_json::from_str(&json).unwrap();
    assert_eq!(ValueRef::root(&arena2).encoding_name(), Some(&b"US-ASCII"[..]));
    assert_eq!(dump(&arena2), bytes);
}

/// An encoding name outside the fixed table still round-trips through the
/// arena's custom-encoding side table.
#[test]
fn custom_encoding_name_roundtrips() {
    let mut arena = Arena::builder();
    let s = arena.push_str_with_encoding_name(b"hello".to_vec(), b"MyWeirdEncoding");
    arena.set_root(s);

    let json = serde_json::to_string(&arena).unwrap();
    assert!(json.contains("\"__encoding\":\"MyWeirdEncoding\""));

    let arena2: Arena<'static> = serde_json::from_str(&json).unwrap();
    let v = ValueRef::root(&arena2);
    assert_eq!(v.as_bytes(), Some(&b"hello"[..]));
    assert_eq!(v.encoding_name(), Some(&b"MyWeirdEncoding"[..]));
}
