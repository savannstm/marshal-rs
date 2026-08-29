//! Load/dump correctness tests, using byte fixtures captured from real Ruby
//! `Marshal.dump` output (carried over from v2's test suite, which was
//! verified against an actual Ruby build - see the crate README's
//! References section).

use marshal_rs::{Arena, Kind, ValueRef, dump, load};

fn root(bytes: &[u8]) -> marshal_rs::Arena<'_> {
    load(bytes).expect("should load")
}

#[test]
fn invalid_marshal_version() {
    assert!(load(b"\x04\x090").is_err());
}

#[test]
fn null() {
    let arena = root(b"\x04\x080");
    assert_eq!(ValueRef::root(&arena).kind(), Kind::Nil);
    assert_eq!(dump(&arena), b"\x04\x080");
}

#[test]
fn boolean() {
    let arena = root(b"\x04\x08T");
    assert_eq!(ValueRef::root(&arena).as_bool(), Some(true));
    assert_eq!(dump(&arena), b"\x04\x08T");

    let arena = root(b"\x04\x08F");
    assert_eq!(ValueRef::root(&arena).as_bool(), Some(false));
    assert_eq!(dump(&arena), b"\x04\x08F");
}

#[test]
fn fixnum_positive() {
    for (bytes, value) in [
        (&b"\x04\x08i\0"[..], 0),
        (&b"\x04\x08i\x0A"[..], 5),
        (&b"\x04\x08i\x02\x2C\x01"[..], 300),
        (&b"\x04\x08i\x03\x70\x11\x01"[..], 70000),
        (&b"\x04\x08i\x04\0\0\0\x01"[..], 16777216),
    ] {
        let arena = root(bytes);
        assert_eq!(ValueRef::root(&arena).as_i64(), Some(value));
        assert_eq!(dump(&arena), bytes);
    }
}

#[test]
fn fixnum_negative() {
    for (bytes, value) in [
        (&b"\x04\x08i\xF6"[..], -5),
        (&b"\x04\x08i\xFE\xD4\xFE"[..], -300),
        (&b"\x04\x08i\xFD\x90\xEE\xFE"[..], -70000),
        (&b"\x04\x08i\xFD\0\0\0"[..], -16777216),
    ] {
        let arena = root(bytes);
        assert_eq!(ValueRef::root(&arena).as_i64(), Some(value));
        assert_eq!(dump(&arena), bytes);
    }
}

#[test]
fn bignum_positive() {
    for (bytes, decimal) in [
        (&b"\x04\x08l+\n\0\0\0\0\0\0\0\0\x02\0"[..], "36893488147419103232"),
        (&b"\x04\x08l+\n\0\0\0\0\0\0\0\0\x04\0"[..], "73786976294838206464"),
        (&b"\x04\x08l+\n\0\0\0\0\0\0\0\0\x08\0"[..], "147573952589676412928"),
    ] {
        let arena = root(bytes);
        assert_eq!(ValueRef::root(&arena).as_bigint_decimal().as_deref(), Some(decimal));
        assert_eq!(dump(&arena), bytes);
    }
}

#[test]
fn bignum_negative() {
    for (bytes, decimal) in [
        (&b"\x04\x08l-\n\0\0\0\0\0\0\0\0\x02\0"[..], "-36893488147419103232"),
        (&b"\x04\x08l-\n\0\0\0\0\0\0\0\0\x04\0"[..], "-73786976294838206464"),
        (&b"\x04\x08l-\n\0\0\0\0\0\0\0\0\x08\0"[..], "-147573952589676412928"),
    ] {
        let arena = root(bytes);
        assert_eq!(ValueRef::root(&arena).as_bigint_decimal().as_deref(), Some(decimal));
        assert_eq!(dump(&arena), bytes);
    }
}

#[test]
fn float() {
    for (bytes, text) in [
        (&b"\x04\x08f\x06\x30"[..], "0"),
        (&b"\x04\x08f\x07-0"[..], "-0"),
        (&b"\x04\x08f\x0C\x33\x2E\x31\x34\x31\x35\x39"[..], "3.14159"),
        (&b"\x04\x08f\x0D\x2D\x32\x2E\x37\x31\x38\x32\x38"[..], "-2.71828"),
        (&b"\x04\x08f\x08nan"[..], "nan"),
        (&b"\x04\x08f\x08inf"[..], "inf"),
        (&b"\x04\x08f\t-inf"[..], "-inf"),
    ] {
        let arena = root(bytes);
        assert_eq!(ValueRef::root(&arena).as_float_bytes(), Some(text.as_bytes()));
        assert_eq!(dump(&arena), bytes);
    }
}

