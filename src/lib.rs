//!**`marshal-rs` is a Rust implementation of Ruby-lang's `Marshal`.**
//!
//! This project is essentially just [@savannstm/marshal](https://github.com/savannstm/marshal), rewritten using Rust.
//! It is capable of **_BLAZINGLY FAST_** loading dumped from Ruby Marshal files, as well as **_BLAZINGLY FAST_** dumping them back to Marshal format.
//!
//! # Quick overview
//!
//! This crate has two main functions: `load()` and `dump()`.
//!
//! `load()` takes a `&[u8]`, consisting of Marshal data bytes (that can be read using std::fs::read()) as its only argument, and outputs serde_json::Value (sonic_rs::Value, if "sonic" feature is enabled).
//!
//! `dump()` takes a `Value` as its only argument, and outputs Vec\<u8\>, consisting of Marshal bytes.
//!
//! If serializes Ruby data to JSON using the table:
//!
//!| Ruby object                                    | Serialized to JSON                                                        |
//!| ---------------------------------------------- | ------------------------------------------------------------------------- |
//!| `nil`                                          | `null`                                                                    |
//!| `1337` (Integer)                               | `1337`                                                                    |
//!| `36893488147419103232` (Big Integer)           | `{ __type: "bigint", value: "36893488147419103232" }` (Plain object)      |
//!| `13.37` (Float)                                | `13.37`                                                                   |
//!| `"ligma"` (String)                             | `"ligma"`                                                                 |
//!| `:ligma` (Symbol)                              | `"__symbol__ligma"`                                                       |
//!| `/lgma/i` (Regex)                              | `{ "__type": "regexp", "expression": "lgma", flags: "i" }` (Plain object) |
//!| `[]` (Array)                                   | `[]`                                                                      |
//!| `{}` (Hash)                                    | `{}` (Plain object)                                                       |
//!| `Object.new` (Including structs, modules etc.) | `{ "__class": "__symbol__Object", "__type": "object" }` (Plain object)    |
//!
//! ## Strings
//!
//! By default, Ruby strings, that include encoding instance variable, are serialized to JSON strings, and those which don't, serialized to `{ __type: "bytes", data: [...] }` objects.
//!
//! This behavior can be controlled with `string_mode` argument of load() function.
//!
//! `StringMode::UTF8` tries to convert arrays without instance variable to string, and produces string if array is valid UTF8, and object otherwise.
//!
//! `StringMode::Binary` converts all strings to objects.
//!
//!## Objects and Symbols
//!
//! For objects, that cannot be serialized in JSON (such as Objects and Symbols), `marshal-rs` uses approach of stringifying and adding prefixes and properties. It stringifyies symbols and prefixes them with `__symbol__`, and serializes objects' classes and types as `__class` keys and `__type` keys respectively.
//!
//! ## Hash keys
//!
//! For Hash keys, that in Ruby may be represented using Integer, Float, Object etc, `marshal-rs` tries to preserve key type with prefixing stringifiyed key with it type. For example, Ruby `{1 => nil}` Hash will be converted to `{"__integer__1": null}` object.
//!
//! load(), in turn, takes serialized JSON object and serializes it back to Ruby Marshal format. It does not preserve strings' initial encoding, writing all strings as UTF-8 encoded, as well as does not writes links, which effectively means that output Marshal data might be larger in size than initial.
//!
//! ## Instance variables
//!
//! Instance variables always decoded as strings with "\_\_symbol\_\_" prefix.
//! You can manage the prefix of instance variables using `instance_var_prefix` argument in load() and dump(). Passed string replaces "@" instance variables' prefixes.
//!
//! # Quick example
//!
//! ```rust
//! use std::fs::read;
//! use marshal_rs::load::load;
//! use marshal_rs::dump::dump;
//!
//! fn main() {
//!    // Read marshal data
//!    let marshal_data: Vec<u8> = read("./marshal_file.marshal").unwrap();
//!
//!    // Serializing to json
//!    // load() takes a &[u8] as argument, so bytes Vec must be borrowed
//!    let serialized_to_json: serde_json::Value = load(&marshal_data, None, None);
//!
//!    // Here you may std::fs::write() serialized JSON to file
//!
//!    // Serializing back to marshal
//!    // dump() requires owned Value as argument
//!    let serialized_to_marshal: Vec<u8> = dump(serialized_to_json, None);
//!
//!    // Here you may std::fs::write() serialized Marshal data to file
//! }
//! ```
//!
//! # MSRV
//!
//! Minimum supported Rust version is 1.63.0.

#[allow(dead_code)]
#[repr(u8)]
#[derive(PartialEq, Clone, Copy)]
enum Constants {
    True = 84,         // 'T'
    False = 70,        // 'F'
    Nil = 48,          // '0'
    Fixnum = 105,      // 'i'
    Symbol = 58,       // ':'
    Symlink = 59,      // ';'
    Link = 64,         // '@'
    InstanceVar = 73,  // 'I'
    Extended = 101,    // 'e'
    Array = 91,        // '['
    Bignum = 108,      // 'l'
    Class = 99,        // 'c'
    Module = 109,      // 'm'
    ModuleOld = 77,    // 'M'
    Data = 100,        // 'd'
    Float = 102,       // 'f'
    Hash = 123,        // '{'
    HashDefault = 125, // '}'
    Object = 111,      // 'o'
    Regexp = 47,       // '/'
    String = 34,       // '"'
    Struct = 83,       // 'S'
    UserClass = 67,    // 'C'
    UserDefined = 117, // 'u'
    UserMarshal = 85,  // 'U'
    Positive = 43,     // '+'
    Negative = 45,     // '-'

    // Regular expression flags
    RegexpIgnore = 1,
    RegexpMultiline = 4,
    RegexpBoth = 5,
    RegexpNone = 0,
}

impl std::ops::BitAnd<Constants> for u8 {
    type Output = u8;

    fn bitand(self, rhs: Constants) -> Self::Output {
        self & (rhs as u8)
    }
}

impl PartialEq<Constants> for u8 {
    fn eq(&self, other: &Constants) -> bool {
        *self == *other as u8
    }
}

const ENCODING_SHORT_SYMBOL: &str = "__symbol__E";
const ENCODING_LONG_SYMBOL: &str = "__symbol__encoding";
const EXTENDS_SYMBOL: &str = "__ruby_extends__";
const DEFAULT_SYMBOL: &str = "__ruby_default__";
const MARSHAL_VERSION: u16 = 0x0408;

pub mod dump;
pub mod load;
