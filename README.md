# marshal-rs

**`marshal-rs` is a BLAZINGLY :crab::crab: FAST:fire::fire: `no_std`-capable Rust implementation of Ruby-lang's `Marshal` binary format.**

v3 is a from-scratch rewrite of the crate. The old `Value` tree (an `Rc<SafeCell<Value>>` graph, deep-cloned on every load) is gone; in its place is a flat `Arena` of 16-byte `Copy` nodes addressed by `u32` handles, so object links resolve as an index copy instead of a subtree clone, cycles are representable without `Rc`/`RefCell`, and the core tokenizer works with zero allocation on a genuinely freestanding target. See [CHANGELOG-like notes below](#coming-from-v2) if you're upgrading.

This crate has some ports:

- [C API](./crates/rpgmasd-capi/) - C bindings, installable via `cargo-c`.
- [WASM](./crates/rpgmasd-wasm/) - WASM bindings generated from Rust code.

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

A C-callable surface over the `Arena` API is a separate workspace crate, `marshal-rs-capi`, not a feature of this one - see [`no_std` / FFI](#no_std--ffi) below.

`std` and `serde` are enabled by default. Disable them for a leaner build:

```toml
marshal-rs = { version = "3", default-features = false, features = ["alloc"] }
```

For genuinely freestanding use (no allocator at all), depend with `default-features = false` and use only `marshal_rs::{wire, reader, writer}`.

## The `Arena` model

`load` returns an `Arena<'a>` that borrows strings, bignum digits, and float text directly out of the input buffer (`Cow<'a, [u8]>` under the hood) - no copy on load beyond what genuinely needs to leave the buffer's lifetime. Call `.into_owned()` to detach it.

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

`dump(&arena)` writes it back to a `Vec<u8>` Marshal byte stream - infallible: every `Arena` reachable through the public API is internally consistent by construction, so there's nothing for it to fail on.

## String encoding

This crate never transcodes or validates text content - a loaded string's bytes are exactly what was on the wire, in whatever encoding they were declared in. A string is `Kind::Str` (carries a declared encoding) if it had Ruby's `E` or `encoding` instance variable at load time, and `Kind::Bytes` (implicitly `ASCII-8BIT`/binary, no ivar was ever written for it) otherwise.

The declared encoding is exposed as a compact id (`ValueRef::encoding_id`) plus its name (`ValueRef::encoding_name`), backed by a fixed table of Ruby's ~100 named encodings (`marshal_rs::encoding`). A name outside the table - a future Ruby encoding, or a custom one from a native extension - still round-trips byte-exact via `ENCODING_CUSTOM` and a side table recording the exact original name, so the table only needs updating to make a newly-common name cheap, not for correctness. `Kind::Regexp` carries the same tag, since Ruby wraps a Regexp's source in the identical `E`/`encoding` ivar mechanism.

Converting the bytes to a particular Rust string type is left to you: pick whatever text stack fits your embedding (`encoding_rs`, ICU, ...). `ValueRef::as_str` is a convenience that succeeds only if the bytes happen to validate as UTF-8, independent of what was actually declared (a `String` can be tagged UTF-8 while its bytes don't validate - `valid_encoding?` can be `false` in Ruby too; RPG Maker's own zlib-compressed script data is tagged this way) - `ValueRef::as_bytes` always works.

Every declared encoding round-trips byte-for-byte on dump - not just UTF-8/ASCII - since nothing is ever re-encoded; the original `E`/`encoding` ivar (or its absence, for `ASCII-8BIT`) is reconstructed from the stored id/name.

## `serde`

With the `serde` feature, `Arena` implements `Serialize`/`Deserialize` directly against the `Serializer`/`Deserializer` traits - no intermediate DOM, and it works with any serde data format, not just JSON. `Nil`/bools/ fixnums serialize as bare JSON primitives; everything else becomes an envelope object:

```json
{
  "__type": "array",
  "__class": "MyArray",
  "__flags": ["user_class"],
  "__value": [1, 2, 3]
}
```

A `Hash`'s `__value` is a JSON array of `[key, value]` pairs, not a JSON object - Ruby hash keys aren't always strings. `__type` must be the object's first key (this is `marshal-rs`'s own wire format, not general JSON); every other key may appear in any order. See `src/ser.rs` for the full envelope reference. Object links/cycles are not preserved across a JSON round-trip - shared or self-referential structure is flattened into independent copies.

A `Str`/`Regexp` with a non-default declared encoding carries an extra `__encoding` field naming it (e.g. `"Shift_JIS"`); a `Str`'s `__value` is plain text when its bytes happen to validate as UTF-8 (the common case, and far more readable), or a JSON byte array otherwise - deserializing accepts either shape, and the encoding always round-trips byte-exact regardless of which shape was used.

## Coming from v2

- `Loader`/`Dumper` structs and `load_utf8`/`load_binary` are gone; use `load`. There is no `StringMode`/`LoadOptions` anymore - this crate never transcodes or validates string content at all now (see [String encoding](#string-encoding)), so there is no policy left to select.
- `Value`/`ValueType`/`Object`/`HashMap` are gone; use `Arena`/`ValueRef`.
- `instance_var_prefix` is gone from the load/dump path - ivar names are always the raw `@name` symbol bytes; substitute a prefix yourself from `ValueRef::members()` if you need one (`name.strip_prefix(b"@")`).
- The JSON envelope changed shape (`__type` is now a string tag, `__id` is gone - arena indices replace it, hash keys are `[key, value]` pairs instead of JSON-string-encoded keys, and a `Str`/`Regexp` may carry an `__encoding` field). Old dumped JSON will not deserialize with v3.
- Dumping now round-trips **every** declared encoding byte-exact, not just UTF-8/ASCII - v2 (and early v3) always re-emitted text as UTF-8 regardless of what was originally declared.
- `bitflags`, `encoding_rs`, `gxhash`, `indexmap`, `num-bigint`, `strum_macros`, and the hard `serde_json` dependency are all gone. Bignum <-> decimal conversion is now hand-rolled (`src/bignum.rs`) rather than pulling in arbitrary-precision arithmetic for a handful of calls per file.
- The `.cargo/config.toml` `-C target-feature=+aes,+sse2` requirement is gone with `gxhash` - nothing about building this crate (or a crate that depends on it) requires special target features anymore.

## Benchmarks

`cargo bench --bench load_dump` measures load/dump throughput in isolation. `cargo bench --bench marshal_c_compare` additionally times Ruby's own stock `Marshal` (`marshal.c`) over the same fixture via a real `ruby` interpreter (must be on `PATH`) and prints a side-by-side comparison - see `benches/marshal_c_compare.rs`/`.rb`.

## Known limitations

- Dumping is recursive, not iterative like loading - it only ever walks an already-validated `Arena`, never untrusted bytes directly, so a malicious _input_ can't reach it, but a very deep hand-built or already-loaded graph could still exhaust the stack. Worth revisiting if that stops being true in practice.
- A symbol carrying its own instance variables (`TYPE_IVAR` directly wrapping `TYPE_SYMBOL` - a legacy, essentially never-emitted-by-modern-Ruby construct) is rejected with `ReadError::Unsupported` rather than silently mishandled.

## References

- [marshal.c](https://github.com/ruby/ruby/blob/master/marshal.c) - the authoritative reference for every wire-format detail in this crate.
- [TypeScript implementation of Marshal](https://github.com/hyrious/marshal) (the original inspiration for this project).
- [Official documentation for Marshal format](https://docs.ruby-lang.org/en/master/marshal_rdoc.html)

## Support

[Me](https://github.com/savannstm), the maintainer of this project, is a poor college student from Eastern Europe.

If you could, please consider supporting us through:

- [Ko-fi](https://ko-fi.com/savannstm)
- [Patreon](https://www.patreon.com/cw/savannstm)
- [Boosty](https://boosty.to/mcdeimos)

Even if you don't, it's fine. We'll continue to do as we right now.

## License

Project is licensed under [WTFPL](https://www.wtfpl.net/).
