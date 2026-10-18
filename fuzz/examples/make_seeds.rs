//! Writes the fuzz corpora from samples the library builds itself, so no fixtures are needed. Run from `fuzz/`:
//! `cargo run --locked --example make_seeds`
use marshal_rs::{Arena, dump};
use std::fs;

const TARGETS: [&str; 7] = ["load", "ser", "de", "bignum", "builder", "reader", "capi"];

// One arena with containers and most scalar kinds, and one with only a root.
fn samples() -> [Arena<'static>; 2] {
    let mut arena = Arena::builder();
    let nil = arena.push_nil();
    let fixnum = arena.push_fixnum(42);
    let text = arena.push_string(String::from("hello"));
    let symbol = arena.push_symbol(b"sym".to_vec());
    let flag = arena.push_bool(true);
    let float = arena.push_float(b"1.5".to_vec());
    let bytes = arena.push_bytes(b"raw".to_vec());
    let array = arena
        .push_array(&[nil, fixnum, text, symbol, flag, float, bytes])
        .expect("every element was pushed");
    let hash = arena
        .push_hash(&[(symbol, fixnum)], Some(nil))
        .expect("keys and default were pushed");
    let root = arena.push_array(&[array, hash]).expect("elements were pushed");
    assert!(arena.set_root(root));

    let mut bare = Arena::builder();
    let root = bare.push_nil();
    assert!(bare.set_root(root));

    [arena, bare]
}

fn main() {
    for target in TARGETS {
        fs::create_dir_all(format!("corpus/{target}")).unwrap();
    }

    for (index, arena) in samples().iter().enumerate() {
        let bytes = dump(arena);
        for target in ["load", "ser", "reader", "capi"] {
            fs::write(format!("corpus/{target}/sample-{index}"), &bytes).unwrap();
        }
        // `de` takes the serde JSON of the same arena.
        fs::write(
            format!("corpus/de/sample-{index}.json"),
            serde_json::to_vec(arena).unwrap(),
        )
        .unwrap();
    }

    // `bignum` takes a sign byte and a little-endian magnitude, or decimal text.
    let magnitudes: [&[u8]; 3] = [
        b"\x00\x01",
        b"\x01\xff\xff\xff\xff\xff\xff\xff\xff",
        b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x01",
    ];
    for (index, magnitude) in magnitudes.iter().enumerate() {
        fs::write(format!("corpus/bignum/magnitude-{index}"), magnitude).unwrap();
    }
    fs::write("corpus/bignum/decimal", "123456789012345678901234567890").unwrap();
    fs::write("corpus/bignum/negative", "-42").unwrap();

    // `builder` reads 4-byte operations: a kind, then three operands. One operation of each kind.
    let operations: Vec<u8> = (0..55u8).flat_map(|kind| [kind, 1, 2, 3]).collect();
    fs::write("corpus/builder/every-kind", operations).unwrap();
}
