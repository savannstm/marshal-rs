//! The serde envelope, from the JSON side: text a person or another tool wrote, and values built by hand.
//! Everything that starts from Marshal bytes Ruby wrote is in `ruby_fixtures.rs`.

use marshal_rs::{Arena, ValueRef, dump, load};

fn from_json(json: &str) -> Result<Arena<'static>, serde_json::Error> {
    serde_json::from_str(json)
}

#[test]
fn trivial_values_are_bare_json() {
    let mut arena = Arena::builder();
    let array = arena.push_array(&[]).unwrap();
    let nil = arena.push_nil();
    let yes = arena.push_bool(true);
    let five = arena.push_fixnum(5);
    for (id, json) in [(nil, "null"), (yes, "true"), (five, "5")] {
        arena.set_root(id);
        assert_eq!(serde_json::to_string(&arena).unwrap(), json);
    }
    arena.set_root(array);
    assert_eq!(
        serde_json::to_string(&arena).unwrap(),
        r#"{"__type":"array","__value":[]}"#
    );
}

/// An encoding name outside the fixed table still round-trips through the arena's custom-encoding side table.
#[test]
fn custom_encoding_name_roundtrips() {
    let mut arena = Arena::builder();
    let s = arena.push_str_with_encoding_name(b"hello".to_vec(), b"MyWeirdEncoding");
    arena.set_root(s);

    let json = serde_json::to_string(&arena).unwrap();
    assert!(json.contains("\"__encoding\":\"MyWeirdEncoding\""));

    let arena2 = from_json(&json).unwrap();
    let v = ValueRef::root(&arena2);
    assert_eq!(v.as_bytes(), Some(&b"hello"[..]));
    assert_eq!(v.encoding_name(), Some(&b"MyWeirdEncoding"[..]));
}

#[test]
fn binary_payloads_are_base64_in_json() {
    // Lengths 0..=4 cover every padding case.
    for len in 0..=4u8 {
        let payload: Vec<u8> = (0..len).map(|i| 0xf0 + i).collect();
        let mut arena = Arena::builder();
        let id = arena.push_bytes(payload.clone());
        arena.set_root(id);
        let json = serde_json::to_string(&arena).unwrap();
        assert!(json.contains("\"__value\":\""), "{json}");
        let back = from_json(&json).unwrap();
        assert_eq!(ValueRef::root(&back).as_bytes(), Some(&payload[..]));
    }
}

