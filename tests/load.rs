#![allow(clippy::approx_constant)]
use marshal_rs::{load, load_binary, HashMap, Object, Value, ValueType};

#[test]
#[should_panic]
fn invalid_marshal_version() {
    load(b"\x04\x090", None).unwrap();
}

#[test]
fn null() {
    assert_eq!(load(b"\x04\x080", None).unwrap(), Value::new());
}

#[test]
fn boolean() {
    assert_eq!(load(b"\x04\x08T", None).unwrap(), Value::bool(true));
    assert_eq!(load(b"\x04\x08F", None).unwrap(), Value::bool(false));
}

#[test]
fn fixnum_positive() {
    assert_eq!(load(b"\x04\x08i\0", None).unwrap(), Value::int(0));
    assert_eq!(load(b"\x04\x08i\x0A", None).unwrap(), Value::int(5));
    assert_eq!(
        load(b"\x04\x08i\x02\x2C\x01", None).unwrap(),
        Value::int(300)
    );
    assert_eq!(
        load(b"\x04\x08i\x03\x70\x11\x01", None).unwrap(),
        Value::int(70000)
    );
    assert_eq!(
        load(b"\x04\x08i\x04\0\0\0\x01", None).unwrap(),
        Value::int(16777216)
    );
}

#[test]
fn fixnum_negative() {
    assert_eq!(load(b"\x04\x08i\xF6", None).unwrap(), Value::int(-5));
    assert_eq!(
        load(b"\x04\x08i\xFE\xD4\xFE", None).unwrap(),
        Value::int(-300)
    );
    assert_eq!(
        load(b"\x04\x08i\xFD\x90\xEE\xFE", None).unwrap(),
        Value::int(-70000)
    );
    assert_eq!(
        load(b"\x04\x08i\xFD\0\0\0", None).unwrap(),
        Value::int(-16777216)
    );
}

#[test]
fn bignum_positive() {
    let json = Value::bigint("36893488147419103232");
    assert_eq!(
        load(b"\x04\x08l+\n\0\0\0\0\0\0\0\0\x02\0", None).unwrap(),
        json
    );

    let json = Value::bigint("73786976294838206464");
    assert_eq!(
        load(b"\x04\x08l+\n\0\0\0\0\0\0\0\0\x04\0", None).unwrap(),
        json
    );

    let json = Value::bigint("147573952589676412928");
    assert_eq!(
        load(b"\x04\x08l+\n\0\0\0\0\0\0\0\0\x08\0", None).unwrap(),
        json
    );
}

#[test]
fn bignum_negative() {
    let json = Value::bigint("-36893488147419103232");
    assert_eq!(
        load(b"\x04\x08l-\n\0\0\0\0\0\0\0\0\x02\0", None).unwrap(),
        json
    );

    let json = Value::bigint("-73786976294838206464");
    assert_eq!(
        load(b"\x04\x08l-\n\0\0\0\0\0\0\0\0\x04\0", None).unwrap(),
        json
    );

    let json = Value::bigint("-147573952589676412928");
    assert_eq!(
        load(b"\x04\x08l-\n\0\0\0\0\0\0\0\0\x08\0", None).unwrap(),
        json
    );
}

#[test]
fn float() {
    let float = Value::float("0");
    assert_eq!(load(b"\x04\x08f\x06\x30", None).unwrap(), float);

    let float = Value::float("-0");
    assert_eq!(load(b"\x04\x08f\x07-0", None).unwrap(), float);

    let float = Value::float("3.14159");
    assert_eq!(
        load(b"\x04\x08f\x0C\x33\x2E\x31\x34\x31\x35\x39", None).unwrap(),
        float
    );

    let float = Value::float("-2.71828");
    assert_eq!(
        load(b"\x04\x08f\x0D\x2D\x32\x2E\x37\x31\x38\x32\x38", None).unwrap(),
        float
    );

    let float = Value::float("nan");
    assert_eq!(load(b"\x04\x08f\x08nan", None).unwrap(), float);

    let float = Value::float("inf");
    assert_eq!(load(b"\x04\x08f\x08inf", None).unwrap(), float);

    let float = Value::float("-inf");
    assert_eq!(load(b"\x04\x08f\t-inf", None).unwrap(), float);
}

