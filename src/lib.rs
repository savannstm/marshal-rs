//! # marshal-rs
//!
//! **`marshal-rs` is a complete Rust implementation of Ruby-lang's `Marshal`.**
//!
//! It is capable of :fire: **_BLAZINGLY FAST_** loading data from dumped Ruby Marshal files, as well as :fire: **_BLAZINGLY FAST_** dumping it back to Marshal format.
//!
//! ## Installation
//!
//! `cargo add marshal-rs`
//!
//! ## Overview
//!
//! This crate has two main structs, `Loader` and `Dumper`, along with helper functions that use them internally. There's three load functions: `load()`, `load_utf8()`, `load_binary()`, and a single dump function: `dump()`.
//!
//! `load()` takes a `&[u8]`, consisting of Marshal data bytes (that can be read using `std::fs::read()`) as its only argument, and outputs `Value`.
//!
//! `dump()`, in turn, takes `marshal_rs::Value` as its only argument and serializes it back to `Vec<u8>` Marshal byte stream. It does not preserve strings' initial encoding, writing all strings as UTF-8 encoded.
//!
//! By default, in `load()` function, Ruby strings, that include encoding instance variable, are serialized to JSON strings, and those which don't, serialized to byte arrays.
//!
//! `load_utf8()` function tries to convert arrays without instance variable to string, and produces string if array is valid UTF-8, and object otherwise.
//!
//! `load_binary()` function converts all strings to objects.
//!
//! This behavior also can be controlled in `Loader` by calling `set_string_mode()`.
//!
//! You can manage the prefix of instance variables using `instance_var_prefix` argument in `load()` and `dump()`, or by using `set_instance_var_prefix()` function in `Loader` or `Dumper`. Passed string replaces "@" instance variables' prefixes.
//!
//! To avoid loss of precision, floats are stored as strings.
//!
//! If Marshal file contains any extra float mantissa bits, `marshal-rs` discards them. They aren't written by latest 4.8 version of Marshal, but it still preserves them, if encounters any. `marshal-rs` does not.
//!
//! The reason this crate wraps around `serde_json::Value`, is because it needs to cleanly track unique object instances and object metadata.
//!
//! The table shows, how `marshal-rs` serializes Ruby types to Value:
//!
//! | Ruby object                                | Serialized to Value                       |
//! | ------------------------------------------ | ----------------------------------------- |
//! | `nil`                                      | `null`                                    |
//! | `true`, `false`                            | `true`, `false`                           |
//! | `1337` (Integer)                           | `1337`                                    |
//! | `36893488147419103232` (Big Integer)       | `"36893488147419103232"`                  |
//! | `13.37` (Float)                            | `"13.37"`                                 |
//! | `"ligma"` (String, with instance variable) | `"ligma"`                                 |
//! | `:ligma` (Symbol)                          | `"ligma"`                                 |
//! | `/lgma/i` (Regex)                          | `"/lgma/i"`                               |
//! | `[ ... ]` (Array)                          | `[ ... ]`                                 |
//! | `Hash`, `Struct`                           | `IndexMap<Value, Value>`                  |
//! | `Object.new`                               | `IndexMap<String, Value>`                 |
//! | `Class`, `Module`                          | `null` (Doesn't dump any data to Marshal) |
//!
//! Value can be stringified and written to JSON using `serde_json::to_string` function. That will wrap each value in an object, that holds its metadata as object keys. For example, `null` will become
//!
//! ```json
//! {
//!     "__id": number,
//!     "__class": "",
//!     "__type": 0,
//!     "__value": null,
//!     "__extensions": [],
//!     "__flags": 0
//! }
//! ```
//!
//! object.
//!
//! Possible `__type` values are defined in `src/types.rs`:
//!
//! ```compile_fail
//! pub enum ValueType {
//!     #[default]
//!     Null = 0,
//!     Bool(bool) = 1,
//!     Integer(i32) = 2,
//!     Float(String) = 3,
//!     Bigint(String) = 4,
//!     String(String) = 5,
//!     Bytes(Vec<u8>) = 6,
//!     Symbol(String) = 7,
//!     Regexp(String) = 8,
//!     Array(Vec<Value>) = 9,
//!     Object(ObjectMap) = 10,
//!     Class = 11,
//!     Module = 12,
//!     HashMap(HashMap) = 13,
//!     Struct(HashMap) = 14,
//! }
//! ```
//!
//! Possible `__flags` values are defined in `src/types.rs`:
//!
//! ```compile_fail
//! struct ValueFlags: u8 {
//!     const None = 0;
//!     const OldModule = 1;
//!     const UserClass = 2;
//!     const Data = 4;
//!     const UserDefined = 8;
//!     const UserMarshal = 16;
//! }
//! ```
//!
//! Keep in mind `__flags` also could be a combination of some flags.
//!
//! ### Unsafe code
//!
//! In this crate, unsafe code provides the ability to replicate Marshal's behavior. It shouldn't ever cause problems.
//!
//! ## Test coverage
//!
//! Currently, tests feature dumping/loading the following values: nil, bool, positive/negative fixnum, positive/negative bignum, float (including inf, nan and negative), utf-8/non-utf-8 strings, object links, array, hashes, structs, objects (including extended with modules, with custom marshal\_ methods, with custom \_load/\_dump methods), regexps, built-in class subclasses.
//!
//! Also tests include loading/dumping RPG Maker game's files and battle-testing them.
//!
//! If something is missing in the tests, open an issue or submit a pull request.
//!
//! ## Example
//!
//! ```rust
//! use std::fs::read;
//! use marshal_rs::{load, dump, Value};
//!
//! fn main() {
//!     // Read marshal data from file
//!     // let marshal_data: Vec<u8> = read("./Map001.rvdata2").unwrap();
//!     // For this example, we'll just take pre-defined marshal data
//!     let marshal_data = [0x04, 0x08, 0x30];
//!
//!     // Serializing to json
//!     // `load()` takes a `&[u8]` as argument, so `Vec<u8>` must be borrowed
//!     let serialized_to_json: Value = load(&marshal_data, None).unwrap();
//!
//!     // Here you may stringify Value using `serde_json::to_string()`, and
//!     // `std::fs::write()` it to file
//!
//!     // Serializing back to marshal
//!     // `dump()` requires owned Value as argument
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

mod constants;
pub mod dump;
pub mod load;
pub mod types;

pub use dump::{dump, Dumper};
pub use load::{load, load_binary, load_utf8, LoadError, Loader, StringMode};
pub use types::{HashMap, Object, Value, ValueType};

thread_local! {
    pub(crate) static VALUE_INSTANCE_COUNTER: types::SafeCell<usize> = const { types::SafeCell::new(0) };
}
