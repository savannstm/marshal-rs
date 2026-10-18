//! The builder and in-place mutation API must refuse ids and indexes that don't fit, not corrupt the arena.
#![cfg(feature = "alloc")]

use marshal_rs::{Arena, ValueRef, dump, load};

#[test]
fn empty_builder_arena_dumps_as_nil() {
    assert_eq!(dump(&Arena::builder()), b"\x04\x080");
}

#[test]
fn containers_reject_ids_outside_the_arena() {
    let mut arena = Arena::builder();
    let one = arena.push_fixnum(1);
    assert!(arena.push_array(&[one, 99]).is_none());
    assert!(arena.push_hash(&[(one, 99)], None).is_none());
    assert!(arena.push_hash(&[(one, one)], Some(99)).is_none());
    assert!(arena.push_object(b"Foo".to_vec(), &[(b"@a".to_vec(), 99)]).is_none());
    assert!(arena.push_struct(b"Foo".to_vec(), &[(b"a".to_vec(), 99)]).is_none());
    assert!(!arena.set_root(99));
    assert!(arena.set_root(one));
}

#[test]
fn slot_setters_stay_inside_their_container() {
    // [[1], [2]]: index 1 of the first array would land on the second array's slot if unchecked.
    let mut arena = Arena::builder();
    let one = arena.push_fixnum(1);
    let two = arena.push_fixnum(2);
    let first = arena.push_array(&[one]).unwrap();
    let second = arena.push_array(&[two]).unwrap();
    let outer = arena.push_array(&[first, second]).unwrap();
    arena.set_root(outer);
    let before = dump(&arena);

    assert!(arena.set_array_value(first, 1, one).is_none());
    assert!(arena.set_array_value(one, 0, one).is_none());
    assert!(arena.set_array_value(999, 0, one).is_none());
    assert!(arena.set_hash_key(first, 0, one).is_none());
    assert!(arena.set_hash_value(first, 0, one).is_none());
    assert_eq!(dump(&arena), before);

    assert_eq!(arena.set_array_value(first, 0, two), Some(one));
}

#[test]
fn content_setters_check_the_kind() {
    let mut arena = Arena::builder();
    let number = arena.push_fixnum(7);
    let text = arena.push_string("hi".into());
    assert!(arena.set_string_text(number, "x".into()).is_none());
    assert!(arena.set_bytes_content(text, b"x".to_vec()).is_none());
    assert!(arena.set_fixnum_value(text, 1).is_none());
    assert!(arena.take_bytes_content(number).is_none());
    assert!(arena.take_float_ascii(text).is_none());

    assert_eq!(arena.set_fixnum_value(number, 8), Some(7));
    assert!(arena.set_string_text(text, "yo".into()).is_some());
    assert_eq!(ValueRef::new(&arena, text).as_str(), Some("yo"));
    assert_eq!(arena.take_bytes_content(text), Some(b"yo".to_vec()));
}

#[test]
fn loaded_arena_can_be_edited_and_dumped() {
    let bytes = b"\x04\x08[\x07i\x06i\x07";
    let mut arena = load(bytes).unwrap().into_owned();
    let root = arena.root();
    let nine = arena.push_fixnum(9);
    arena.set_array_value(root, 1, nine).unwrap();
    assert_eq!(dump(&arena), b"\x04\x08[\x07i\x06i\x0e");
}

#[test]
fn symbols_pushed_after_into_owned_reuse_the_loaded_ones() {
    let mut arena = load(b"\x04\x08[\x06:\x06a").unwrap().into_owned();
    let old = ValueRef::root(&arena).at(0).unwrap().id();
    let new = arena.push_symbol(b"a".to_vec());
    let both = arena.push_array(&[old, new]).unwrap();
    arena.set_root(both);
    assert_eq!(dump(&arena), b"\x04\x08[\x07:\x06a;\x00");
}