#[test]
fn string_utf8() {
    assert_eq!(
        load(b"\x04\x08I\"\x11Short string\x06:\x06ET", None).unwrap(),
        Value::string("Short string")
    );

    assert_eq!(
        load(
            b"\x04\x08I\"\x01\xdcLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong string\x06:\x06ET",
            None
        ).unwrap(),
        Value::string("Long string".repeat(20)),
    );
}

#[test]
fn string_nonutf8() {
    assert_eq!(
        load(
            b"\x04\x08I\"\x0b\xBA\xBA\xD7\xD6\xC4\xDA\x06:\rencoding\"\x08GBK",
            None
        )
        .unwrap(),
        Value::string("汉字内")
    );
}

#[test]
fn string_binary() {
    let json = Value::bytes("Short string".as_bytes());
    assert_eq!(
        load_binary(b"\x04\x08I\"\x11Short string\x06:\x06ET", None).unwrap(),
        json
    );

    let json = Value::bytes("Long string".repeat(20).as_bytes());
    assert_eq!(
        load_binary(
            b"\x04\x08I\"\x01\xdcLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong string\x06:\x06ET",
            None
        ).unwrap(),
        json,
    );
}

#[test]
#[should_panic]
fn invalid_string() {
    // length of string is 4, which is equal to 0x09, but 0x10 length is passed
    load(b"\x04\x08\"\x10\xf0(\x8c(", None).unwrap();
}

#[test]
fn links() {
    let first_float = Value::float("0.1");
    let second_float = Value::float("0.2");
    let third_float = Value::float("0.3");

    let json = vec![
        Value::array(vec![
            first_float.clone(),
            first_float.clone(),
            first_float.clone(),
        ]),
        Value::array(vec![
            second_float.clone(),
            second_float.clone(),
            second_float.clone(),
        ]),
        Value::array(vec![
            third_float.clone(),
            third_float.clone(),
            third_float.clone(),
        ]),
    ];

    assert_eq!(
        load(
            b"\x04\x08[\x08[\x08f\x080.1@\x07@\x07[\x08f\x080.2@\x09@\x09[\x08f\x080.3@\x0b@\x0b",
            None
        )
        .unwrap(),
        Value::array(json)
    );
}

#[test]
fn array() {
    let integer_key = Value::int(5);
    let map = HashMap::from_iter([(integer_key, Value::int(6))]);
    let map_json = Value::from(ValueType::HashMap(map));

    let float_value = Value::float("3");

    let inner_array = vec![Value::int(4)];
    let inner_array_value = Value::array(inner_array);
    let array = vec![
        Value::int(1),
        Value::string("two"),
        float_value,
        inner_array_value,
        map_json,
    ];
    let json = Value::array(array);

    assert_eq!(
        load(
            b"\x04\x08[\x0ai\x06I\"\x08two\x06:\x06ETf\x063[\x06i\x09{\x06i\x0ai\x0b",
            None
        )
        .unwrap(),
        json
    );
}

#[test]
fn hash() {
    let mut object = Value::object(Object::new());
    object.set_class("Object".into());

    let map = HashMap::from_iter([
        (Value::int(1), Value::string("one")),
        (Value::string("two"), Value::int(2)),
        (object, Value::null()),
    ]);
    let map = Value::from(ValueType::HashMap(map));

    assert_eq!(
        load(
            b"\x04\x08{\x08i\x06I\"\x08one\x06:\x06ETI\"\x08two\x06;\0Ti\x07o:\x0bObject\x000",
            None
        )
        .unwrap(),
        map
    );

    let default_symbol = Value::symbol("__ruby_default__");
    let map = Value::from(ValueType::HashMap(HashMap::from_iter([(
        default_symbol,
        Value::string("default"),
    )])));

    assert_eq!(
        load(b"\x04\x08}\0I\"\x0cdefault\x06:\x06ET", None).unwrap(),
        map
    );
}

