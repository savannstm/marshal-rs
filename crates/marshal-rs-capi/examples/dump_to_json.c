#include "../assets/marshal.h"
#include "yyjson/yyjson.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// This example is built against a genuinely no_std marshal-rs-capi (see
// examples/README.md), so the library has no allocator of its own - it must
// be given one via mrs_set_allocator before any other mrs_* call. Plain
// malloc/free happen to be what's available here, but nothing about the
// hook shape requires that; a real freestanding target would back this with
// its own heap.
static uint8_t* host_alloc(size_t size, size_t align) {
    if (size == 0) {
        size = 1;
    }
    // Plain malloc only guarantees alignof(max_align_t) - 16 bytes on every
    // platform this example targets - so anything within that is a plain
    // malloc; anything stricter goes through the platform's aligned
    // allocator (MSVC's UCRT has no standard aligned_alloc).
    if (align <= 16) {
        return (uint8_t*)malloc(size);
    }
#if defined(_WIN32)
    return (uint8_t*)_aligned_malloc(size, align);
#else
    size_t rounded = (size + align - 1) & ~(align - 1);
    return (uint8_t*)aligned_alloc(align, rounded);
#endif
}

static void host_free(uint8_t* ptr, size_t size, size_t align) {
    (void)size;
    if (align <= 16) {
        free(ptr);
        return;
    }
#if defined(_WIN32)
    _aligned_free(ptr);
#else
    free(ptr);
#endif
}

static bool is_valid_utf8(const uint8_t* data, size_t len) {
    size_t i = 0;
    while (i < len) {
        uint8_t byte = data[i];
        size_t extra;
        if ((byte & 0x80) == 0x00) {
            extra = 0;
        } else if ((byte & 0xE0) == 0xC0) {
            extra = 1;
        } else if ((byte & 0xF0) == 0xE0) {
            extra = 2;
        } else if ((byte & 0xF8) == 0xF0) {
            extra = 3;
        } else {
            return false;
        }
        if (i + extra >= len) {
            return false;
        }
        for (size_t j = 1; j <= extra; j++) {
            if ((data[i + j] & 0xC0) != 0x80) {
                return false;
            }
        }
        i += extra + 1;
    }
    return true;
}

// marshal-rs-capi never transcodes - a `Str`'s bytes could be declared in
// any encoding, and even a UTF-8 declaration isn't a validity guarantee
// (RPG Maker's own compressed script data is tagged UTF-8 while not
// actually validating - see `Kind::Str`'s doc comment in src/arena.rs).
// Emitting unvalidated bytes into a JSON string would produce invalid
// JSON, so anything that doesn't actually validate falls back to a byte
// array, same as plain `Bytes`.
static yyjson_mut_val* bytes_to_json(yyjson_mut_doc* doc, const uint8_t* ptr, size_t len, bool try_as_text) {
    if (try_as_text && is_valid_utf8(ptr, len)) {
        return yyjson_mut_strncpy(doc, (const char*)ptr, len);
    }
    yyjson_mut_val* array = yyjson_mut_arr(doc);
    for (size_t i = 0; i < len; i++) {
        yyjson_mut_arr_add_val(array, yyjson_mut_uint(doc, ptr[i]));
    }
    return array;
}

static const char* kind_name(MrsKind kind) {
    switch (kind) {
        case MRS_KIND_NIL:
            return "nil";
        case MRS_KIND_TRUE:
        case MRS_KIND_FALSE:
            return "bool";
        case MRS_KIND_FIXNUM:
            return "fixnum";
        case MRS_KIND_BIGNUM:
            return "bignum";
        case MRS_KIND_FLOAT:
            return "float";
        case MRS_KIND_BYTES:
            return "bytes";
        case MRS_KIND_STR:
            return "str";
        case MRS_KIND_SYMBOL:
            return "symbol";
        case MRS_KIND_REGEXP:
            return "regexp";
        case MRS_KIND_ARRAY:
            return "array";
        case MRS_KIND_HASH:
            return "hash";
        case MRS_KIND_STRUCT:
            return "struct";
        case MRS_KIND_OBJECT:
            return "object";
        case MRS_KIND_CLASS:
            return "class";
        case MRS_KIND_MODULE:
            return "module";
    }
    return "unknown";
}