#[test]
fn legacy_byte_arrays_and_bad_base64_are_handled() {
    let old = from_json(r#"{"__type":"bytes","__value":[1,2,3]}"#).unwrap();
    assert_eq!(ValueRef::root(&old).as_bytes(), Some(&[1u8, 2, 3][..]));
    assert!(from_json(r#"{"__type":"bytes","__value":"AQI"}"#).is_err());
    assert!(from_json(r#"{"__type":"bytes","__value":"A===" }"#).is_err());
}

#[test]
fn envelope_keys_may_come_in_any_order() {
    // As a tool that sorts object keys writes it: `__type` is last, in the outer value and the inner ones.
    let sorted = r#"{"__class":"Pair","__members":[["@a",{"__value":"x","__type":"symbol"}],["@b",{"__value":"AQI=","__type":"bytes"}]],"__type":"object"}"#;
    let arena = from_json(sorted).unwrap();
    let root = ValueRef::root(&arena);
    assert_eq!(root.class_name(), Some(&b"Pair"[..]));
    assert_eq!(root.get("a").and_then(|v| v.as_symbol_bytes()), Some(&b"x"[..]));
    assert_eq!(root.get("b").and_then(|v| v.as_bytes()), Some(&[1u8, 2][..]));

    let in_order = r#"{"__type":"object","__class":"Pair","__members":[["@a",{"__type":"symbol","__value":"x"}],["@b",{"__type":"bytes","__value":"AQI="}]]}"#;
    let reference = from_json(in_order).unwrap();
    assert_eq!(dump(&arena), dump(&reference));
}

#[test]
fn envelope_without_a_type_is_an_error() {
    assert!(from_json(r#"{"__value":[1]}"#).is_err());
    assert!(from_json("{}").is_err());
}

#[test]
fn json_that_would_dump_to_an_invalid_stream_is_rejected() {
    for json in [
        r#"{"__type":"object","__class":"","__members":[]}"#,
        r#"{"__type":"class","__value":""}"#,
        r#"{"__type":"object","__class":"Foo","__members":[],"__flags":["user_class"]}"#,
        r#"{"__type":"class","__value":"Foo","__flags":["user_class"],"__class":"Bar"}"#,
        r#"{"__type":"array","__value":[],"__flags":["data"]}"#,
        r#"{"__type":"bytes","__value":"AQI=","__extensions":["M"],"__ivars":[["",0]]}"#,
        r#"{"__type":"symbol","__value":"a","__extensions":["M"]}"#,
        r#"{"__type":"str","__value":"x","__encoding":"UTF-8","__flags":["user_class","user_defined"],"__class":"A"}"#,
    ] {
        assert!(from_json(json).is_err(), "{json}");
    }
}

#[test]
fn base64_must_be_padded_and_clean() {
    let decode = |text: &str| {
        let json = format!(r#"{{"__type":"bytes","__value":"{text}"}}"#);
        from_json(&json)
            .ok()
            .and_then(|arena| ValueRef::root(&arena).as_bytes().map(<[u8]>::to_vec))
    };
    assert_eq!(decode(""), Some(vec![]));
    assert_eq!(decode("AQID"), Some(vec![1, 2, 3]));
    assert_eq!(decode("AQI="), Some(vec![1, 2]));
    assert_eq!(decode("AQ=="), Some(vec![1]));
    assert_eq!(decode("AQI"), None);
    assert_eq!(decode("AQ I="), None);
    assert_eq!(decode("AQ=A"), None);
    assert_eq!(decode("A==="), None);
    assert_eq!(decode("=QID"), None);
    assert_eq!(decode("AQID\n"), None);
}

/// Names and text that are not UTF-8 are written as `{"__base64": ...}` / base64 text and read back exactly;
/// nothing is validated or replaced.
#[test]
fn names_that_are_not_utf8_come_back_exactly() {
    let mut arena = Arena::builder();
    let name = vec![0xff, 0xfe, 0x83, 0x41];
    let symbol = arena.push_symbol(name.clone());
    let float = arena.push_float(b"1.5\0\xff\xfe".to_vec());
    let object = arena
        .push_object(name.clone(), &[(name.clone(), symbol), (b"@plain".to_vec(), float)])
        .unwrap();
    arena.set_root(object);

    let json = serde_json::to_string(&arena).unwrap();
    assert!(json.contains(r#""__class":{"__base64":"#), "{json}");
    assert!(json.contains(r#""__base64":true"#), "{json}");
    assert!(json.contains(r#"["@plain","#), "{json}");
    let back = from_json(&json).unwrap();
    assert_eq!(dump(&back), dump(&arena));

    // A name object holds exactly one key, and valid base64.
    assert!(from_json(r#"{"__type":"object","__class":{"__base64":"AQI=","x":1},"__members":[]}"#).is_err());
    assert!(from_json(r#"{"__type":"object","__class":{"__base64":"AQI"},"__members":[]}"#).is_err());
}

/// A class name Ruby wrote without an encoding stays untagged through JSON; a name built with one keeps it.
#[test]
fn class_name_tags_survive_json() {
    // `Café`, no encoding ivar.
    let plain = b"\x04\x08c\x0a\x43\x61\x66\xc3\xa9";
    let loaded = load(plain).unwrap();
    let back = from_json(&serde_json::to_string(&loaded).unwrap()).unwrap();
    assert_eq!(dump(&back), plain);

    let mut built = Arena::builder();
    let class = built.push_class("Café".as_bytes().to_vec()).unwrap();
    assert!(built.set_root(class));
    let back = from_json(&serde_json::to_string(&built).unwrap()).unwrap();
    assert_eq!(dump(&back), dump(&built));
}
