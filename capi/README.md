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

- Scalar accessors (`mrs_as_bool`, `mrs_as_i64`, `mrs_as_f64`) write through an out-parameter and return whether the value was that type. Strings and symbols are borrowed byte ranges (`mrs_as_bytes`), as are class names (`mrs_class_name`), class and module paths (`mrs_as_path`) and regexp sources (`mrs_as_regexp`), all valid until the arena is freed. Arrays, hashes and struct/object members are walked by index (`mrs_array_get`, `mrs_hash_key_at`, `mrs_member_value_at`, ...).
- Buffers from `mrs_dump` and `mrs_as_bignum_decimal` are released with `mrs_buffer_free`.
- Out-parameters may be `NULL` (the function then only answers whether the value is of that kind), and a byte buffer may be `NULL` when its length is `0`. A length above `i32::MAX` is refused (Marshal cannot store it): the builder functions return `MRS_VALUE_ID_NONE` / `false`.
- Pointers handed out (`mrs_as_bytes`, `mrs_class_name`, ...) stay valid until `mrs_arena_free`, also across later `mrs_push_*` / `mrs_set_*` calls. An arena may be read from several threads at once as long as none of them changes it.
- `mrs_load` copies the input once and the arena borrows its strings from that copy, so the buffer you pass can be freed right after the call.
- Running out of memory aborts the process; no function reports it.
- `mrs_version()` returns the library's version string, to check against the header at run time.
- `mrs_load_ex` also reports the byte offset of the error (`MrsLoadError`). `mrs_load_many` loads a file made of several concatenated streams, such as an RPG Maker save: take each arena out of the returned list with `mrs_arena_list_take`, then release the list with `mrs_arena_list_free`.
- `mrs_body` steps from a `marshal_dump` / `_dump_data` wrapper to its body; every other function already reads through a wrapper (`mrs_kind`, `mrs_array_get`, ...), while `mrs_class_name`, `mrs_extension_*` and `mrs_extra_ivar_*` describe the wrapper itself.
- `mrs_arena_new` and the `mrs_push_*` / `mrs_set_*` functions build and edit an arena; each refuses what does not fit (a bad id, a wrong kind, invalid UTF-8) with `MRS_VALUE_ID_NONE` / `false` and changes nothing. Dump the result with `mrs_dump`.
- `mrs_as_float_text` gives a float's text as Ruby wrote it, and `mrs_load` reports bytes after the value as `MRS_ERROR_TRAILING_DATA`.
- `mrs_flags` says how a value was wrapped (`MRS_FLAG_*`), `mrs_extension_*` and `mrs_extra_ivar_*` list the modules it was `extend`ed with and the instance variables it carried besides its encoding, `mrs_hash_get_symbol` and `mrs_member_get` look an entry up by name, and `mrs_as_bignum_bytes` / `mrs_hash_default` expose a bignum's raw magnitude and a hash's default value.
- A `MrsValueId` that is not a node of the arena it is used with is harmless: `mrs_kind` returns `MRS_KIND_INVALID` and the other functions answer as for a value of the wrong kind (`false`, `0`, `MRS_VALUE_ID_NONE`). `NULL, 0` is accepted wherever a byte buffer is passed.

## `no_std`

Built with `--no-default-features`, the library has no allocator of its own: register one with `mrs_set_allocator` before any other allocating call (every build exports it; with `std` it does nothing).

```c
typedef uint8_t* (*MrsAllocFn)(size_t size, size_t align);
typedef void (*MrsFreeFn)(uint8_t* ptr, size_t size, size_t align);
void mrs_set_allocator(MrsAllocFn alloc, MrsFreeFn free);
```

`alloc` returns at least `size` bytes aligned to `align` (a power of two), or `NULL`; `free` gets back exactly the `(ptr, size, align)` that `alloc` produced. Only the first call to `mrs_set_allocator` takes effect; later ones are ignored, since switching allocators while memory is outstanding would free it through the wrong one.

A failed allocation has no error return. If `alloc` returns `NULL`, or none was registered, the library's panic handler runs and calls C `abort()`, which the host must provide when linking (libc's will do). See [`examples/README.md`](examples/README.md) for a worked example.