#[test]
fn builder_refuses_values_ruby_would_not_read_back() {
    let mut arena = Arena::builder();
    assert!(arena.push_str_with_encoding(b"x".to_vec(), 200).is_none());
    assert!(
        arena
            .push_str_with_encoding(b"x".to_vec(), marshal_rs::ENCODING_CUSTOM)
            .is_none()
    );
    assert!(
        arena
            .push_str_with_encoding(b"x".to_vec(), marshal_rs::ENCODING_UTF_8)
            .is_some()
    );
    assert!(arena.push_class(Vec::new()).is_none());
    assert!(arena.push_module(Vec::new(), false).is_none());
    assert!(arena.push_object(Vec::new(), &[]).is_none());
    assert!(arena.push_struct(Vec::new(), &[]).is_none());
}

#[test]
fn typed_setters_keep_dump_consistent() {
    let mut arena = Arena::builder();
    let object = arena.push_object(b"Foo".to_vec(), &[]).unwrap();
    let array = arena.push_array(&[]).unwrap();
    let text = arena.push_bytes(b"data".to_vec());
    let nil = arena.push_nil();
    let one = arena.push_fixnum(1);

    // Only the five built-in types take a subclass; only bytes and strings are a `_dump`.
    assert!(!arena.set_user_class(object, b"Bar".to_vec()));
    assert!(!arena.set_user_class(array, Vec::new()));
    assert!(!arena.set_user_class(999, b"Bar".to_vec()));
    assert!(arena.set_user_class(array, b"Bar".to_vec()));
    assert!(!arena.set_user_defined(array, b"Baz".to_vec()));
    assert!(arena.set_user_defined(text, b"Table".to_vec()));
    assert!(!arena.set_user_class(text, b"Bar".to_vec()));

    // Immediates cannot be extended or carry instance variables.
    assert!(!arena.push_extension(one, b"M".to_vec()));
    assert!(!arena.push_extension(nil, b"M".to_vec()));
    assert!(!arena.push_extra_ivar(one, b"@a", nil));
    assert!(!arena.push_extra_ivar(object, b"@a", 999));
    assert!(!arena.push_extra_ivar(object, b"", nil));
    assert!(arena.push_extension(object, b"M".to_vec()));
    assert!(arena.push_extension(object, b"N".to_vec()));
    assert!(arena.push_extra_ivar(object, b"@a", one));

    let all = arena.push_array(&[object, array, text]).unwrap();
    arena.set_root(all);
    assert_eq!(
        dump(&arena),
        b"\x04\x08[\x08e:\x06Me:\x06No:\x08Foo\x06:\x07@ai\x06C:\x08Bar[\x00u:\x0aTable\x09data"
    );
}

#[test]
fn builder_wrappers_round_trip_through_load() {
    let mut arena = Arena::builder();
    let inner = arena.push_object(b"Bar".to_vec(), &[]).unwrap();
    let wrapper = arena.push_user_marshal(b"Foo".to_vec(), inner).unwrap();
    assert!(arena.push_data(Vec::new(), inner).is_none());
    assert!(arena.push_data(b"Foo".to_vec(), 999).is_none());
    arena.set_root(wrapper);
    let bytes = dump(&arena);
    assert_eq!(bytes, b"\x04\x08U:\x08Fooo:\x08Bar\x00");
    let back = load(&bytes).unwrap();
    assert_eq!(dump(&back), bytes);
}

#[test]
fn edits_through_a_wrapper_reach_its_body() {
    let mut arena = Arena::builder();
    let one = arena.push_fixnum(1);
    let two = arena.push_fixnum(2);
    let body = arena.push_array(&[one]).unwrap();
    let wrapper = arena.push_data(b"Foo".to_vec(), body).unwrap();
    arena.set_root(wrapper);
    assert_eq!(arena.set_array_value(wrapper, 0, two), Some(one));
    assert_eq!(dump(&arena), b"\x04\x08d:\x08Foo[\x06i\x07");
}