static yyjson_mut_val* to_json(yyjson_mut_doc* doc, const MrsArena* arena, MrsValueId id) {
    MrsKind kind = mrs_kind(arena, id);

    // Trivial kinds serialize as bare JSON primitives, matching the
    // envelope this crate's own `serde` feature uses (src/ser.rs) - no
    // reason for the C side to invent a different shape.
    if (kind == MRS_KIND_NIL) {
        return yyjson_mut_null(doc);
    }
    if (kind == MRS_KIND_TRUE || kind == MRS_KIND_FALSE) {
        bool value = false;
        mrs_as_bool(arena, id, &value);
        return yyjson_mut_bool(doc, value);
    }
    if (kind == MRS_KIND_FIXNUM) {
        int64_t value = 0;
        mrs_as_i64(arena, id, &value);
        return yyjson_mut_int(doc, value);
    }

    yyjson_mut_val* out = yyjson_mut_obj(doc);
    yyjson_mut_obj_add_strncpy(doc, out, "__type", kind_name(kind), strlen(kind_name(kind)));

    const uint8_t* class_ptr = NULL;
    size_t class_len = 0;
    if (mrs_class_name(arena, id, &class_ptr, &class_len)) {
        yyjson_mut_obj_add_strncpy(doc, out, "__class", (const char*)class_ptr, class_len);
    }

    // marshal-rs-capi never transcodes - a Str/Regexp's bytes are exactly
    // what was on the wire. Surface the declared encoding (when it isn't
    // the implicit ASCII-8BIT default) so a consumer of this JSON can
    // transcode itself if it needs to; matches `src/ser.rs`'s `__encoding`
    // field.
    if (kind == MRS_KIND_STR || kind == MRS_KIND_REGEXP) {
        uint8_t encoding_id = 0;
        const uint8_t* enc_ptr = NULL;
        size_t enc_len = 0;
        if (mrs_encoding_id(arena, id, &encoding_id) && encoding_id != MRS_ENCODING_ASCII_8BIT &&
            mrs_encoding_name(arena, id, &enc_ptr, &enc_len)) {
            yyjson_mut_obj_add_strncpy(doc, out, "__encoding", (const char*)enc_ptr, enc_len);
        }
    }

    switch (kind) {
        case MRS_KIND_BIGNUM: {
            uint8_t* ptr = NULL;
            size_t len = 0;
            if (mrs_as_bignum_decimal(arena, id, &ptr, &len)) {
                yyjson_mut_obj_add_strncpy(doc, out, "__value", (const char*)ptr, len);
                mrs_buffer_free(ptr, len);
            }
            break;
        }
        case MRS_KIND_FLOAT: {
            double value = 0.0;
            mrs_as_f64(arena, id, &value);
            yyjson_mut_obj_add_real(doc, out, "__value", value);
            break;
        }
        case MRS_KIND_BYTES:
        case MRS_KIND_STR:
        case MRS_KIND_SYMBOL: {
            const uint8_t* ptr = NULL;
            size_t len = 0;
            if (mrs_as_bytes(arena, id, &ptr, &len)) {
                yyjson_mut_obj_add_val(doc, out, "__value", bytes_to_json(doc, ptr, len, kind != MRS_KIND_BYTES));
            }
            break;
        }
        case MRS_KIND_REGEXP: {
            const uint8_t* ptr = NULL;
            size_t len = 0;
            uint8_t options = 0;
            if (mrs_as_regexp(arena, id, &ptr, &len, &options)) {
                yyjson_mut_val* regexp = yyjson_mut_obj(doc);
                yyjson_mut_obj_add_val(doc, regexp, "source", bytes_to_json(doc, ptr, len, true));
                yyjson_mut_obj_add_uint(doc, regexp, "options", options);
                yyjson_mut_obj_add_val(doc, out, "__value", regexp);
            }
            break;
        }
        case MRS_KIND_ARRAY: {
            uint32_t len = mrs_array_len(arena, id);
            yyjson_mut_val* array = yyjson_mut_arr(doc);
            for (uint32_t i = 0; i < len; i++) {
                yyjson_mut_arr_add_val(array, to_json(doc, arena, mrs_array_get(arena, id, i)));
            }
            yyjson_mut_obj_add_val(doc, out, "__value", array);
            break;
        }
        case MRS_KIND_HASH: {
            // Represented as an array of [key, value] pairs, not a JSON
            // object: Ruby hash keys aren't always strings.
            uint32_t len = mrs_hash_len(arena, id);
            yyjson_mut_val* pairs = yyjson_mut_arr(doc);
            for (uint32_t i = 0; i < len; i++) {
                yyjson_mut_val* pair = yyjson_mut_arr(doc);
                yyjson_mut_arr_add_val(pair, to_json(doc, arena, mrs_hash_key_at(arena, id, i)));
                yyjson_mut_arr_add_val(pair, to_json(doc, arena, mrs_hash_value_at(arena, id, i)));
                yyjson_mut_arr_add_val(pairs, pair);
            }
            yyjson_mut_obj_add_val(doc, out, "__value", pairs);
            break;
        }
        case MRS_KIND_STRUCT:
        case MRS_KIND_OBJECT: {
            uint32_t len = mrs_members_len(arena, id);
            yyjson_mut_val* members = yyjson_mut_arr(doc);
            for (uint32_t i = 0; i < len; i++) {
                const uint8_t* name_ptr = NULL;
                size_t name_len = 0;
                mrs_member_name_at(arena, id, i, &name_ptr, &name_len);
                yyjson_mut_val* pair = yyjson_mut_arr(doc);
                yyjson_mut_arr_add_val(pair, bytes_to_json(doc, name_ptr, name_len, true));
                yyjson_mut_arr_add_val(pair, to_json(doc, arena, mrs_member_value_at(arena, id, i)));
                yyjson_mut_arr_add_val(members, pair);
            }
            yyjson_mut_obj_add_val(doc, out, "__members", members);
            break;
        }
        case MRS_KIND_CLASS:
        case MRS_KIND_MODULE: {
            const uint8_t* ptr = NULL;
            size_t len = 0;
            if (mrs_as_path(arena, id, &ptr, &len)) {
                yyjson_mut_obj_add_val(doc, out, "__value", bytes_to_json(doc, ptr, len, true));
            }
            break;
        }
        default:
            break;
    }

    return out;
}

