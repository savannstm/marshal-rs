#![no_main]
//! Drives the builder and mutation API with arbitrary ids, indexes and kinds: whatever it accepts must
//! dump, and the dump must load again and dump to the same bytes.
use libfuzzer_sys::fuzz_target;
use marshal_rs::{dump, load, Arena};

fuzz_target!(|data: &[u8]| {
    let mut arena = Arena::builder();
    for op in data.chunks(4) {
        let [kind, a, b, c] = [op[0], *op.get(1).unwrap_or(&0), *op.get(2).unwrap_or(&0), *op.get(3).unwrap_or(&0)];
        // Ids are small on purpose, so most of them are real nodes and some are not.
        let (a, b, c) = (u32::from(a) % 24, u32::from(b) % 24, u32::from(c) % 24);
        let name = vec![b'A' + (a % 4) as u8];
        match kind % 55 {
            0 => drop(arena.push_nil()),
            1 => drop(arena.push_fixnum(a as i32 - 12)),
            2 => drop(arena.push_bytes(name)),
            3 => drop(arena.push_string(String::from("é"))),
            4 => drop(arena.push_symbol(name)),
            5 => drop(arena.push_array(&[a, b, c])),
            6 => drop(arena.push_hash(&[(a, b)], (c % 2 == 0).then_some(c))),
            7 => drop(arena.push_object(name, &[(b"@a".to_vec(), a)])),
            8 => drop(arena.push_struct(name, &[(b"a".to_vec(), a)])),
            9 => drop(arena.push_float(if a % 2 == 0 { b"1.5".to_vec() } else { b"x".to_vec() })),
            10 => drop(arena.push_user_marshal(name, a)),
            11 => drop(arena.push_data(name, a)),
            12 => drop(arena.set_user_class(a, name)),
            13 => drop(arena.set_user_defined(a, name)),
            14 => drop(arena.push_extension(a, name)),
            15 => drop(arena.push_extra_ivar(a, b"@z", b)),
            16 => drop(arena.set_array_value(a, b as usize % 4, c)),
            17 => drop(arena.set_hash_value(a, b as usize % 4, c)),
            18 => drop(arena.set_member_value(a, b"a", b)),
            19 => drop(arena.set_string_text(a, String::from("x"))),
            20 => drop(arena.take_bytes_content(a)),
            21 => drop(arena.push_class(name)),
            22 => drop(arena.clear_user_class(a)),
            24 => drop(arena.push_bool(a % 2 == 0)),
            25 => drop(arena.push_bignum(b % 2 == 0, vec![a as u8, b as u8, c as u8])),
            26 => drop(arena.push_regexp(name, c as u8)),
            27 => drop(arena.push_module(name, a % 2 == 0)),
            28 => drop(arena.push_str_with_encoding_name(name, if b % 2 == 0 { &b"Shift_JIS"[..] } else { &b"Bogus"[..] })),
            29 => drop(arena.push_symbol_with_encoding_name(name, &b"Bogus"[..])),
            30 => drop(arena.push_regexp_with_encoding_name(name, c as u8, &b"UTF-8"[..])),
            31 => drop(arena.set_array_string(a, b as usize % 4, String::from("y"))),
            32 => drop(arena.set_array_bytes(a, b as usize % 4, name)),
            33 => drop(arena.set_array_symbol(a, b as usize % 4, name)),
            34 => drop(arena.set_array_fixnum(a, b as usize % 4, c as i32 - 12)),
            35 => drop(arena.set_array_bool(a, b as usize % 4, c % 2 == 0)),
            36 => drop(arena.set_hash_key(a, b as usize % 4, c)),
            37 => drop(arena.set_hash_default(a, b)),
            38 => drop(arena.set_member_string(a, b"a", String::from("y"))),
            39 => drop(arena.set_member_bytes(a, b"a", name)),
            40 => drop(arena.set_member_symbol(a, b"a", name)),
            41 => drop(arena.set_member_fixnum(a, b"a", b as i32 - 12)),
            42 => drop(arena.set_member_bool(a, b"a", c % 2 == 0)),
            43 => drop(arena.set_bytes_content(a, name)),
            44 => drop(arena.set_str_content(a, name)),
            45 => drop(arena.set_encoding(a, c as u8)),
            46 => drop(arena.set_encoding_name(a, if b % 2 == 0 { &b"Shift_JIS"[..] } else { &b"Bogus"[..] })),
            47 => drop(arena.set_fixnum_value(a, c as i32 - 12)),
            48 => drop(arena.take_bignum_magnitude(a)),
            49 => drop(arena.take_float_ascii(a)),
            50 => drop(arena.take_regexp_source(a)),
            51 => drop(arena.take_class_path(a)),
            52 => drop(arena.take_module_path(a)),
            53 => drop(arena.push_symbol_with_encoding(name, c as u8)),
            54 => drop(arena.push_str_with_encoding(name, c as u8)),
            _ => drop(arena.set_root(a)),
        }
    }

    let first = dump(&arena);
    let reloaded = load(&first).expect("own output must load");
    assert!(dump(&reloaded) == first, "dump is not stable");
});