#[test]
fn string_utf8() {
    let bytes: &[u8] = b"\x04\x08I\"\x11Short string\x06:\x06ET";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert_eq!(v.kind(), Kind::Str);
    assert_eq!(v.as_str(), Some("Short string"));
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn string_nonutf8() {
    // GBK-encoded "\xba\xba\xd7\xd6\xc4\xda" -> "汉字内" - this crate never
    // transcodes, so the raw GBK bytes come through completely untouched;
    // only the *tag* ("GBK") is understood/preserved.
    let bytes: &[u8] = b"\x04\x08I\"\x0b\xBA\xBA\xD7\xD6\xC4\xDA\x06:\rencoding\"\x08GBK";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert_eq!(v.kind(), Kind::Str);
    assert_eq!(v.as_bytes(), Some(&b"\xBA\xBA\xD7\xD6\xC4\xDA"[..]));
    // Raw GBK bytes don't validate as UTF-8, so `as_str` correctly declines.
    assert_eq!(v.as_str(), None);
    assert_eq!(v.encoding_name(), Some(&b"GBK"[..]));
    // Byte-exact: the declared encoding round-trips through the fixed
    // name<->id table, unlike v2 (and early v3), which always re-dumped
    // text as UTF-8 regardless of what was originally declared.
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn string_binary_default_mode_stays_bytes() {
    // No `E`/`encoding` ivar at all -> Auto mode leaves it as raw bytes.
    let bytes: &[u8] = b"\x04\x08\"\x11Short string";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert_eq!(v.kind(), Kind::Bytes);
    assert_eq!(v.as_bytes(), Some(&b"Short string"[..]));
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn invalid_string_length_errors_cleanly() {
    // Declared length 0x10, only 4 bytes actually present.
    assert!(load(b"\x04\x08\"\x10\xf0(\x8c(").is_err());
}

#[test]
fn links_object_link_reuses_same_node() {
    let bytes: &[u8] = b"\x04\x08[\x08[\x08f\x080.1@\x07@\x07[\x08f\x080.2@\x09@\x09[\x08f\x080.3@\x0b@\x0b";
    let arena = root(bytes);
    let outer = ValueRef::root(&arena);
    assert_eq!(outer.len(), 3);
    for inner in outer.array() {
        assert_eq!(inner.len(), 3);
        let ids: Vec<_> = inner.array().map(|v| v.id()).collect();
        assert_eq!(ids[0], ids[1]);
        assert_eq!(ids[1], ids[2]);
    }
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn array_mixed_contents() {
    let bytes: &[u8] = b"\x04\x08[\x0ai\x06I\"\x08two\x06:\x06ETf\x063[\x06i\x09{\x06i\x0ai\x0b";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert_eq!(v.len(), 5);
    assert_eq!(v.at(0).unwrap().as_i64(), Some(1));
    assert_eq!(v.at(1).unwrap().as_str(), Some("two"));
    assert_eq!(v.at(2).unwrap().as_float_bytes(), Some(&b"3"[..]));
    assert_eq!(v.at(3).unwrap().len(), 1);
    assert_eq!(v.at(3).unwrap().at(0).unwrap().as_i64(), Some(4));
    let h = v.at(4).unwrap();
    assert_eq!(h.kind(), Kind::Hash);
    assert_eq!(h.entries().count(), 1);
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn hash_basic() {
    let bytes: &[u8] = b"\x04\x08{\x08i\x06I\"\x08one\x06:\x06ETI\"\x08two\x06;\0Ti\x07o:\x0bObject\x000";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert_eq!(v.kind(), Kind::Hash);
    assert_eq!(v.entries().count(), 3);
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn hash_with_default() {
    let bytes: &[u8] = b"\x04\x08}\0I\"\x0cdefault\x06:\x06ET";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert_eq!(v.kind(), Kind::Hash);
    assert_eq!(v.entries().count(), 0);
    assert_eq!(v.default_value().unwrap().as_str(), Some("default"));
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn ruby_struct() {
    let bytes: &[u8] = b"\x04\x08S:\x0bPerson\x07:\x09nameI\"\x0aAlice\x06:\x06ET:\x08agei#";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert_eq!(v.kind(), Kind::Struct);
    assert_eq!(v.class_name(), Some(&b"Person"[..]));
    let members: Vec<_> = v.members().collect();
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].0, b"name");
    assert_eq!(members[0].1.as_str(), Some("Alice"));
    assert_eq!(members[1].0, b"age");
    assert_eq!(members[1].1.as_i64(), Some(30));
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn object() {
    let bytes: &[u8] = b"\x04\x08o:\x11CustomObject\x06:\x0a@dataI\"\x10object data\x06:\x06ET";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert_eq!(v.kind(), Kind::Object);
    assert_eq!(v.class_name(), Some(&b"CustomObject"[..]));
    assert_eq!(v.get("@data").unwrap().as_str(), Some("object data"));
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn custom_marshal() {
    let bytes: &[u8] = b"\x04\x08U:\x14CustomDumpClass{\x06:\tdataI\"\x13Important Data\x06:\x06ET";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert_eq!(v.kind(), Kind::Hash);
    assert!(v.is_user_marshal());
    assert_eq!(v.class_name(), Some(&b"CustomDumpClass"[..]));
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn extended_object() {
    let bytes: &[u8] = b"\x04\x08Ie:\rMyModule\"\x12I am a string\x06:\x06ET";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert_eq!(v.kind(), Kind::Str);
    assert_eq!(v.extensions().collect::<Vec<_>>(), vec![&b"MyModule"[..]]);
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn module() {
    let bytes: &[u8] = b"\x04\x08m\rMyModule";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert_eq!(v.kind(), Kind::Module);
    assert!(!v.is_old_module());
    assert_eq!(v.as_path(), Some(&b"MyModule"[..]));
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn regexp_with_encoding() {
    let bytes: &[u8] = b"\x04\x08I/\x0caboba.*\x03\x06:\x06EF";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    let (source, options) = v.as_regexp().unwrap();
    assert_eq!(source, b"aboba.*");
    assert_eq!(options, 0x03);
    // `:E => false` means US-ASCII - a regexp's declared encoding round-
    // trips through the same ivar mechanism a string's does.
    assert_eq!(v.encoding_name(), Some(&b"US-ASCII"[..]));
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn regexp_without_encoding() {
    let bytes: &[u8] = b"\x04\x08/\x0caboba.*\x03";
    let arena = root(bytes);
    let (source, options) = ValueRef::root(&arena).as_regexp().unwrap();
    assert_eq!(source, b"aboba.*");
    assert_eq!(options, 0x03);
    assert_eq!(dump(&arena), bytes);
}

/// An encoding name outside the fixed table (see `src/encoding.rs`) still
/// round-trips byte-exact through the arena's custom-encoding side table -
/// this is the escape hatch that keeps a not-yet-tabled or future Ruby
/// encoding from losing fidelity.
#[test]
fn custom_encoding_name_round_trips() {
    let mut arena = Arena::builder();
    let s = arena.push_str_with_encoding_name(b"hello".to_vec(), b"MyWeirdEncoding");
    arena.set_root(s);
    let bytes = dump(&arena);

    let arena2 = root(&bytes);
    let v = ValueRef::root(&arena2);
    assert_eq!(v.as_bytes(), Some(&b"hello"[..]));
    assert_eq!(v.encoding_name(), Some(&b"MyWeirdEncoding"[..]));
    assert_eq!(dump(&arena2), bytes);
}

#[test]
fn regexp_custom_encoding_name_round_trips() {
    let mut arena = Arena::builder();
    let r = arena.push_regexp_with_encoding_name(b"a.*b".to_vec(), 0, b"MyWeirdEncoding");
    arena.set_root(r);
    let bytes = dump(&arena);

    let arena2 = root(&bytes);
    let v = ValueRef::root(&arena2);
    assert_eq!(v.as_regexp(), Some((&b"a.*b"[..], 0)));
    assert_eq!(v.encoding_name(), Some(&b"MyWeirdEncoding"[..]));
    assert_eq!(dump(&arena2), bytes);
}

/// An untagged `Bytes` value implicitly means `ASCII-8BIT`, matching Ruby's
/// own default - even though no ivar was ever written for it.
#[test]
fn untagged_bytes_implies_ascii_8bit() {
    let bytes: &[u8] = b"\x04\x08\"\x11Short string";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert_eq!(v.kind(), Kind::Bytes);
    assert_eq!(v.encoding_id(), Some(marshal_rs::ENCODING_ASCII_8BIT));
    assert_eq!(v.encoding_name(), Some(&b"ASCII-8BIT"[..]));
}

#[test]
fn custom_dump_and_load() {
    let bytes: &[u8] = b"\x04\x08Iu:\x11CustomObject\x0bterces\x06:\x06ET";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert!(v.is_user_defined());
    assert_eq!(v.class_name(), Some(&b"CustomObject"[..]));
    assert_eq!(v.as_str(), Some("terces"));
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn array_subclass() {
    let bytes: &[u8] = b"\x04\x08C:\x0cMyArray[\x08i\x06i\x07i\x08";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert_eq!(v.kind(), Kind::Array);
    assert!(v.is_user_class());
    assert_eq!(v.class_name(), Some(&b"MyArray"[..]));
    assert_eq!(v.len(), 3);
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn string_subclass() {
    let bytes: &[u8] = b"\x04\x08IC:\rMyString\"\nhello\x06:\x06ET";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert_eq!(v.kind(), Kind::Str);
    assert!(v.is_user_class());
    assert_eq!(v.class_name(), Some(&b"MyString"[..]));
    assert_eq!(v.as_str(), Some("hello"));
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn regexp_subclass() {
    let bytes: &[u8] = b"\x04\x08IC:\rMyRegexp/\rfoo.*bar\x00\x06:\x06EF";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert!(v.is_user_class());
    assert_eq!(v.class_name(), Some(&b"MyRegexp"[..]));
    let (source, _) = v.as_regexp().unwrap();
    assert_eq!(source, b"foo.*bar");
    assert_eq!(v.encoding_name(), Some(&b"US-ASCII"[..]));
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn hash_subclass() {
    let bytes: &[u8] = b"\x04\x08C:\x0bMyHash{\x07:\x06ai\x06:\x06bi\x07";
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert_eq!(v.kind(), Kind::Hash);
    assert!(v.is_user_class());
    assert_eq!(v.class_name(), Some(&b"MyHash"[..]));
    assert_eq!(v.entries().count(), 2);
    assert_eq!(dump(&arena), bytes);
}

/// Regression coverage carried over from PR #3:
/// `SymbolLink` (`;`) keys must resolve to the same interned symbol as the
/// value they reference, including across repeated `UserMarshal` containers.
#[test]
fn symbol_link_keys_preserved_in_repeated_hashmap() {
    const BYTES: &[u8] = &[
        0x04, 0x08, 0x5b, 0x07, 0x55, 0x3a, 0x0f, b'O', b'p', b'e', b'n', b'S', b't', b'r', b'u', b'c', b't', 0x7b,
        0x07, 0x3a, 0x06, b'a', 0x69, 0x06, 0x3a, 0x06, b'b', 0x66, 0x06, b'2', 0x55, 0x3b, 0x00, 0x7b, 0x07, 0x3b,
        0x06, 0x69, 0x0f, 0x3b, 0x07, 0x66, 0x08, b'2', b'e', b'1',
    ];
    let arena = root(BYTES);
    let v = ValueRef::root(&arena);
    assert_eq!(v.len(), 2);
    for elem in v.array() {
        assert!(elem.is_user_marshal());
        let mut names: Vec<_> = elem.entries().map(|(k, _)| k.as_str().unwrap().to_owned()).collect();
        names.sort();
        assert_eq!(names, ["a", "b"]);
    }
    assert_eq!(dump(&arena), BYTES);
}

#[test]
fn plain_hash_with_symbol_link_values() {
    let bytes: &[u8] = &[
        0x04, 0x08, b'{', 0x07, 0x3a, 0x06, b'a', 0x3a, 0x06, b'b', 0x3a, 0x06, b'c', 0x3b, 0x07,
    ];
    let arena = root(bytes);
    let v = ValueRef::root(&arena);
    assert_eq!(v.entries().count(), 2);
    assert_eq!(dump(&arena), bytes);
}

#[test]
fn deeply_nested_array_does_not_overflow_stack() {
    let depth = 50_000;
    let mut bytes: Vec<u8> = vec![4, 8];
    for _ in 0..depth {
        bytes.push(b'[');
        bytes.push(1 + 5); // one-element array
    }
    bytes.push(b'0');
    let arena = root(&bytes);
    let mut v = ValueRef::root(&arena);
    for _ in 0..depth {
        assert_eq!(v.len(), 1);
        v = v.at(0).unwrap();
    }
    assert!(v.is_nil());
}
