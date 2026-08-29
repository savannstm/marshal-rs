# marshal-rs-capi

C API for [`marshal-rs`](../..): a C-callable surface over its `Arena`/`ValueRef` API, for embedding the crate in C/C++ applications. Installable via [`cargo-c`](https://github.com/lu-zero/cargo-c). Produces a shared/static library (`libmarshal_rs`), a C header (`marshal.h`), and a `pkg-config` file.

## Building / installing

```bash
cd crates/marshal-rs-capi
cargo install cargo-c # or cargo binstall cargo-c
cargo cbuild --release
cargo cinstall --release --prefix=/usr/local
```

## API reference

- `mrs_load` parses a Marshal byte buffer into an owned `MrsArena` handle, returning `NULL` and writing `*out_error` (an `MrsError`, mirroring a payload-free subset of `marshal_rs::ReadError`) on failure. `mrs_arena_free` releases the handle.
- `mrs_dump` serializes an arena back to Marshal bytes into a freshly heap-allocated buffer; release it with `mrs_buffer_free`.
- `mrs_root`/`mrs_kind` get the arena's root value id and a value's `Kind`.
- Scalar accessors - `mrs_as_bool`/`mrs_as_i64`/`mrs_as_f64` - write through an out-parameter and return whether the value was actually that type.
- `mrs_as_bytes` borrows a `Str`/`Bytes`/`Symbol` value's raw bytes; `mrs_class_name` borrows a value's declared class/module name; `mrs_as_path` borrows a `Class`/`Module` value's own raw path (distinct from `mrs_class_name`, which is the declared class _of_ a value). `mrs_as_regexp` borrows a `Regexp`'s source pattern plus its option bits (`1`=ignorecase, `2`=extended, `4`=multiline, combinable).
- `mrs_encoding_id`/`mrs_encoding_name` read a `Str`/`Bytes`/`Regexp` value's declared text encoding - this crate never transcodes, so these surface exactly what was on the wire. `0` (`MRS_ENCODING_ASCII_8BIT`) is Ruby's implicit default; `255` (`MRS_ENCODING_CUSTOM`) means the name isn't in the fixed table and must be resolved via `mrs_encoding_name` instead of hardcoded against the id.
- `mrs_as_bignum_decimal` renders a `Bignum` as a decimal string in a freshly heap-allocated buffer - release it with `mrs_buffer_free`, exactly like `mrs_dump`'s result.
- Collection accessors - `mrs_array_len`/`mrs_array_get`, `mrs_hash_len`/`mrs_hash_key_at`/`mrs_hash_value_at`, `mrs_members_len`/`mrs_member_name_at`/`mrs_member_value_at` - walk an `Array`, `Hash`, or a `Struct`/`Object`'s ivars by index.

All borrowed pointer/length pairs are valid exactly as long as the `MrsArena` they came from is not freed.

## `no_std`

Disable this crate's default `std` feature for a genuinely `no_std` build:

```bash
cargo cbuild --release --no-default-features
```

With `std` off there is no other Rust code around to supply a global allocator or panic handler, so the crate provides both itself, in `host_alloc` (`src/lib.rs`):

- A minimal abort-on-panic handler.
- A global allocator backed by two function-pointer hooks the host registers once via `mrs_set_allocator`, **before any other allocating `mrs_*` call** - never a hardcoded `malloc`/`free`, since a freestanding target may not have libc at all:

  ```c
  typedef uint8_t* (*MrsAllocFn)(size_t size, size_t align);
  typedef void (*MrsFreeFn)(uint8_t* ptr, size_t size, size_t align);
  void mrs_set_allocator(MrsAllocFn alloc, MrsFreeFn free);
  ```

  `alloc` returns a pointer to at least `size` bytes aligned to `align` (a power of two), or `NULL` on failure; `free` gets back exactly the `(ptr, size, align)` triple a prior `alloc` call produced. Skipping this call (or calling any other allocating function first) makes every allocation fail, surfacing as `NULL`/out-of-memory-shaped returns - it does not crash. `mrs_set_allocator` only exists in a `no_std` build; the default `std`-linked build uses Rust's own allocator and does not declare or need it.

See [`examples/dump_to_json.c`](examples/README.md) for a worked C11 example that registers `malloc`/`free`-backed hooks against a `no_std` build.
