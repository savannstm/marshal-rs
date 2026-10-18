# marshal-rs

**`marshal-rs` is a BLAZINGLY 🦀🦀 FAST 🔥🔥 `no_std`-capable Rust implementation of Ruby-lang's `Marshal` binary format.**

Loaded values live in a flat `Arena` of 16-byte `Copy` nodes addressed by `u32` handles: object links resolve as index copies, cycles need no `Rc`/`RefCell`, and the tokenizer allocates nothing, so it runs on freestanding targets. If you're upgrading from v3, see [Upgrading from v3](#upgrading-from-v3).

Ports:

- [C API](./capi/) - C bindings, installable via `cargo-c`.
- [WASM](./wasm/) - WebAssembly bindings.

## Installation

```bash
cargo add marshal-rs
```

## Feature tiers

| Feature  | Pulls in | Gives you                                                                                                                                                                                                                                                    |
| -------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| _(none)_ | -        | `wire`/`reader`/`writer`: a `no_std`, allocation-free tokenizer and token writer over `&[u8]` / a fixed `&mut [u8]`. Genuinely freestanding - usable from embedded Rust or through `marshal-rs-capi` in a C/C++ project with no heap at the tokenizer level. |
| `alloc`  | `alloc`  | `Arena`, `ValueRef`, `load`/`dump` - the DOM most users want.                                                                                                                                                                                                |
| `std`    | `alloc`  | `std::io`-backed I/O.                                                                                                                                                                                                                                        |
| `serde`  | `alloc`  | Streaming `Serialize`/`Deserialize` for `Arena` (JSON or any other serde format - see below).                                                                                                                                                                |

`std` and `serde` are enabled by default. Disable them for a leaner build:

```toml
marshal-rs = { version = "4", default-features = false, features = ["alloc"] }
```

For genuinely freestanding use (no allocator at all), depend with `default-features = false` and use only `marshal_rs::{wire, reader, writer}`.

## The `Arena` model

`load` returns an `Arena<'a>` that borrows strings, bignum digits and float text from the input buffer (`Cow<'a, [u8]>` under the hood), copying only what must outlive it. Call `.into_owned()` to detach it.

Values are read through `ValueRef`, a cheap `Copy` cursor:

```rust
use marshal_rs::{load, ValueRef};

let bytes: &[u8] = /* read from a .rvdata2 file, etc. */
    &[0x04, 0x08, 0x5b, 0x08, 0x69, 0x06, 0x69, 0x07, 0x69, 0x08];

let arena = load(bytes)?;
let root = ValueRef::root(&arena);
for item in root.array() {
    if let Some(n) = item.as_i64() {
        println!("{n}");
    }
}
# Ok::<(), marshal_rs::ReadError>(())
```

(`Arena` doesn't implement `core::ops::Index` - a `ValueRef` is constructed fresh on every call, so there's no stored value to hand back a `&Output` to. Use `ValueRef::at(index)` / `ValueRef::get("@ivar")` / `ValueRef::lookup(key)` instead of `v[i]` / `v["key"]`.)

`dump(&arena)` writes it back to a `Vec<u8>` Marshal byte stream. It is infallible: the builder refuses ids that aren't nodes of the arena, so there is nothing to fail on. An arena nothing was pushed into dumps as `nil`.

`Hash` lookups by key ([`ValueRef::lookup`], `lookup_symbol`) scan the entries. To look up many keys in one big hash, build an index once with `ValueRef::symbol_index()` (symbol-keyed entries by name).

### Instance variables and wrappers

