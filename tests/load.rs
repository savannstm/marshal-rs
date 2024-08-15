#![allow(clippy::approx_constant)]
use core::str;
use marshal_rs::load::load;
use serde_json::{from_value, json};

#[test]
#[should_panic(expected = "Incompatible Marshal file format or version.")]
fn invalid_marshal_version() {
    load(&[0x04, 0x09, 0x30]);
}

#[test]
fn null() {
    assert_eq!(load(&[0x04, 0x08, 0x30]), json!(null));
}

#[test]
fn boolean() {
    assert_eq!(load(&[0x04, 0x08, 0x54]), json!(true));
    assert_eq!(load(&[0x04, 0x08, 0x46]), json!(false))
}

#[test]
fn fixnum_positive() {
    assert_eq!(load(&[0x04, 0x08, 0x69, 0x00]), json!(0));
    assert_eq!(load(&[0x04, 0x08, 0x69, 0x0A]), json!(5));
    assert_eq!(load(&[0x04, 0x08, 0x69, 0x02, 0x2C, 0x01]), json!(300));
    assert_eq!(
        load(&[0x04, 0x08, 0x69, 0x03, 0x70, 0x11, 0x01]),
        json!(70000)
    );
    assert_eq!(
        load(&[0x04, 0x08, 0x69, 0x04, 0x00, 0x00, 0x00, 0x01]),
        json!(16777216)
    );
}

#[test]
fn fixnum_negative() {
    assert_eq!(load(&[0x04, 0x08, 0x69, 0x00]), json!(-0));
    assert_eq!(load(&[0x04, 0x08, 0x69, 0xF6]), json!(-5));
    assert_eq!(load(&[0x04, 0x08, 0x69, 0xFE, 0xD4, 0xFE]), json!(-300));
    assert_eq!(
        load(&[0x04, 0x08, 0x69, 0xFD, 0x90, 0xEE, 0xFE]),
        json!(-70000)
    );
}

#[test]
fn float() {
    assert_eq!(load(&[0x04, 0x08, 0x66, 0x06, 0x30]), json!(0));
    assert_eq!(load(&[0x04, 0x08, 0x66, 0x07, 0x07, 0x2D, 0x30]), json!(-0));
    assert_eq!(
        load(&[0x04, 0x08, 0x66, 0x0C, 0x33, 0x2E, 0x31, 0x34, 0x31, 0x35, 0x39]),
        json!(3.14159)
    );
    assert_eq!(
        load(&[0x04, 0x08, 0x66, 0x0D, 0x2D, 0x32, 0x2E, 0x37, 0x31, 0x38, 0x32, 0x38]),
        json!(-2.71828)
    );
}

#[test]
fn string() {
    assert_eq!(
        str::from_utf8(
            &from_value::<Vec<_>>(
                load(&[
                    0x04, 0x08, 0x49, 0x22, 0x11, 0x53, 0x68, 0x6F, 0x72, 0x74, 0x20, 0x73, 0x74,
                    0x72, 0x69, 0x6E, 0x67, 0x06, 0x3A, 0x06, 0x45, 0x54
                ])["data"]
                    .take()
            )
            .unwrap()
        )
        .unwrap(),
        json!("Short string")
    );

    assert_eq!(
        str::from_utf8(
            &from_value::<Vec<_>>(
                load(&[
                    0x04, 0x08, 0x49, 0x22, 0x01, 0xdc, 0x4c, 0x6f, 0x6e, 0x67, 0x20, 0x73, 0x74,
                    0x72, 0x69, 0x6e, 0x67, 0x4c, 0x6f, 0x6e, 0x67, 0x20, 0x73, 0x74, 0x72, 0x69,
                    0x6e, 0x67, 0x4c, 0x6f, 0x6e, 0x67, 0x20, 0x73, 0x74, 0x72, 0x69, 0x6e, 0x67,
                    0x4c, 0x6f, 0x6e, 0x67, 0x20, 0x73, 0x74, 0x72, 0x69, 0x6e, 0x67, 0x4c, 0x6f,
                    0x6e, 0x67, 0x20, 0x73, 0x74, 0x72, 0x69, 0x6e, 0x67, 0x4c, 0x6f, 0x6e, 0x67,
                    0x20, 0x73, 0x74, 0x72, 0x69, 0x6e, 0x67, 0x4c, 0x6f, 0x6e, 0x67, 0x20, 0x73,
                    0x74, 0x72, 0x69, 0x6e, 0x67, 0x4c, 0x6f, 0x6e, 0x67, 0x20, 0x73, 0x74, 0x72,
                    0x69, 0x6e, 0x67, 0x4c, 0x6f, 0x6e, 0x67, 0x20, 0x73, 0x74, 0x72, 0x69, 0x6e,
                    0x67, 0x4c, 0x6f, 0x6e, 0x67, 0x20, 0x73, 0x74, 0x72, 0x69, 0x6e, 0x67, 0x4c,
                    0x6f, 0x6e, 0x67, 0x20, 0x73, 0x74, 0x72, 0x69, 0x6e, 0x67, 0x4c, 0x6f, 0x6e,
                    0x67, 0x20, 0x73, 0x74, 0x72, 0x69, 0x6e, 0x67, 0x4c, 0x6f, 0x6e, 0x67, 0x20,
                    0x73, 0x74, 0x72, 0x69, 0x6e, 0x67, 0x4c, 0x6f, 0x6e, 0x67, 0x20, 0x73, 0x74,
                    0x72, 0x69, 0x6e, 0x67, 0x4c, 0x6f, 0x6e, 0x67, 0x20, 0x73, 0x74, 0x72, 0x69,
                    0x6e, 0x67, 0x4c, 0x6f, 0x6e, 0x67, 0x20, 0x73, 0x74, 0x72, 0x69, 0x6e, 0x67,
                    0x4c, 0x6f, 0x6e, 0x67, 0x20, 0x73, 0x74, 0x72, 0x69, 0x6e, 0x67, 0x4c, 0x6f,
                    0x6e, 0x67, 0x20, 0x73, 0x74, 0x72, 0x69, 0x6e, 0x67, 0x4c, 0x6f, 0x6e, 0x67,
                    0x20, 0x73, 0x74, 0x72, 0x69, 0x6e, 0x67, 0x4c, 0x6f, 0x6e, 0x67, 0x20, 0x73,
                    0x74, 0x72, 0x69, 0x6e, 0x67, 0x06, 0x3a, 0x06, 0x45, 0x54,
                ])["data"]
                    .take()
            )
            .unwrap()
        )
        .unwrap(),
        "Long string".repeat(20),
    )
}