#[test]
fn a_builder_arena_dumps_nil_until_a_root_is_set() {
    let mut arena = Arena::builder();
    let _ignored = arena.push_fixnum(7);
    assert_eq!(dump(&arena), b"\x04\x080");
}

#[test]
fn numbers_compare_by_value_however_they_are_stored() {
    let mut arena = Arena::builder();
    let key = arena.push_fixnum(1);
    let value = arena.push_symbol(b"one".to_vec());
    let hash = arena.push_hash(&[(key, value)], None).unwrap();
    arena.set_root(hash);

    let mut other = Arena::builder();
    // 1 as a bignum with padding zeros, and a bignum no `i64` holds.
    let padded = other.push_bignum(false, vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let huge = other.push_bignum(false, vec![0xff; 9]);
    let huge_padded = other.push_bignum(false, vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0]);
    assert_eq!(ValueRef::new(&other, padded).as_i64(), Some(1));
    assert_eq!(ValueRef::new(&other, huge).as_i64(), None);

    let root = ValueRef::root(&arena);
    assert!(root.lookup(ValueRef::new(&other, padded)).is_some());
    assert!(root.lookup(ValueRef::new(&other, huge)).is_none());

    let mut third = Arena::builder();
    let key = third.push_bignum(false, vec![0xff; 9]);
    let value = third.push_nil();
    let big_hash = third.push_hash(&[(key, value)], None).unwrap();
    third.set_root(big_hash);
    let big_root = ValueRef::root(&third);
    assert!(big_root.lookup(ValueRef::new(&other, huge)).is_some());
    assert!(big_root.lookup(ValueRef::new(&other, huge_padded)).is_some());
}

#[test]
fn as_str_requires_a_text_encoding() {
    let mut arena = Arena::builder();
    let utf8 = arena.push_string("é".into());
    let sjis = arena.push_str_with_encoding_name(b"abc".to_vec(), b"Shift_JIS");
    assert_eq!(ValueRef::new(&arena, utf8).as_str(), Some("é"));
    assert_eq!(ValueRef::new(&arena, sjis).as_str(), None);
    assert_eq!(ValueRef::new(&arena, sjis).as_bytes(), Some(&b"abc"[..]));
}

#[test]
fn floats_are_stored_unchecked() {
    let mut arena = Arena::builder();
    let tails: [&[u8]; 4] = [
        b"0.1\0\x99\x99\x9a",
        b"1.5\0\xff",
        b"nan\0\x80",
        b"abc\xffnot a number at all",
    ];
    let ids: Vec<u32> = tails.iter().map(|tail| arena.push_float(tail.to_vec())).collect();
    let array = arena.push_array(&ids).unwrap();
    arena.set_root(array);
    let dumped = dump(&arena);
    let back = load(&dumped).unwrap();
    let root = ValueRef::root(&back);
    for (index, tail) in tails.iter().enumerate() {
        assert_eq!(root.at(index).unwrap().as_float_bytes(), Some(*tail));
    }
    // Nothing was checked on the way in, so the one that is not a number is just not a number.
    assert_eq!(root.at(3).unwrap().as_f64(), None);
}

#[test]
fn member_names_match_with_or_without_the_at() {
    let mut arena = Arena::builder();
    let one = arena.push_fixnum(1);
    let object = arena.push_object(b"Foo".to_vec(), &[(b"@a".to_vec(), one)]).unwrap();
    let strukt = arena.push_struct(b"Bar".to_vec(), &[(b"a".to_vec(), one)]).unwrap();
    for (target, names) in [(object, [&b"@a"[..], b"a"]), (strukt, [&b"a"[..], b"a"])] {
        for name in names {
            let two = arena.push_fixnum(2);
            assert!(arena.set_member_value(target, name, two).is_some(), "{target} {name:?}");
            assert!(arena.set_member_fixnum(target, name, 3).is_some(), "{target} {name:?}");
        }
        assert!(arena.set_member_fixnum(target, b"@b", 3).is_none());
        assert!(arena.set_member_fixnum(target, b"b", 3).is_none());
    }
}

