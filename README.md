# marshal-rs

**`marshal-rs` is a Rust implementation of Ruby-lang's `Marshal`.**

This project is essentially just [@savannstm/marshal](https://github.com/savannstm/marshal), rewritten using Rust.
It is capable of :fire: **_BLAZINGLY FAST_** loading dumped from Ruby Marshal files, as well as :fire: **_BLAZINGLY FAST_** dumping them back to Marshal format.

## Quick overview

This crate has two main functions: `load()` and `dump()`.
`load()` takes a `&[u8]`, consisting of read Marshal data bytes (that can be read using std::fs::read()) as its only argument, and outputs serde_json::Value (sonic_rs::Value, if "sonic" feature is enabled).

If serializes Ruby data to JSON using the table:

| Ruby object                                  | Serialized to JSON                                                       |
| -------------------------------------------- | ------------------------------------------------------------------------ |
| nil                                          | null                                                                     |
| 1337 (Integer)                               | 1337                                                                     |
| 13.37 (Float)                                | 13.37                                                                    |
| "ligma" (String)                             | { "\_\_type": "bytes", "data": [108, 105, 103, 109, 97] } (Plain object) |
| :ligma (Symbol)                              | "\_\_symbol\_\_ligma"                                                    |
| /lgma/i (Regex)                              | { "\_\_type": "regexp", "expression": "lgma", flags: "i" }               |
| [] (Array)                                   | []                                                                       |
| {} (Hash)                                    | {} (Plain object)                                                        |
| Object.new (Including structs, modules etc.) | { "\_\_class": "\_\_symbol\_\_Object", "\_\_type": "object" }            |

As you can see, marshal-rs serializes strings as objects containing `data` key, that itself contains a bytes array, representing the string. It currently does not support serialization of Ruby strings to UTF-16 encoded strings, but it will be added someday.

Objects, that cannot be serialized in JSON (such as objects and symbols), marshal-rs uses approach of stringifying and adding prefixes and properties. It stringifyies symbols and prefixes them with `__symbol__`, and serializes objects' classes and types as `__class` keys and `__type` keys respectively.

load(), in turn, takes serialized JSON object and serializes it back to Ruby Marshal format. It does not preserve strings' encoding (but someday will), as well as does not writes links, which effectively means that output Marshal data might be larger in size than initial.

## Quick example

```rust
use std::fs::read;
use marshal_rs::load::load;
use marshal_rs::dump::dump;

fn main() {
    // Read marshal data
    let marshal_data: Vec<u8> = read("./marshal_file.marshal").unwrap();

    // Serializing to json
    // load() takes a &[u8] as argument, so bytes Vec must be borrowed
    let serialized_to_json: serde_json::Value = load(&marshal_data);

    // Here you may std::fs::write() serialized JSON to file

    // Serializing back to marshal
    // dump() requires owned Value as argument
    let serialized_to_marshal: Vec<u8> = dump(serialized_to_json);

    // Here you may std::fs::write() serialized Marshal data to file
}
```

## License

Project is licensed under WTFPL.
