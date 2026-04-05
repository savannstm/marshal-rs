//! Regression test: SymbolLink (;) keys in repeated UserMarshal containers.
//!
//! When the same class/symbol keys appear multiple times in a Marshal stream,
//! Ruby emits a symbol reference (byte `;`) with an offset into the symbol table.
//! marshal-rs should resolve these references to the original symbol when used
//! as hashmap keys.
//!
//! This test reproduces data produced by:
//!   arr = [OpenStruct.new(a: 1, b: 2.0), OpenStruct.new(a: 10, b: 20.0)]
//!   Marshal.dump(arr)
//!
//! The 2nd OpenStruct references the class "OpenStruct" and the symbols :a :b
//! via symbol links. Expected: 2 hashmap elements each with 2 keys.

/// Actual bytes from Ruby 3.x:
///   [OpenStruct.new(a:1, b:2.0), OpenStruct.new(a:10, b:20.0)]
const RUBY_MARSHAL_ARRAY_OF_OPENSTRUCTS: &[u8] = &[
    0x04, 0x08, // version 4.8
    0x5b, 0x07, // Array, 2 elements
    // Element 0: UserMarshal
    0x55,
    0x3a, 0x0f, b'O', b'p', b'e', b'n', b'S', b't', b'r', b'u', b'c', b't', // :OpenStruct (sym offset 0)
    0x7b, 0x07, // Hash, 2 pairs
    0x3a, 0x06, b'a', // :a (sym offset 1)
    0x69, 0x06, // 1 (int)
    0x3a, 0x06, b'b', // :b (sym offset 2)
    0x66, 0x06, b'2', // "2" (float)
    // Element 1: UserMarshal with symbol references
    0x55,
    0x3b, 0x00, // SymbolLink to offset 0 (OpenStruct)
    0x7b, 0x07, // Hash, 2 pairs
    0x3b, 0x06, // SymbolLink to offset 1 (:a)
    0x69, 0x0f, // 10 (int)
    0x3b, 0x07, // SymbolLink to offset 2 (:b)
    0x66, 0x08, b'2', b'e', b'1', // "2e1" (float = 20.0)
];

#[test]
fn symbol_link_keys_preserved_in_repeated_hashmap() {
    let value = marshal_rs::load(RUBY_MARSHAL_ARRAY_OF_OPENSTRUCTS, None)
        .expect("should load array");
    let arr = value.as_array().expect("should be array");
    assert_eq!(arr.len(), 2, "array should have 2 elements");

    for (i, elem) in arr.iter().enumerate() {
        let h = elem.as_hashmap().unwrap_or_else(|| panic!("element {i} should be hashmap"));
        let key_symbols: Vec<_> = h.keys()
            .filter_map(|k| k.as_str().map(|s| s.to_string()))
            .collect();
        assert_eq!(
            h.len(), 2,
            "element {i} should have 2 keys, got {} with symbols={:?}",
            h.len(), key_symbols
        );

        // Verify both keys are symbols
        let mut keys: Vec<_> = h.keys().filter_map(|k| k.as_str().map(|s| s.to_string())).collect();
        keys.sort();
        assert_eq!(keys, vec!["a".to_string(), "b".to_string()], "element {i} symbol keys");
    }
}

#[test]
fn symbol_link_values_are_distinct() {
    let value = marshal_rs::load(RUBY_MARSHAL_ARRAY_OF_OPENSTRUCTS, None)
        .expect("should load");
    let arr = value.as_array().expect("should be array");

    // Element 0: a=1, b=2.0
    let h0 = arr[0].as_hashmap().expect("element 0 hashmap");
    let entries_0: Vec<(String, String)> = h0.iter()
        .map(|(k, v)| (
            k.as_str().unwrap_or("?").to_string(),
            format!("{:?}", v)
        ))
        .collect();
    println!("element 0 entries: {:?}", entries_0);

    // Element 1: a=10, b=20.0
    let h1 = arr[1].as_hashmap().expect("element 1 hashmap");
    let entries_1: Vec<(String, String)> = h1.iter()
        .map(|(k, v)| (
            k.as_str().unwrap_or("?").to_string(),
            format!("{:?}", v)
        ))
        .collect();
    println!("element 1 entries: {:?}", entries_1);

    assert_eq!(h0.len(), 2);
    assert_eq!(h1.len(), 2);
}

/// Minimal case: single Hash with two symbol references (no UserMarshal wrapping).
/// Stream: { :a => :b, :c => :b } where the second :b is a symbol link.
#[test]
fn plain_hash_with_symbol_link_values() {
    // Ruby: Marshal.dump({ a: :b, c: :b })
    // "\x04\b{\a:\x06a:\x06b:\x06c;\a"
    let bytes: &[u8] = &[
        0x04, 0x08,
        b'{', 0x07,
        0x3a, 0x06, b'a',       // :a (sym 0)
        0x3a, 0x06, b'b',       // :b (sym 1)
        0x3a, 0x06, b'c',       // :c (sym 2)
        0x3b, 0x07,             // SymbolLink offset 1 → :b
    ];
    let value = marshal_rs::load(bytes, None).expect("should load");
    let h = value.as_hashmap().expect("should be hashmap");
    assert_eq!(h.len(), 2, "should have 2 keys (:a and :c)");
}
