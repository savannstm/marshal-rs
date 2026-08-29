#ifndef MARSHAL_RS_H
#define MARSHAL_RS_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque owning handle to a loaded/built Arena. Must be released exactly
 * once via mrs_arena_free. */
typedef struct MrsArena MrsArena;

/* A ValueId is an index into an MrsArena; MRS_VALUE_ID_NONE marks "no
 * value" (e.g. an out-of-range array/hash/member lookup). */
typedef uint32_t MrsValueId;
#define MRS_VALUE_ID_NONE UINT32_MAX

/* Mirrors (a subset of) ReadError as a stable, C-representable code. */
typedef enum MrsError {
    MRS_OK = 0,
    MRS_UNEXPECTED_EOF = 1,
    MRS_INVALID_HEADER = 2,
    MRS_INVALID_TAG = 3,
    MRS_SYMBOL_TABLE_FULL = 4,
    MRS_UNKNOWN_SYMBOL_LINK = 5,
    MRS_UNKNOWN_OBJECT_LINK = 6,
    MRS_LENGTH_OVERFLOW = 7,
    MRS_UNSUPPORTED = 8,
} MrsError;

/* Mirrors Kind (src/arena.rs) exactly, in declaration order. */
typedef enum MrsKind {
    MRS_KIND_NIL = 0,
    MRS_KIND_TRUE = 1,
    MRS_KIND_FALSE = 2,
    MRS_KIND_FIXNUM = 3,
    MRS_KIND_BIGNUM = 4,
    MRS_KIND_FLOAT = 5,
    MRS_KIND_BYTES = 6,
    MRS_KIND_STR = 7,
    MRS_KIND_SYMBOL = 8,
    MRS_KIND_REGEXP = 9,
    MRS_KIND_ARRAY = 10,
    MRS_KIND_HASH = 11,
    MRS_KIND_STRUCT = 12,
    MRS_KIND_OBJECT = 13,
    MRS_KIND_CLASS = 14,
    MRS_KIND_MODULE = 15,
} MrsKind;

/* Host allocation hooks, required only when this library was built without
 * its default `std` feature (a genuinely no_std/freestanding build - see the
 * "no_std / FFI" section of the top-level README). `alloc` must return a
 * pointer to at least `size` bytes aligned to `align` (a power of two), or
 * NULL on failure; `free` gets back exactly the (ptr, size, align) triple a
 * prior `alloc` call produced. Neither is assumed to be malloc/free - back
 * them with whatever allocator the target actually has. */
typedef uint8_t* (*MrsAllocFn)(size_t size, size_t align);
typedef void (*MrsFreeFn)(uint8_t* ptr, size_t size, size_t align);

/* Registers the host's allocator. Must be called exactly once, before any
 * other mrs_* function, in a no_std build of this library; has no effect
 * (and need not be called) in the default std-linked build, which uses
 * Rust's own allocator. */
void mrs_set_allocator(MrsAllocFn alloc, MrsFreeFn free);

/* Loads a Marshal byte buffer. Returns NULL and sets *out_error (if
 * non-null) on failure. Free the result with mrs_arena_free. */
MrsArena* mrs_load(const uint8_t* buf, size_t len, MrsError* out_error);

/* Releases an MrsArena handle. NULL is a no-op. */
void mrs_arena_free(MrsArena* arena);

/* Dumps `arena` back to Marshal bytes into a freshly heap-allocated buffer;
 * `*out_len` receives its length. Free the result with mrs_buffer_free. */
uint8_t* mrs_dump(const MrsArena* arena, size_t* out_len);

/* Releases a buffer returned by mrs_dump. `len` must be the same value
 * `mrs_dump` wrote to `*out_len`. */
void mrs_buffer_free(uint8_t* buf, size_t len);

MrsValueId mrs_root(const MrsArena* arena);
MrsKind mrs_kind(const MrsArena* arena, MrsValueId id);

bool mrs_as_bool(const MrsArena* arena, MrsValueId id, bool* out);
bool mrs_as_i64(const MrsArena* arena, MrsValueId id, int64_t* out);
bool mrs_as_f64(const MrsArena* arena, MrsValueId id, double* out);

/* Borrowed pointer into `arena`'s storage - valid as long as `arena` is
 * not freed. Works for Str, Bytes, and Symbol values. */
bool mrs_as_bytes(const MrsArena* arena, MrsValueId id, const uint8_t** out_ptr, size_t* out_len);

/* Borrowed pointer to the value's declared class/module name, if any. */
bool mrs_class_name(const MrsArena* arena, MrsValueId id, const uint8_t** out_ptr, size_t* out_len);

/* Borrowed pointer to a Class/Module value's raw path (Ruby stores these as
 * plain strings, not symbols) - distinct from mrs_class_name, which is the
 * *declared class of* a value, not a Class/Module value's own name. */
bool mrs_as_path(const MrsArena* arena, MrsValueId id, const uint8_t** out_ptr, size_t* out_len);

/* A Regexp's source pattern (borrowed) and its option bits (1=ignorecase,
 * 2=extended, 4=multiline - combinable). */
bool mrs_as_regexp(
    const MrsArena* arena,
    MrsValueId id,
    const uint8_t** out_ptr,
    size_t* out_len,
    uint8_t* out_options
);

/* This crate never transcodes - a Str/Bytes/Regexp value's bytes are exactly
 * what was on the wire, tagged with a declared encoding id. 0 means
 * ASCII-8BIT (Ruby's default; also what an untagged Bytes value implicitly
 * means even though no ivar was ever written for it). 255 means the name
 * isn't in the fixed table - look it up with mrs_encoding_name instead of
 * hardcoding against the id. */
#define MRS_ENCODING_ASCII_8BIT 0
#define MRS_ENCODING_CUSTOM 255
bool mrs_encoding_id(const MrsArena* arena, MrsValueId id, uint8_t* out);

/* Borrowed pointer to the name behind mrs_encoding_id - resolved from the
 * fixed table for a known id, or from the arena's custom-encoding side table
 * for MRS_ENCODING_CUSTOM. Valid as long as `arena` is not freed. */
bool mrs_encoding_name(const MrsArena* arena, MrsValueId id, const uint8_t** out_ptr, size_t* out_len);

/* A Bignum's value as a decimal string, in a freshly heap-allocated buffer -
 * release it with mrs_buffer_free, exactly like mrs_dump's result. */
bool mrs_as_bignum_decimal(const MrsArena* arena, MrsValueId id, uint8_t** out_ptr, size_t* out_len);

uint32_t mrs_array_len(const MrsArena* arena, MrsValueId id);
MrsValueId mrs_array_get(const MrsArena* arena, MrsValueId id, uint32_t index);

uint32_t mrs_hash_len(const MrsArena* arena, MrsValueId id);
MrsValueId mrs_hash_key_at(const MrsArena* arena, MrsValueId id, uint32_t index);
MrsValueId mrs_hash_value_at(const MrsArena* arena, MrsValueId id, uint32_t index);

uint32_t mrs_members_len(const MrsArena* arena, MrsValueId id);
bool mrs_member_name_at(const MrsArena* arena, MrsValueId id, uint32_t index, const uint8_t** out_ptr, size_t* out_len);
MrsValueId mrs_member_value_at(const MrsArena* arena, MrsValueId id, uint32_t index);

#ifdef __cplusplus
}
#endif

#endif /* MARSHAL_RS_H */
