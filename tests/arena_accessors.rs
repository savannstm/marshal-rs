#![cfg(feature = "alloc")]
//! The read-side accessors on `Arena` itself, checked against values built with the builder.

use marshal_rs::{Arena, ENCODING_ASCII_8BIT, ENCODING_CUSTOM, ENCODING_US_ASCII, ValueRef};

#[test]
fn class_and_symbol_accessors_read_back_what_was_pushed() {
    let mut arena = Arena::builder();
    let object = arena.push_object(b"Point".to_vec(), &[]).unwrap();
    let class = arena.class_of(object).unwrap();

    assert_eq!(arena.symbol_bytes(class), b"Point");
    assert_eq!(arena.symbol_str(class), Some("Point"));
    assert_eq!(arena.symbol_encoding(class), ENCODING_ASCII_8BIT);
    assert_eq!(arena.symbol_effective_encoding(class), ENCODING_US_ASCII);
    assert_eq!(arena.symbol_custom_encoding(class), None);
    assert_eq!(arena.symbol_unusual_tag(class), None);

    assert_eq!(arena.class_of(0), None, "nil has no class");
    assert_eq!(arena.class_of(999), None, "an id outside the arena has no class");
}

#[test]
fn symbol_str_is_none_for_bytes_that_are_not_utf8() {
    let mut arena = Arena::builder();
    let object = arena.push_object(vec![0xff], &[]).unwrap();
    let class = arena.class_of(object).unwrap();
    assert_eq!(arena.symbol_str(class), None);
}

#[test]
fn a_symbol_with_an_unknown_encoding_keeps_its_name() {
    let mut arena = Arena::builder();
    let symbol = arena.push_symbol_with_encoding_name(b"sym".to_vec(), b"Bogus");
    let value = ValueRef::new(&arena, symbol);
    assert_eq!(value.symbol_tag(), Some(ENCODING_CUSTOM));
    assert_eq!(value.encoding_name(), Some(&b"Bogus"[..]));
}

#[test]
fn extensions_are_listed_per_value() {
    let mut arena = Arena::builder();
    let object = arena.push_object(b"Foo".to_vec(), &[]).unwrap();
    assert!(arena.push_extension(object, b"Mixin".to_vec()));

    let modules: Vec<&[u8]> = arena.extensions_of(object).map(|sym| arena.symbol_bytes(sym)).collect();
    assert_eq!(modules, [&b"Mixin"[..]]);
    assert_eq!(arena.extensions_of(0).count(), 0);
}

#[test]
fn custom_encoding_of_names_only_unknown_encodings() {
    let mut arena = Arena::builder();
    let tagged = arena.push_str_with_encoding_name(b"x".to_vec(), b"Bogus");
    let known = arena.push_str_with_encoding_name(b"y".to_vec(), b"UTF-8");
    assert_eq!(arena.custom_encoding_of(tagged), Some(&b"Bogus"[..]));
    assert_eq!(arena.custom_encoding_of(known), None);
}

#[test]
fn a_marshal_dump_wrapper_points_at_its_body() {
    let mut arena = Arena::builder();
    let body = arena.push_fixnum(1);
    let wrapper = arena.push_user_marshal(b"Wrap".to_vec(), body).unwrap();

    assert_eq!(arena.body_of(wrapper), Some(body));
    assert_eq!(arena.body_of(body), None, "a plain value has no body");
    assert_eq!(arena.body_of(999), None);
    assert!(arena.class_of(wrapper).is_some());
}