#[test]
fn an_extra_ivar_on_an_object_is_one_more_member() {
    let mut arena = Arena::builder();
    let one = arena.push_fixnum(1);
    let object = arena.push_object(b"Foo".to_vec(), &[(b"@a".to_vec(), one)]).unwrap();
    let two = arena.push_fixnum(2);
    assert!(arena.push_extra_ivar(object, b"@b", two));
    arena.set_root(object);
    // `o:\bFoo` with two ivars, no `I` wrapper (which only strings, arrays and the like get).
    assert_eq!(dump(&arena), b"\x04\x08o:\x08Foo\x07:\x07@ai\x06:\x07@bi\x07");
    let dumped = dump(&arena);
    let back = load(&dumped).unwrap();
    assert_eq!(ValueRef::root(&back).get("b").and_then(|v| v.as_i64()), Some(2));
}

#[test]
fn class_and_module_paths_that_are_not_ascii_are_tagged_utf8_like_rubys() {
    let mut arena = Arena::builder();
    let class = arena.push_class("Café".as_bytes().to_vec()).unwrap();
    let module = arena.push_module("Módulo".as_bytes().to_vec(), false).unwrap();
    let plain = arena.push_class(b"String".to_vec()).unwrap();
    let binary = arena.push_class(b"Caf\xe9".to_vec()).unwrap();
    let array = arena.push_array(&[class, module, plain, binary]).unwrap();
    arena.set_root(array);
    assert_eq!(
        dump(&arena),
        b"\x04\x08[\x09Ic\x0aCaf\xc3\xa9\x06:\x06ETIm\x0cM\xc3\xb3dulo\x06;\x00Tc\x0bStringc\x09Caf\xe9".as_slice()
    );
    // What a stream says wins over the default: an `E` that is given replaces the one the builder put.
    let dumped = dump(&arena);
    let again = load(&dumped).unwrap();
    assert_eq!(dump(&again), dumped);
}

/// Ruby numbers a `_dump` value after its instance variables, so a link past it means the value and not one of them.
#[test]
fn a_dump_value_is_numbered_after_its_instance_variables() {
    // [ Test("ab", z: "x"), @2, @1 ]: object 1 is "x", object 2 the Test value.
    let bytes = b"\x04\x08[\x08Iu:\x09Test\x07ab\x06:\x06z\"\x06x@\x07@\x06";
    let arena = load(bytes).unwrap();
    let root = ValueRef::root(&arena);
    let (first, linked, ivar) = (root.at(0).unwrap(), root.at(1).unwrap(), root.at(2).unwrap());
    assert_eq!(first.id(), linked.id());
    assert!(linked.is_user_defined());
    assert_eq!(linked.class_name(), Some(&b"Test"[..]));
    assert_eq!(ivar.as_bytes(), Some(&b"x"[..]));
    assert_eq!(dump(&arena), bytes);

    // And the other way: a value built with the API is numbered where Ruby numbers it.
    let mut built = Arena::builder();
    let data = built.push_bytes(b"ab".to_vec());
    assert!(built.set_user_defined(data, b"Test".to_vec()));
    let value = built.push_bytes(b"x".to_vec());
    assert!(built.push_extra_ivar(data, b"z", value));
    let array = built.push_array(&[data, data, value]).unwrap();
    built.set_root(array);
    assert_eq!(dump(&built), bytes);
}

