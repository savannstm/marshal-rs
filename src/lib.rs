//! # marshal-rs
//!
//! **`marshal-rs` is a Rust implementation of Ruby-lang's `Marshal`.**
//!
//! This project is a **complete** Rust implementation of Ruby Marshal.
//! It is capable of :fire: **_BLAZINGLY FAST_** loading data from dumped Ruby Marshal files, as well as :fire: **_BLAZINGLY FAST_** dumping it back to Marshal format.
//!
//! ## Installation
//!
//! `cargo add marshal-rs`
//!
//! ## Overview
//!
//! This crate has two main structs, `Loader` and `Dumper`, along with helper functions that use them internally. There's three load functions: `load()`, `load_utf8()`, `load_binary()`, and a single dump function: `dump()`. For more information, see [Strings](#strings).
//!
//! `load()` takes a `&[u8]`, consisting of Marshal data bytes (that can be read using `std::fs::read()`) as its only argument, and outputs `UuidValue`.
//!
//! `dump()`, in turn, takes `UuidValue` as its only argument and serializes it back to `Vec<u8>` Marshal byte stream. It does not preserve strings' initial encoding, writing all strings as UTF-8 encoded.
//!
//! `UuidValue` is defined in `marshal_rs::types`. The reason this crate wraps around `serde_json::Value` this way, is because it needs to track unique object instances, like in Ruby, to properly write object links and produce the same files, as Marshal.
//!
//! The table shows, how `marshal-rs` serializes Ruby types to JSON:
//!
//! | Ruby object                                    | Serialized to JSON                                                        |
//! | ---------------------------------------------- | ------------------------------------------------------------------------- |
//! | `nil`                                          | `null`                                                                    |
//! | `1337` (Integer)                               | `1337`                                                                    |
//! | `36893488147419103232` (Big Integer)           | `{ __type: "bigint", value: "36893488147419103232" }` (Plain object)      |
//! | `13.37` (Float)                                | `"__float__13.37"`                                                        |
//! | `"ligma"` (String)                             | `"ligma"`                                                                 |
//! | `:ligma` (Symbol)                              | `"__symbol__ligma"`                                                       |
//! | `/lgma/i` (Regex)                              | `{ "__type": "regexp", "expression": "lgma", flags: "i" }` (Plain object) |
//! | `[]` (Array)                                   | `[]`                                                                      |
//! | `{}` (Hash)                                    | `{}` (Plain object)                                                       |
//! | `Object.new` (Including structs, modules etc.) | `{ "__class": "__symbol__Object", "__type": "object" }` (Plain object)    |
//!
//! ### Strings
//!
//! By default, in `load()` function, Ruby strings, that include encoding instance variable, are serialized to JSON strings, and those which don't, serialized to `{ __type: "bytes", data: [...] }` objects.
//!
//! `load_utf8()` function tries to convert arrays without instance variable to string, and produces string if array is valid UTF8, and object otherwise.
//!
//! `load_binary()` function converts all strings to objects.
//!
//! This behavior also can be controlled in `Loader` by calling `set_string_mode()`.
//!
//! ### Objects and Symbols
//!
//! For objects, that cannot be serialized in JSON (such as `Object` and `Symbol`), `marshal-rs` uses approach of stringifying and adding prefixes and properties. It stringifyies symbols and prefixes them with `__symbol__`, and serializes objects' classes and types as `__class` keys and `__type` keys respectively.
//!
//! ### Floats
//!
//! To avoid loss of precision, floats are stored as strings with `__float__` prefix.
//!
//! If Marshal file contains any extra float mantissa bits, `marshal-rs` discards them. They aren't written by latest 4.8 version of Marshal, but it still preserves them, if encounters any. `marshal-rs` does not.
//!
//! ### Hash keys
//!
//! For Hash keys, that in Ruby may be represented with any type, `marshal-rs` tries to preserve key type with prefixing stringifiyed key with it type. For example, Ruby `{ 1 => nil }` Hash will be converted to `{ "__integer__1": null }` object.
//!
//! The table shows the prefixes for the types:
//!
//! | Type      | Prefix        |
//! | --------- | ------------- |
//! | `Nil`     | `__null__`    |
//! | `Boolean` | `__boolean__` |
//! | `Integer` | `__integer__` |
//! | `Float`   | `__float__`   |
//! | `Object`  | `__object__`  |
//! | `Array`   | `__array__`   |
//!
//! ### Instance variables
//!
//! Instance variables are always decoded as strings with `__symbol__` prefix.
//! You can manage the prefix of instance variables using `instance_var_prefix` argument in `load()` and `dump()`, or by using `set_instance_var_prefix()` function in `Loader` or `Dumper`. Passed string replaces "@" instance variables' prefixes.
//!
//! ### Object links
//!
//! We nailed writing object links. Dumped objects should be exactly similar to the objects dumped with Ruby.
//!
//! ### Unsafe code
//!
//! In this crate, unsafe code provides the ability to replicate Marshal's behavior. It shouldn't ever cause problems.
//!
//! ## Example
//!
//! ```rust
//! use std::fs::read;
//! use marshal_rs::{load, dump, UuidValue};
//!
//! fn main() {
//!     // Read marshal data from file
//!     // let marshal_data: Vec<u8> = read("./Map001.rvdata2").unwrap();
//!     // For this example, we'll just take pre-defined marshal data
//!     let marshal_data: Vec<u8> = [0x04, 0x08, 0x30].to_vec();
//!
//!     // Serializing to json
//!     // `load()` takes a `&[u8]` as argument, so `Vec<u8>` must be borrowed
//!     let serialized_to_json: UuidValue = load(&marshal_data, None).unwrap();
//!
//!     // Here you may `std::fs::write()` serialized JSON to file
//!
//!     // Serializing back to marshal
//!     // `dump()` requires owned UuidValue as argument
//!     let serialized_to_marshal: Vec<u8> = dump(serialized_to_json, None);
//!
//!     // Here you may `std::fs::write()` serialized Marshal data to file
//! }
//! ```
//!
//! ## MSRV
//!
//! Minimum supported Rust version is 1.63.0.
//!
//! ## References
//!
//! -   [marshal.c](https://github.com/ruby/ruby/blob/master/marshal.c) (Had to compile Ruby manually, and add debug prints to marshal.c, to even figure out what's going on)
//! -   [TypeScript implementation of Marshal](https://github.com/hyrious/marshal) (This project inspired me to start working on this)
//! -   [Official documentation for Marshal format](https://docs.ruby-lang.org/en/master/marshal_rdoc.html) (Mostly useless)
//!
//! ## License
//!
//! Project is licensed under WTFPL.
//!

pub mod constants;
pub mod dump;
pub mod load;
pub mod types;

// Convenient re-exports
pub use dump::{dump, Dumper};
pub use load::{load, load_binary, load_utf8, LoadError, Loader, StringMode};
pub use types::UuidValue;
