#![allow(clippy::approx_constant)]
use cfg_if::cfg_if;
use marshal_rs::load::{load, StringMode};
use num_bigint::{BigInt, BigUint};
cfg_if! {
    if #[cfg(feature = "sonic")] {
        use sonic_rs::json;
    } else {
        use serde_json::json;
    }
}
use std::str::FromStr;

#[test]
#[should_panic(expected = "Incompatible Marshal file format or version.")]
fn invalid_marshal_version() {
    load(b"\x04\x090", None, None);
}

#[test]
fn null() {
    assert_eq!(load(b"\x04\x080", None, None), json!(null));
}

#[test]
fn boolean() {
    assert_eq!(load(b"\x04\x08T", None, None), json!(true));
    assert_eq!(load(b"\x04\x08F", None, None), json!(false))
}

#[test]
fn fixnum_positive() {
    assert_eq!(load(b"\x04\x08i\x00", None, None), json!(0));
    assert_eq!(load(b"\x04\x08i\x0A", None, None), json!(5));
    assert_eq!(load(b"\x04\x08i\x02\x2C\x01", None, None), json!(300));
    assert_eq!(load(b"\x04\x08i\x03\x70\x11\x01", None, None), json!(70000));
    assert_eq!(
        load(b"\x04\x08i\x04\x00\x00\x00\x01", None, None),
        json!(16777216)
    );
}

#[test]
fn fixnum_negative() {
    assert_eq!(load(b"\x04\x08i\xF6", None, None), json!(-5));
    assert_eq!(load(b"\x04\x08i\xFE\xD4\xFE", None, None), json!(-300));
    assert_eq!(
        load(b"\x04\x08i\xFD\x90\xEE\xFE", None, None),
        json!(-70000)
    );
}

#[test]
fn bignum_positive() {
    assert_eq!(
        load(
            b"\x04\x08l+\n\x00\x00\x00\x00\x00\x00\x00\x00\x02\x00",
            None,
            None
        ),
        json!({"__type": "bigint", "value": BigUint::from_str("36893488147419103232").unwrap().to_string()})
    );

    assert_eq!(
        load(
            b"\x04\x08l+\n\x00\x00\x00\x00\x00\x00\x00\x00\x04\x00",
            None,
            None
        ),
        json!({"__type": "bigint", "value": BigUint::from_str("73786976294838206464").unwrap().to_string()})
    );

    assert_eq!(
        load(
            b"\x04\x08l+\n\x00\x00\x00\x00\x00\x00\x00\x00\x08\x00",
            None,
            None
        ),
        json!({"__type": "bigint", "value": BigUint::from_str("147573952589676412928").unwrap().to_string()})
    );
}

#[test]
fn bignum_negative() {
    assert_eq!(
        load(
            b"\x04\x08l-\n\x00\x00\x00\x00\x00\x00\x00\x00\x02\x00",
            None,
            None
        ),
        json!({"__type": "bigint", "value": BigInt::from_str("-36893488147419103232").unwrap().to_string()})
    )
}

#[test]
fn float() {
    assert_eq!(load(b"\x04\x08f\x06\x30", None, None), json!(0.0));
    assert_eq!(load(b"\x04\x08f\x07-0", None, None), json!(-0.0));
    assert_eq!(
        load(b"\x04\x08f\x0C\x33\x2E\x31\x34\x31\x35\x39", None, None),
        json!(3.14159)
    );
    assert_eq!(
        load(b"\x04\x08f\x0D\x2D\x32\x2E\x37\x31\x38\x32\x38", None, None),
        json!(-2.71828)
    );
}

#[test]
fn string_utf8() {
    assert_eq!(
        load(b"\x04\x08I\"\x11Short string\x06:\x06ET", None, None),
        json!("Short string")
    );

    assert_eq!(
        load(
            b"\x04\x08I\"\x01\xdcLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong string\x06:\x06ET",
            None, None
        ),
        "Long string".repeat(20),
    )
}

#[test]
fn string_nonutf8() {
    assert_eq!(
        load(
            b"\x04\x08I\"\x0b\xBA\xBA\xD7\xD6\xC4\xDA\x06:\rencoding\"\x08GBK",
            None,
            None
        ),
        json!("汉字内")
    )
}

#[test]
fn string_binary() {
    assert_eq!(
        load(
            b"\x04\x08I\"\x11Short string\x06:\x06ET",
            Some(StringMode::Binary),
            None
        ),
        json!({"__type": "bytes", "data": "Short string".as_bytes()})
    );

    assert_eq!(
        load(
            b"\x04\x08I\"\x01\xdcLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong string\x06:\x06ET",
            Some(StringMode::Binary),
            None
        ),
        json!({"__type": "bytes", "data": "Long string".repeat(20).as_bytes()}),
    )
}

#[test]
#[should_panic(expected = "Marshal data is too short.")]
fn invalid_string() {
    // length of string is 4, which is equal to 0x09, but 0x10 length is passed
    load(b"\x04\x08\"\x10\xf0(\x8c(", None, None);
}

#[test]
fn links() {
    assert_eq!(
        load(
            b"\x04\x08[\x08[\x08f\x080.1@\x07@\x07[\x08f\x080.2@\x09@\x09[\x08f\x080.3@\x0b@\x0b",
            None,
            None
        ),
        json!([[0.1, 0.1, 0.1], [0.2, 0.2, 0.2], [0.3, 0.3, 0.3]])
    )
}

#[test]
fn array() {
    assert_eq!(
        load(
            b"\x04\x08[\x0ai\x06I\"\x08two\x06:\x06ETf\x063[\x06i\x09{\x06i\x0ai\x0b",
            None,
            None
        ),
        json!([1, "two", 3.0, [4], {"__integer__5": 6}])
    )
}

#[test]
fn hash() {
    assert_eq!(
        load(
            b"\x04\x08{\x08i\x06I\"\x08one\x06:\x06ETI\"\x08two\x06;\x00Ti\x07o:\x0bObject\x000",
            None,
            None
        ),
        json!({"__integer__1": "one", "two": 2, r#"__object__{"__class":"__symbol__Object","__type":"object"}"#: null})
    );

    assert_eq!(
        load(b"\x04\x08}\x00I\"\x0cdefault\x06:\x06ET", None, None),
        json!({"__ruby_default__": "default"})
    )
}

#[test]
fn ruby_struct() {
    assert_eq!(
        load(
            b"\x04\x08S:\x0bPerson\x07:\x09nameI\"\x0aAlice\x06:\x06ET:\x08agei#",
            None,
            None
        ),
        json!({"__class": "__symbol__Person", "__members": {"__symbol__age": 30, "__symbol__name": "Alice"}, "__type": "struct"})
    )
}

#[test]
fn object() {
    assert_eq!(
        load(
            b"\x04\x08o:\x11CustomObject\x06:\x0a@dataI\"\x10object data\x06:\x06ET",
            None,
            None
        ),
        json!({"__class": "__symbol__CustomObject", "__symbol__@data": "object data", "__type": "object"})
    )
}
