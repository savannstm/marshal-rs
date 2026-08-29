use marshal_rs::{ValueRef, dump, load};

fn main() {
    // Marshal.dump({ name: "Alice", hp: 30, tags: [:hero, :fire] })
    let bytes: Vec<u8> = vec![
        0x04, 0x08, 0x7b, 0x08, // Hash, 3 pairs
        0x3a, 0x09, b'n', b'a', b'm', b'e', // :name
        0x49, 0x22, 0x0a, b'A', b'l', b'i', b'c', b'e', 0x06, 0x3a, 0x06, b'E', 0x54, // "Alice" (UTF-8 wrapped)
        0x3a, 0x07, b'h', b'p', // :hp
        0x69, 0x23, // 30
        0x3a, 0x09, b't', b'a', b'g', b's', // :tags
        0x5b, 0x07, // Array, 2 elements
        0x3a, 0x09, b'h', b'e', b'r', b'o', // :hero
        0x3a, 0x09, b'f', b'i', b'r', b'e', // :fire
    ];

    let arena = load(&bytes).expect("valid Marshal data");
    let root = ValueRef::root(&arena);

    println!("name: {:?}", root.lookup_symbol("name").and_then(|v| v.as_str()));
    println!("hp: {:?}", root.lookup_symbol("hp").and_then(|v| v.as_i64()));
    if let Some(tags) = root.lookup_symbol("tags") {
        let names: Vec<_> = tags.array().filter_map(|t| t.as_symbol_bytes()).collect();
        println!("tags: {names:?}");
    }

    // Round-trip back to Marshal bytes.
    let re_dumped = dump(&arena);
    assert_eq!(re_dumped, bytes);
    println!("round-trip OK ({} bytes)", re_dumped.len());
}
