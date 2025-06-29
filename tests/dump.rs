#![allow(clippy::approx_constant)]
use marshal_rs::{dump, load, HashMap, Object, Value};

#[test]
fn null() {
    assert_eq!(dump(Value::null(), None), b"\x04\x080");
}

#[test]
fn boolean() {
    assert_eq!(dump(Value::bool(true), None), b"\x04\x08T");
    assert_eq!(dump(Value::bool(false), None), b"\x04\x08F");
}

#[test]
fn fixnum_positive() {
    assert_eq!(dump(Value::int(0), None), b"\x04\x08i\0");
    assert_eq!(dump(Value::int(5), None), b"\x04\x08i\x0A");
    assert_eq!(dump(Value::int(300), None), b"\x04\x08i\x02\x2C\x01");
    assert_eq!(dump(Value::int(70000), None), b"\x04\x08i\x03p\x11\x01");
    assert_eq!(dump(Value::int(16777216), None), b"\x04\x08i\x04\0\0\0\x01");
}

#[test]
fn fixnum_negative() {
    assert_eq!(dump(Value::int(-5), None), b"\x04\x08i\xF6");
    assert_eq!(dump(Value::int(-300), None), b"\x04\x08i\xFE\xD4\xFE");
    assert_eq!(dump(Value::int(-70000), None), b"\x04\x08i\xFD\x90\xEE\xFE");
    assert_eq!(dump(Value::int(-16777216), None), b"\x04\x08i\xFD\0\0\0");
}

#[test]
fn bignum_positive() {
    let json = Value::bigint("36893488147419103232");
    assert_eq!(dump(json, None,), b"\x04\x08l+\n\0\0\0\0\0\0\0\0\x02");

    let json = Value::bigint("73786976294838206464");
    assert_eq!(dump(json, None,), b"\x04\x08l+\n\0\0\0\0\0\0\0\0\x04");

    let json = Value::bigint("147573952589676412928");
    assert_eq!(dump(json, None,), b"\x04\x08l+\n\0\0\0\0\0\0\0\0\x08");
}

#[test]
fn bignum_negative() {
    let json = Value::bigint("-36893488147419103232");
    assert_eq!(dump(json, None), b"\x04\x08l-\n\0\0\0\0\0\0\0\0\x02",);

    let json = Value::bigint("-73786976294838206464");
    assert_eq!(dump(json, None), b"\x04\x08l-\n\0\0\0\0\0\0\0\0\x04");

    let json = Value::bigint("-147573952589676412928");
    assert_eq!(dump(json, None), b"\x04\x08l-\n\0\0\0\0\0\0\0\0\x08");
}

#[test]
fn float() {
    assert_eq!(dump(Value::float("0"), None), b"\x04\x08f\x060");
    assert_eq!(dump(Value::float("-0"), None), b"\x04\x08f\x07-0");
    assert_eq!(dump(Value::float("3.14159"), None), b"\x04\x08f\x0C3.14159");
    assert_eq!(dump(Value::float("-2.71828"), None), b"\x04\x08f\r-2.71828");
    assert_eq!(dump(Value::float("nan"), None), b"\x04\x08f\x08nan");
    assert_eq!(dump(Value::float("inf"), None), b"\x04\x08f\x08inf");
    assert_eq!(dump(Value::float("-inf"), None), b"\x04\x08f\t-inf");
}

#[test]
fn string_utf8() {
    assert_eq!(
        dump(Value::string("Short string"), None),
        b"\x04\x08I\"\x11Short string\x06:\x06ET"
    );

    assert_eq!(
        dump(
            Value::string("Long string".repeat(20)),
            None
        ),
        b"\x04\x08I\"\x01\xdcLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong string\x06:\x06ET",
    );
}

#[test]
fn string_nonutf8() {
    assert_eq!(
        dump(Value::string("汉字内"), None),
        b"\x04\x08I\"\x0E\xE6\xB1\x89\xE5\xAD\x97\xE5\x86\x85\x06:\x06ET"
    );
}

#[test]
fn string_binary() {
    let json = Value::bytes("Short string".as_bytes());
    assert_eq!(dump(json, None), b"\x04\x08\"\x11Short string");

    let json = Value::bytes("Long string".repeat(20).as_bytes());
    assert_eq!(
        dump(
            json,
            None
        ),
        b"\x04\x08\"\x01\xdcLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong stringLong string",
    );
}

#[test]
fn links() {
    let json = load(b"\x04\x08[\x08[\x08f\x080.1@\x07@\x07[\x08f\x080.2@\t@\t[\x08f\x080.3@\x0b@\x0b", None).unwrap();

    assert_eq!(
        dump(json, None),
        b"\x04\x08[\x08[\x08f\x080.1@\x07@\x07[\x08f\x080.2@\x09@\x09[\x08f\x080.3@\x0b@\x0b"
    );
}

