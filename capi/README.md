# marshal-rs-capi

C bindings for `marshal-rs`.

## Install

```bash
cargo install cargo-c
cargo cinstall --release --prefix=/usr/local
```

Installs `libmarshal_rs` (shared and static), the header as `<marshal-rs/marshal.h>`, and a `pkg-config` file. The header documents every function.

## Usage

```c
#include <marshal-rs/marshal.h>

MrsError error;
MrsArena *arena = mrs_load(bytes, len, &error);
if (!arena) {
    /* `error` says why */
}

MrsValueId root = mrs_root(arena);
if (mrs_kind(arena, root) == MRS_KIND_ARRAY) {
    for (uint32_t i = 0; i < mrs_array_len(arena, root); i++) {
        MrsValueId item = mrs_array_get(arena, root, i);
        /* ... */
    }
}

size_t out_len;
uint8_t *out = mrs_dump(arena, &out_len);   /* Marshal bytes */
/* ... */
mrs_buffer_free(out, out_len);
mrs_arena_free(arena);
```

- Scalar accessors (`mrs_as_bool`, `mrs_as_i64`, `mrs_as_f64`) write through an out-parameter and return whether the value was that type. Strings, symbols, class names and regexps are borrowed byte ranges (`mrs_as_bytes`, `mrs_class_name`, ...), valid until the arena is freed. Arrays, hashes and struct/object members are walked by index (`mrs_array_get`, `mrs_hash_key_at`, `mrs_member_value_at`, ...).
- Buffers from `mrs_dump` and `mrs_as_bignum_decimal` are released with `mrs_buffer_free`.

## `no_std`

Built with `--no-default-features`, the library has no allocator of its own: register one with `mrs_set_allocator` before any other allocating call (every build exports it; with `std` it does nothing).

```c
typedef uint8_t* (*MrsAllocFn)(size_t size, size_t align);
typedef void (*MrsFreeFn)(uint8_t* ptr, size_t size, size_t align);
void mrs_set_allocator(MrsAllocFn alloc, MrsFreeFn free);
```

`alloc` returns at least `size` bytes aligned to `align` (a power of two), or `NULL`; `free` gets back exactly the `(ptr, size, align)` that `alloc` produced. Without a registered allocator every allocation fails and surfaces as a `NULL` return. See [`examples/README.md`](examples/README.md) for a worked example.