/// A subclass or `extend` marker may name its class with an encoding of its own; the name is an object and is
/// numbered before the value the marker is on.
#[test]
fn markers_with_a_tagged_name_number_the_name_first() {
    // [ e(Extra tagged Shift_JIS) o:Plain, @1, @2 ]: object 1 is the name "Shift_JIS", object 2 the Plain.
    let bytes = b"\x04\x08[\x08eI:\x0aExtra\x06:\x0dencoding\"\x0eShift_JISo:\x0aPlain\x00@\x06@\x07";
    let arena = load(bytes).unwrap();
    let root = ValueRef::root(&arena);
    assert_eq!(root.at(1).unwrap().as_bytes(), Some(&b"Shift_JIS"[..]));
    assert_eq!(root.at(2).unwrap().id(), root.at(0).unwrap().id());
    assert_eq!(root.at(0).unwrap().extensions().collect::<Vec<_>>(), [&b"Extra"[..]]);
    assert_eq!(dump(&arena), bytes);
}

/// Found by fuzzing (`fuzz/`): taking the magnitude of a negative bignum left a negative zero, which dumped as `l-`
/// and reloaded as `l+`, so the dump changed on reload.
#[test]
fn taking_a_negative_bignums_magnitude_leaves_zero_positive() {
    let mut arena = Arena::builder();
    let id = arena.push_bignum(true, vec![1, 2]);
    assert_eq!(arena.take_bignum_magnitude(id), Some(vec![1, 2]));
    let _ = arena.set_root(id);

    let dumped = dump(&arena);
    assert_eq!(dumped, b"\x04\x08l+\x00");
    assert_eq!(dump(&load(&dumped).unwrap()), dumped);
}

/// Found by fuzzing (`fuzz/`): taking a class path left an empty name, which neither this crate nor Ruby loads. The
/// class becomes `nil`, and its `E` ivar (a non-ASCII name carries one) goes with it.
#[test]
fn taking_a_class_path_leaves_nil() {
    let mut arena = Arena::builder();
    let class = arena.push_class("Ünit".as_bytes().to_vec()).unwrap();
    assert_eq!(arena.take_class_path(class), Some("Ünit".as_bytes().to_vec()));
    assert!(arena.set_root(class));

    let dumped = dump(&arena);
    assert_eq!(dumped, b"\x04\x080");
    assert!(ValueRef::root(&load(&dumped).unwrap()).is_nil());
}

/// The module counterpart of [`taking_a_class_path_leaves_nil`].
#[test]
fn taking_a_module_path_leaves_nil() {
    let mut arena = Arena::builder();
    let module = arena.push_module(b"Mixin".to_vec(), false).unwrap();
    assert_eq!(arena.take_module_path(module), Some(b"Mixin".to_vec()));
    assert!(arena.set_root(module));

    let dumped = dump(&arena);
    assert_eq!(dumped, b"\x04\x080");
    assert!(ValueRef::root(&load(&dumped).unwrap()).is_nil());
}

/// Taking a float's text leaves `0`, which loads as `0.0`; an empty text would dump as a float with no value.
#[test]
fn taking_a_float_text_leaves_zero() {
    let mut arena = Arena::builder();
    let id = arena.push_float(b"1.5".to_vec());
    assert_eq!(arena.take_float_ascii(id), Some(b"1.5".to_vec()));
    assert!(arena.set_root(id));

    let dumped = dump(&arena);
    assert_eq!(dumped, b"\x04\x08f\x060");
    assert_eq!(ValueRef::root(&load(&dumped).unwrap()).as_f64(), Some(0.0));
}

/// Taking a regexp source leaves an empty source, which loads as the regexp `//`.
#[test]
fn taking_a_regexp_source_leaves_an_empty_regexp() {
    let mut arena = Arena::builder();
    let id = arena.push_regexp(b"abc".to_vec(), 1);
    assert_eq!(arena.take_regexp_source(id), Some(b"abc".to_vec()));
    assert!(arena.set_root(id));

    let dumped = dump(&arena);
    let loaded = load(&dumped).unwrap();
    assert_eq!(ValueRef::root(&loaded).as_regexp(), Some((&b""[..], 1)));
}