A string's or regexp's `E`/`encoding` instance variable becomes its declared encoding (below). Any other instance variable a value carried - on a string, or on an array, hash, object, ... - is kept and written back by `dump`; read them with `ValueRef::extra_ivars()`, add one with `Arena::push_extra_ivar`. A symbol keeps its own encoding too: Ruby wraps a non-ASCII symbol (a class name, a member name, a symbol value) in `TYPE_IVAR` with `E`/`encoding`. That tag is kept and written back as it was: UTF-8, `Shift_JIS`, ..., or none (as Ruby 1.8 streams and binary symbols have it; see [String encoding](#string-encoding)). `nil`, booleans, fixnums and symbols cannot carry instance variables or be `extend`ed (they are shared nodes), and the loader rejects such a marker.

`extend`ed modules (`ValueRef::extensions()`) come in stream order, which matters: Ruby extends from the last listed module to the first. `Arena::push_extension` takes stream order too, so `obj.extend(A).extend(B)` is `push_extension(B)` then `push_extension(A)`. An `Object`'s instance variables are its members, so `push_extra_ivar` on one adds a member (only strings, arrays, structs and the like get a `TYPE_IVAR` wrapper). A non-ASCII class or module path built with `push_class` / `push_module` gets a UTF-8 tag, as Ruby writes it, when its bytes are UTF-8.

Object numbering follows Ruby's, which `Marshal` links depend on: a value is numbered before what it holds, except a `_dump` string (`u`; after its class name and instance variables, so a `Time` after its zone) and a `marshal_dump` / `_dump_data` wrapper (after its class name). An encoding name a symbol carries is an object too, numbered at the point that follows from that.

A `marshal_dump` / `_dump_data` result (`U` / `d`) is a _wrapper_ node holding only the wrapper's class, whose _body_ is a node of its own - with a class, `extend`s and instance variables that belong to it, not to the wrapper. Reading through a `ValueRef` sees through the wrapper to the body (`kind`, `array`, `as_str`, ... answer for the body), while `class_name`, `extensions` and `extra_ivars` describe the wrapper itself; `ValueRef::body()` steps to the body, `is_data()` / `is_user_marshal()` tell the two wrappers apart. Build one with `Arena::push_user_marshal` / `push_data`.

A subclass of a built-in type (`C`, `ValueRef::is_user_class()`) is only valid on a string, regexp, array or hash, and a `_dump` string (`u`, `is_user_defined()`) only on bytes or a string; the loader rejects the marker on anything else, and `Arena::set_user_class` / `set_user_defined` refuse it.

### In-place mutation

An `Arena` can be mutated after loading. Two categories of methods apply:

- **Slot setters** (`set_array_*`/`set_hash_*`/`set_member_*`) repoint a container slot to a freshly-pushed value, leaving the old value itself untouched (now unreferenced but harmless garbage - the arena never reclaims).
- **Content setters** (`set_*_content`/`set_str_content`/`set_string_text`/`set_str_with_encoding`/`set_fixnum_value`) overwrite a node's payload in place while keeping its `ValueId`. Every reference to that node - reachable from more than one container via Marshal's `@n` back-reference - sees the new content at once, exactly as though the object were mutated in Ruby.

Every setter, and every `push_*` that takes ids, checks its input: a wrong kind, an index outside the container, or an id that isn't a node of the arena returns `None` (or `false`) and changes nothing. So do values Ruby could not read back (an unknown encoding id, an empty class name). A float's text is stored as it is, unchecked, since Ruby before 1.9 writes text that is not a plain number. There are no raw flag or class setters; `set_user_class`, `set_user_defined`, `push_user_marshal`, `push_data`, `push_extension` and `push_extra_ivar` are the checked ways to give a node those. A new `Arena::builder()` holds a `nil` root, so dumping it before `set_root` writes `nil`. Editing a wrapper's id edits its body.

The distinction matters for shared values: `dump` records identity by `ValueId`, so a content setter updates every reference at once, while a slot setter updates only the container it was called on. There is no content setter for `Symbol`: rewriting an interned symbol's bytes would silently change every instance of it, including ivars and class names that share it.

## String encoding

This crate never transcodes or validates text: a loaded string's bytes are exactly what was on the wire. A string is `Kind::Str` (declared encoding) if it had an `E` or `encoding` ivar at load time, and `Kind::Bytes` (implicitly `ASCII-8BIT`) otherwise.

The declared encoding is exposed as a compact id (`ValueRef::encoding_id`) and its name (`ValueRef::encoding_name`), from a fixed table of Ruby's ~100 named encodings (`marshal_rs::encoding`). A name outside the table, such as a future Ruby encoding or one from a native extension, still round-trips byte-exact via `ENCODING_CUSTOM` and a side table holding the original name. The table only makes common names cheap; correctness does not depend on it. `Kind::Regexp` carries the same tag.

Converting bytes to a Rust string type is up to you (`encoding_rs`, ICU, ...). `ValueRef::as_str` succeeds for a string only if it is declared UTF-8 or US-ASCII _and_ its bytes validate; a symbol gives its name when that is valid UTF-8. A UTF-8-tagged string can still have invalid bytes (Ruby's `valid_encoding?` is `false` there too; RPG Maker's zlib-compressed script data is tagged this way). `ValueRef::as_bytes` works for every string and `bytes` value, whatever its encoding.

Every declared encoding round-trips byte-for-byte on dump, not just UTF-8/ASCII, since nothing is re-encoded. The `E`/`encoding` ivar (or its absence, for `ASCII-8BIT`) is rebuilt from the stored id or name. Ruby writes each encoding name once per stream and links to it afterwards, and `dump` does the same.

**Symbols** have an encoding too. `ValueRef::encoding_id`/`encoding_name` answer as Ruby would (US-ASCII for an untagged ASCII name, ASCII-8BIT for any other untagged one), and `ValueRef::symbol_tag` gives what is stored. The same bytes under two encodings are two symbols, as in Ruby. Class, module, member and ivar names are symbols too; the builder tags them with [`default_symbol_encoding`](crate::encoding::default_symbol_encoding) (UTF-8 for non-ASCII UTF-8, none otherwise). That is the only place bytes are checked to choose a tag, and it never rejects anything.

**Changing an encoding** never touches the bytes - a tag is a claim about them, as in Ruby's `force_encoding`, and is not checked:

- `Arena::set_encoding(id, encoding_id)` / `set_encoding_name(id, name)` re-tag a string, regexp or symbol (a `Kind::Bytes` string becomes `Kind::Str` when given an encoding, and the other way round for ASCII-8BIT).
- `Arena::set_string_text(id, String)` replaces the text and tags it **UTF-8**, since a Rust `String` is not in whatever encoding the old text had; `set_str_content(id, bytes)` replaces the bytes and keeps the tag; `set_str_with_encoding(id, bytes, encoding_id)` does both.
- `push_symbol_with_encoding` / `push_symbol_with_encoding_name` build a tagged symbol; `push_symbol` picks the default tag.

In the serde envelope a `symbol` carries `__encoding` only when its tag differs from the one its bytes would get anyway. A name (`__class`, `__extensions`, member and ivar names) is a plain string, or `{"__base64": ...}` when it is not UTF-8. A name whose tag differs from the default (which Ruby never writes, but reads) is an object: `{"__name": "Plain", "__encoding": "Shift_JIS"}` or `{"__base64": "...", "__encoding": "..."}`.

## `serde`

With the `serde` feature, `Arena` implements `Serialize`/`Deserialize` directly, with no intermediate DOM, for any serde format, not just JSON. `nil`, booleans and fixnums serialize as bare JSON primitives; everything else becomes an envelope object:

```json
{
  "__type": "array",
  "__class": "MyArray",
  "__flags": ["user_class"],
  "__value": [1, 2, 3]
}
```

A `Hash`'s `__value` is a JSON array of `[key, value]` pairs, not an object, since Ruby hash keys aren't always strings. Keys may come in any order; an envelope whose `__type` is not first is buffered before it is read, which costs extra time and memory. See `src/ser.rs` for the full envelope reference. Object links and cycles are not preserved across a JSON round-trip: shared or self-referential structure becomes independent copies.

To write binary payloads in the serializer's own byte type instead (a `Uint8Array` with `serde-wasm-bindgen`, a byte string in CBOR or `MessagePack`), serialize `marshal_rs::ser::Envelope { arena: &arena, binary: BinaryMode::Bytes }` instead of the arena itself; reading accepts both forms.

A `marshal_dump` / `_dump_data` wrapper is `{"__type": "user_marshal" | "data", "__class": "Foo", "__value": <the body's envelope>}`. Instance variables other than the encoding are listed in `__ivars` as `[name, value]` pairs, `extend`s in `__extensions`; unknown `__flags` are an error. Float text, class and module paths and class names are checked: a document that would dump to a stream Ruby can't read is rejected when it is read.

A `Str`/`Regexp` with a non-default declared encoding carries an extra `__encoding` field naming it (e.g. `"Shift_JIS"`); a `Str`'s `__value` is plain text when its bytes happen to validate as UTF-8 (the common case, and far more readable), or a base64 string with `"__base64": true` otherwise (raw bytes under `BinaryMode::Bytes`). With the default `BinaryMode::Base64`, a `bytes` value's `__value` and a `regexp`'s source are base64 (standard alphabet, padded). Deserializing also accepts the older JSON byte arrays, and raw byte buffers from formats that have them. The encoding always round-trips byte-exact regardless of which shape was used.

Nothing is validated or replaced. A `float`, `symbol`, `class` or `module` `__value` follows the `str` rule, and a non-UTF-8 name (`__class`, `__extensions`, `__encoding`, member and ivar names) is `{"__base64": "..."}`. Floats from Ruby before 1.9, with mantissa bytes after a NUL, therefore round-trip exactly. An envelope nested deeper than `marshal_rs::ser::MAX_NESTING` (256) is an error to read or write.

## Upgrading from v3

- Mutators and `push_*` that take ids now return `Option` (`set_root` returns `bool`) instead of panicking on a wrong kind or an out-of-range index/id: `set_array_*`, `set_hash_*`, `set_bytes_content`, `set_string_text`, `set_fixnum_value`, `take_*`, `push_array`, `push_hash`, `push_struct`, `push_object`.
- JSON/serde envelope: binary payloads are base64 strings (was: byte arrays); non-UTF-8 names are `{"__base64": "..."}` (was: replaced with U+FFFD); a `struct`/`object` without `__class` is an error.
- `load` rejects bytes after the value (`ReadError::TrailingData`); use `load_many` for a file of several streams.
- `U` / `d` wrappers are no longer merged into their body: `class_name()` is the wrapper's, the body's own class is `body().class_name()`. In JSON they are `"__type": "user_marshal" | "data"` with the body under `__value`, and `"data"` / `"user_marshal"` are no longer `__flags`. `__ivars` is new.
- `extensions()` returns modules in stream order (it used to return them reversed for a value with several).
- `Arena::set_class`, `clear_class`, `add_flags`, `remove_flags` and `set_flags` are gone (see the checked methods above); `Node` is no longer public, `Arena::node` neither; `push_str_with_encoding`, `push_class`, `push_module` and `push_extension` return `Option`/`bool`; `Arena::builder()` is no longer `const`.
- The loader rejects `C` on a value that is not a plain built-in, and `e` or ivars on `nil`/booleans/fixnums/symbols; both used to load and then dump to a stream Ruby rejects.
- Inputs over `i32::MAX` bytes, and containers claiming more elements than the input has bytes left, are rejected with `ReadError::LengthOverflow`. A negative length or count is rejected with `ReadError::NegativeLength`.
- `ValueRef::as_str` no longer succeeds for strings declared in a non-UTF-8, non-ASCII encoding.
- A symbol's encoding is kept (it used to be dropped on load, and re-derived on dump from whether the bytes were UTF-8). `Token::Symbol` now also stands for a symbol wrapped in its encoding - there is no `Token::BeginIvar` before it - and `Reader::last_symbol_encoding` says which [`SymbolEncoding`] it declared.
- `Arena::set_string_text` now tags the string UTF-8 (it used to keep the tag it had); `set_str_content` is the old behaviour.
- `Reader` gained `remaining`, `last_symbol` and `next_symbol`; the C API gained `mrs_load_ex`, `mrs_load_many` and more (see `capi/README.md`).

## Benchmarks

- `cargo bench --bench load_dump`: load/dump throughput.
- `cargo bench --bench marshal_c_compare`: the same, plus Ruby's stock `Marshal` (`marshal.c`) via a `ruby` on `PATH`.

## References

- [marshal.c](https://github.com/ruby/ruby/blob/master/marshal.c) - the authoritative reference for every wire-format detail in this crate.
- [TypeScript implementation of Marshal](https://github.com/hyrious/marshal) (the original inspiration for this project).
- [Official documentation for Marshal format](https://docs.ruby-lang.org/en/master/marshal_rdoc.html)

## Support

[Me](https://github.com/savannstm), the maintainer of this project, is a poor college student from Eastern Europe.

If you could, please consider supporting us through:

- [Boosty](https://boosty.to/mcdeimos) (has lower fees)
- [Ko-fi](https://ko-fi.com/savannstm)

Even if you don't, we will still continue working on the project anyways.

## License

Licensed under [WTFPL](https://www.wtfpl.net/).