int main(int argc, char** argv) {
    if (argc != 3) {
        fprintf(stderr, "usage: %s <input.rvdata2> <output.json>\n", argv[0]);
        return 1;
    }

    mrs_set_allocator(host_alloc, host_free);

    FILE* input = fopen(argv[1], "rb");
    if (!input) {
        fprintf(stderr, "could not open %s\n", argv[1]);
        return 1;
    }
    fseek(input, 0, SEEK_END);
    long size = ftell(input);
    fseek(input, 0, SEEK_SET);

    uint8_t* bytes = malloc((size_t)size);
    if (fread(bytes, 1, (size_t)size, input) != (size_t)size) {
        fprintf(stderr, "failed to read %s\n", argv[1]);
        fclose(input);
        free(bytes);
        return 1;
    }
    fclose(input);

    MrsError error = MRS_OK;
    MrsArena* arena = mrs_load(bytes, (size_t)size, &error);
    if (!arena) {
        fprintf(stderr, "%s: failed to load (error code %d)\n", argv[1], (int)error);
        free(bytes);
        return 1;
    }

    yyjson_mut_doc* doc = yyjson_mut_doc_new(NULL);
    yyjson_mut_val* root = to_json(doc, arena, mrs_root(arena));
    yyjson_mut_doc_set_root(doc, root);
    mrs_arena_free(arena);

    yyjson_write_err write_error;
    bool ok = yyjson_mut_write_file(argv[2], doc, 0, NULL, &write_error);
    yyjson_mut_doc_free(doc);
    if (!ok) {
        fprintf(stderr, "failed to write %s: %s\n", argv[2], write_error.msg);
        free(bytes);
        return 1;
    }

    printf("wrote %s (from %ld bytes of Marshal data)\n", argv[2], size);
    free(bytes);
    return 0;
}
