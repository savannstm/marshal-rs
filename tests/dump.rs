#![allow(clippy::approx_constant)]
use marshal_rs::dump;
#[cfg(not(feature = "sonic"))]
use serde_json::json;
#[cfg(feature = "sonic")]
use sonic_rs::json;

#[test]
fn null() {
    assert_eq!(dump(json!(null), None), b"\x04\x080");
}

#[test]
fn boolean() {
    assert_eq!(dump(json!(true), None), b"\x04\x08T");
    assert_eq!(dump(json!(false), None), b"\x04\x08F");
}

#[test]
fn fixnum_positive() {
    assert_eq!(dump(json!(0), None), b"\x04\x08i\0");
    assert_eq!(dump(json!(5), None), b"\x04\x08i\x0A");
    assert_eq!(dump(json!(300), None), b"\x04\x08i\x02\x2C\x01");
    assert_eq!(dump(json!(70000), None), b"\x04\x08i\x03p\x11\x01");
    assert_eq!(dump(json!(16777216), None), b"\x04\x08i\x04\0\0\0\x01");
}

#[test]
fn fixnum_negative() {
    assert_eq!(dump(json!(-5), None), b"\x04\x08i\xF6");
    assert_eq!(dump(json!(-300), None), b"\x04\x08i\xFE\xD4\xFE");
    assert_eq!(dump(json!(-70000), None), b"\x04\x08i\xFD\x90\xEE\xFE");
    assert_eq!(dump(json!(-16777216), None), b"\x04\x08i\xFD\0\0\0");
}

#[test]
fn bignum_positive() {
    assert_eq!(
        dump(
            json!({"__type": "bigint", "value": "36893488147419103232"}),
            None,
        ),
        b"\x04\x08l+\n\0\0\0\0\0\0\0\0\x02\0"
    );

    assert_eq!(
        dump(
            json!({"__type": "bigint", "value": "73786976294838206464"}),
            None,
        ),
        b"\x04\x08l+\n\0\0\0\0\0\0\0\0\x04\0"
    );

    assert_eq!(
        dump(
            json!({"__type": "bigint", "value": "147573952589676412928"}),
            None,
        ),
        b"\x04\x08l+\n\0\0\0\0\0\0\0\0\x08\0"
    );
}

#[test]
fn bignum_negative() {
    assert_eq!(
        dump(
            json!({"__type": "bigint", "value": "-36893488147419103232"}),
            None,
        ),
        b"\x04\x08l-\n\0\0\0\0\0\0\0\0\x02\0",
    );

    assert_eq!(
        dump(
            json!({"__type": "bigint", "value": "-73786976294838206464"}),
            None,
        ),
        b"\x04\x08l-\n\0\0\0\0\0\0\0\0\x04\0"
    );

    assert_eq!(
        dump(
            json!({"__type": "bigint", "value": "-147573952589676412928"}),
            None,
        ),
        b"\x04\x08l-\n\0\0\0\0\0\0\0\0\x08\0"
    );
}

#[test]
fn float() {
    assert_eq!(dump(json!(0), None), b"\x04\x08i\0");
    assert_eq!(dump(json!(-0.0), None), b"\x04\x08f\x07-0");
    assert_eq!(
        dump(json!(3.14159), None),
        b"\x04\x08f\x0C\x33\x2E\x31\x34\x31\x35\x39"
    );
    assert_eq!(
        dump(json!(-2.71828), None),
        b"\x04\x08f\x0D\x2D\x32\x2E\x37\x31\x38\x32\x38"
    );
}

#[test]
fn string_utf8() {
    assert_eq!(
        dump(json!("Short string"), None),
        b"\x04\x08I\"\x11Short string\x06:\x06ET"
    );

    assert_eq!(
        dump(
            json!("Long string".repeat(20)),
            None
        ),
        b"\x04\x08I\"\x01\xdcLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong string\x06:\x06ET",
    );
}

#[test]
fn string_nonutf8() {
    assert_eq!(
        dump(json!("汉字内"), None),
        b"\x04\x08I\"\x0E\xE6\xB1\x89\xE5\xAD\x97\xE5\x86\x85\x06:\x06ET"
    );
}

#[test]
fn string_binary() {
    assert_eq!(
        dump(
            json!({"__type": "bytes", "data": "Short string".as_bytes()}),
            None
        ),
        b"\x04\x08\"\x11Short string"
    );

    assert_eq!(
        dump(
            json!({"__type": "bytes", "data": "Long string".repeat(20).as_bytes()}),
            None
        ),
        b"\x04\x08\"\x01\xdcLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong string",
    );
}

#[test]
#[should_panic]
fn links() {
    assert_eq!(
        dump(
            json!([[0.1, 0.1, 0.1], [0.2, 0.2, 0.2], [0.3, 0.3, 0.3]]),
            None,
        ),
        b"\x04\x08[\x08[\x08f\x080.1@\x07@\x07[\x08f\x080.2@\x09@\x09[\x08f\x080.3@\x0b@\x0b"
    );
}

#[test]
fn array() {
    assert_eq!(
        dump(json!([1, "two", 3.0, [4], {"__integer__5": 6}]), None,),
        b"\x04\x08[\x0ai\x06I\"\x08two\x06:\x06ETf\x063[\x06i\x09{\x06i\x0ai\x0b"
    );
}

#[test]
fn hash() {
    assert_eq!(
        dump(
            json!({"__integer__1": "one", "two": 2, r#"__object__{"__class":"__symbol__Object","__type":"object"}"#: null}),
            None
        ),
        b"\x04\x08{\x08i\x06I\"\x08one\x06:\x06ETI\"\x08two\x06;\0Ti\x07o:\x0bObject\x000"
    );

    assert_eq!(
        dump(json!({"__ruby_default__": "default"}), None),
        b"\x04\x08}\0I\"\x0cdefault\x06:\x06ET"
    );
}

#[test]
fn ruby_struct() {
    assert_eq!(
        dump(
            json!({"__class": "__symbol__Person", "__members": {"__symbol__age": 30, "__symbol__name": "Alice"}, "__type": "struct"}),
            None,
        ).iter().map(|&x| x as u32).sum::<u32>(),
        b"\x04\x08S:\x0bPerson\x07:\x09nameI\"\x0aAlice\x06:\x06ET:\x08agei#".iter().map(|&x| x as u32).sum::<u32>(),
    );
}

#[test]
fn object() {
    assert_eq!(
        dump(
            json!({"__class": "__symbol__CustomObject", "__symbol__@data": "object data", "__type": "object"}),
            None
        ),
        b"\x04\x08o:\x11CustomObject\x06:\x0a@dataI\"\x10object data\x06:\x06ET"
    );
}