#[test]
fn array() {
    let map = Value::hash(HashMap::from([(Value::int(5), Value::int(6))]));
    let json = Value::array(Vec::from([
        Value::int(1),
        Value::string("two"),
        Value::float("3"),
        Value::array(Vec::from([Value::int(4)])),
        map,
    ]));

    assert_eq!(
        dump(json, None),
        b"\x04\x08[\x0ai\x06I\"\x08two\x06:\x06ETf\x063[\x06i\x09{\x06i\x0ai\x0b"
    );
}

#[test]
fn hash() {
    let mut object = Value::object(Object::new());
    object.set_class("Object".into());

    let json = Value::hash(HashMap::from([
        (Value::int(1), Value::string("one")),
        (Value::string("two"), Value::int(2)),
        (object, Value::null()),
    ]));

    assert_eq!(
        dump(
            json,
            None
        ),
        b"\x04\x08{\x08i\x06I\"\x08one\x06:\x06ETI\"\x08two\x06;\0Ti\x07o:\x0bObject\x000"
    );
}

#[test]
fn hash_default() {
    let json = Value::hash(HashMap::from([(
        Value::symbol("__ruby_default__"),
        Value::string("default"),
    )]));

    assert_eq!(dump(json, None), b"\x04\x08}\0I\"\x0cdefault\x06:\x06ET");
}

#[test]
fn ruby_struct() {
    let age_key = Value::symbol("age");
    let name_key = Value::symbol("name");

    let mut json = Value::rstruct(HashMap::from([
        (name_key, Value::string("Alice")),
        (age_key, Value::int(30)),
    ]));

    json.set_class("Person".to_owned());

    assert_eq!(
        dump(json, None),
        b"\x04\x08S:\x0bPerson\x07:\x09nameI\"\x0aAlice\x06:\x06ET:\x08agei#"
    );
}

#[test]
fn object() {
    let mut map = Object::with_capacity(1);
    map.insert("@data".to_owned(), Value::string("object data"));

    let mut json = Value::object(map);
    json.set_class("CustomObject".to_owned());

    assert_eq!(
        dump(
            json,
            None
        ),
        b"\x04\x08o:\x11CustomObject\x06:\x0a@dataI\"\x10object data\x06:\x06ET"
    );
}

#[test]
fn custom_marshal() {
    let mut json = Value::hash(HashMap::from_iter([(
        Value::symbol("data"),
        Value::string("Important Data"),
    )]));

    json.set_user_marshal(true);
    json.set_class("CustomDumpClass".to_owned());

    assert_eq!(
        dump(json, None),
        b"\x04\x08U:\x14CustomDumpClass{\x06:\tdataI\"\x13Important Data\x06:\x06ET"
    )
}

#[test]
fn extended_object() {
    let mut json = Value::bytes("I am a string".as_bytes());
    json.add_extension("MyModule".to_owned());

    assert_eq!(dump(json, None), b"\x04\x08e:\rMyModule\"\x12I am a string")
}

#[test]
fn regexp() {
    let json = Value::regexp("/aboba.*/ix");
    assert_eq!(dump(json, None), b"\x04\x08/\x0caboba.*\x03")
}

#[test]
fn custom_dump_and_load() {
    let mut json = Value::bytes("terces".as_bytes());
    json.set_user_defined(true);
    json.set_class("CustomObject".to_owned());

    assert_eq!(dump(json, None), b"\x04\x08u:\x11CustomObject\x0bterces");
}

#[test]
fn array_subclass() {
    let mut json = Value::array([Value::int(1), Value::int(2), Value::int(3)]);
    json.set_user_class(true);
    json.set_class("MyArray".to_owned());

    assert_eq!(
        dump(json, None),
        b"\x04\x08C:\x0cMyArray[\x08i\x06i\x07i\x08"
    );
}

#[test]
fn string_subclass() {
    let mut json = Value::bytes("hello".as_bytes());
    json.set_user_class(true);
    json.set_class("MyString".to_owned());

    assert_eq!(dump(json, None), b"\x04\x08C:\rMyString\"\nhello");
}

#[test]
fn regexp_subclass() {
    let mut json = Value::regexp("/foo.*bar/");
    json.set_user_class(true);
    json.set_class("MyRegexp".to_owned());

    assert_eq!(dump(json, None), b"\x04\x08C:\rMyRegexp/\rfoo.*bar\x00");
}

#[test]
fn hash_subclass() {
    let mut json = Value::hash(HashMap::from_iter([
        (Value::symbol("a"), Value::int(1)),
        (Value::symbol("b"), Value::int(2)),
    ]));

    json.set_user_class(true);
    json.set_class("MyHash".to_owned());

    assert_eq!(
        dump(json, None),
        b"\x04\x08C:\x0bMyHash{\x07:\x06ai\x06:\x06bi\x07"
    );
}