#[test]
fn ruby_struct() {
    let age_key = Value::symbol("age");
    let name_key = Value::symbol("name");
    let map = HashMap::from_iter([
        (name_key, Value::string("Alice")),
        (age_key, Value::int(30)),
    ]);

    let mut json = Value::rstruct(map);
    json.set_class("Person".to_owned());

    assert_eq!(
        load(
            b"\x04\x08S:\x0bPerson\x07:\x09nameI\"\x0aAlice\x06:\x06ET:\x08agei#",
            None
        )
        .unwrap(),
        json
    );
}

#[test]
fn object() {
    let mut json = Value::object(Object::from_iter([(
        "@data".into(),
        Value::string("object data"),
    )]));
    json.set_class("CustomObject".into());

    assert_eq!(
        load(
            b"\x04\x08o:\x11CustomObject\x06:\x0a@dataI\"\x10object data\x06:\x06ET",
            None
        )
        .unwrap(),
        json
    );
}

#[test]
fn custom_marshal() {
    let mut json = Value::hash(HashMap::from_iter([(
        Value::symbol("data"),
        Value::string("Important Data"),
    )]));
    json.set_class("CustomDumpClass".into());
    json.set_user_marshal(true);

    assert_eq!(
        load(b"\x04\x08U:\x14CustomDumpClass{\x06:\tdataI\"\x13Important Data\x06:\x06ET", None ).unwrap(),
        json
    )
}

#[test]
fn extended_object() {
    let mut json = Value::bytes("I am a string".as_bytes());
    json.add_extension("MyModule".into());

    assert_eq!(
        load_binary(
            b"\x04\x08Ie:\rMyModule\"\x12I am a string\x06:\x06ET",
            None
        )
        .unwrap(),
        json
    );
}

#[test]
fn module() {
    let mut json = Value::module();
    json.set_class("MyModule".into());
    json.set_old_module(false);

    assert_eq!(load(b"\x04\x08m\rMyModule", None).unwrap(), json)
}

#[test]
fn regexp_with_encoding() {
    let json = Value::regexp("/aboba.*/ix");
    assert_eq!(
        load(b"\x04\x08I/\x0caboba.*\x03\x06:\x06EF", None).unwrap(),
        json
    )
}

#[test]
fn regexp_without_encoding() {
    let json = Value::regexp("/aboba.*/ix");
    assert_eq!(load(b"\x04\x08/\x0caboba.*\x03", None).unwrap(), json)
}

#[test]
fn custom_dump_and_load() {
    let mut json = Value::string("terces");
    json.set_class("CustomObject".into());
    json.set_user_defined(true);

    assert_eq!(
        load(b"\x04\x08Iu:\x11CustomObject\x0bterces\x06:\x06ET", None)
            .unwrap(),
        json
    );
}

#[test]
fn array_subclass() {
    let mut json = Value::array([Value::int(1), Value::int(2), Value::int(3)]);
    json.set_class("MyArray".into());
    json.set_user_class(true);

    assert_eq!(
        load(b"\x04\x08C:\x0cMyArray[\x08i\x06i\x07i\x08", None).unwrap(),
        json
    );
}

#[test]
fn string_subclass() {
    let mut json = Value::string("hello");
    json.set_class("MyString".into());
    json.set_user_class(true);

    assert_eq!(
        load(b"\x04\x08IC:\rMyString\"\nhello\x06:\x06ET", None).unwrap(),
        json
    );
}

#[test]
fn regexp_subclass() {
    let mut json = Value::regexp("/foo.*bar/");
    json.set_class("MyRegexp".into());
    json.set_user_class(true);

    assert_eq!(
        load(b"\x04\x08IC:\rMyRegexp/\rfoo.*bar\x00\x06:\x06EF", None).unwrap(),
        json
    );
}

#[test]
fn hash_subclass() {
    let a_key = Value::symbol("a");
    let b_key = Value::symbol("b");
    let map =
        HashMap::from_iter([(a_key, Value::int(1)), (b_key, Value::int(2))]);

    let mut json = Value::hash(map);
    json.set_class("MyHash".into());
    json.set_user_class(true);

    assert_eq!(
        load(b"\x04\x08C:\x0bMyHash{\x07:\x06ai\x06:\x06bi\x07", None).unwrap(),
        json
    );
}