#[test]
#[should_panic(expected = "Marshal data is too short.")]
fn invalid_string() {
    // length of string is 4, which is equal to 0x09, but 0x10 length is passed
    load(&[0x04, 0x08, 0x22, 0x10, 0xF0, 0x28, 0x8C, 0x28]);
}

#[test]
fn array() {
    assert_eq!(
        load(&[
            0x04, 0x08, 0x5b, 0x0a, 0x69, 0x06, 0x49, 0x22, 0x08, 0x74, 0x77, 0x6f, 0x06, 0x3a,
            0x06, 0x45, 0x54, 0x66, 0x06, 0x33, 0x5b, 0x06, 0x69, 0x09, 0x7b, 0x06, 0x69, 0x0a,
            0x69, 0x0b
        ]),
        json!([1, {"__type": "bytes", "data": [116, 119, 111]}, 3.0, [4], {"__integer__5": 6}])
    )
}

#[test]
fn hash() {
    assert_eq!(
        load(&[
            0x04, 0x08, 0x7B, 0x08, 0x69, 0x06, 0x49, 0x22, 0x08, 0x6F, 0x6E, 0x65, 0x06, 0x3A,
            0x06, 0x45, 0x54, 0x49, 0x22, 0x08, 0x74, 0x77, 0x6F, 0x06, 0x3B, 0x00, 0x54, 0x69,
            0x07, 0x6F, 0x3A, 0x0B, 0x4F, 0x62, 0x6A, 0x65, 0x63, 0x74, 0x00, 0x30
        ]),
        json!({"__integer__1": {"__type": "bytes", "data": [111, 110, 101]}, "__object__{\"__type\":\"bytes\",\"data\":[116,119,111]}": 2, r#"__object__{"__class":"__symbol__Object","__type":"object"}"#: null})
    );

    assert_eq!(
        load(&[
            0x04, 0x08, 0x7D, 0x00, 0x49, 0x22, 0x0C, 0x64, 0x65, 0x66, 0x61, 0x75, 0x6C, 0x74,
            0x06, 0x3A, 0x06, 0x45, 0x54
        ]),
        json!({"__ruby_default__": {"__type": "bytes", "data": [100,101,102,97,117,108,116]}})
    )
}

#[test]
fn ruby_struct() {
    assert_eq!(
        load(&[
            0x04, 0x08, 0x53, 0x3A, 0x0B, 0x50, 0x65, 0x72, 0x73, 0x6F, 0x6E, 0x07, 0x3A, 0x09,
            0x6E, 0x61, 0x6D, 0x65, 0x49, 0x22, 0x0A, 0x41, 0x6C, 0x69, 0x63, 0x65, 0x06, 0x3A,
            0x06, 0x45, 0x54, 0x3A, 0x08, 0x61, 0x67, 0x65, 0x69, 0x23
        ]),
        json!({"__class": "__symbol__Person", "__members": {"__symbol____symbol__age": 30, "__symbol____symbol__name": {"__type": "bytes", "data":  [65, 108, 105, 99, 101]}}, "__type": "struct"})
    )
}

#[test]
fn object() {
    assert_eq!(
        load(&[
            0x04, 0x08, 0x6f, 0x3a, 0x11, 0x43, 0x75, 0x73, 0x74, 0x6f, 0x6d, 0x4f, 0x62, 0x6a,
            0x65, 0x63, 0x74, 0x06, 0x3a, 0x0a, 0x40, 0x64, 0x61, 0x74, 0x61, 0x49, 0x22, 0x10,
            0x6f, 0x62, 0x6a, 0x65, 0x63, 0x74, 0x20, 0x64, 0x61, 0x74, 0x61, 0x06, 0x3a, 0x06,
            0x45, 0x54
        ]),
        json!({"__class": "__symbol__CustomObject", "__symbol__@data": {"__type": "bytes", "data":  [111, 98, 106, 101, 99, 116, 32, 100, 97, 116, 97]}, "__type": "object"})
    )
}
