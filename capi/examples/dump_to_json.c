#include <marshal-rs/marshal.h>
#include "yyjson/yyjson.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// Built against a no_std marshal-rs-capi (see examples/README.md), which has no allocator of its own: give it
// one via mrs_set_allocator before any other mrs_* call. malloc/free are what is available here, but the hook
// does not require them; a freestanding target would use its own heap.
static uint8_t* host_alloc(size_t size, size_t align) {
    if (size == 0) {
        size = 1;
    }
    // Plain malloc guarantees only alignof(max_align_t), 16 bytes on every target here; stricter alignment
    // goes through the platform's aligned allocator (MSVC's UCRT has no standard aligned_alloc).
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

// The JSON follows the envelope the Rust crate's `serde` feature writes (src/ser.rs), so either side can read
// the other's files. Binary payloads are padded base64. A `str`, `float`, `symbol`, `class` or `module` is text
// when its bytes are valid UTF-8, else base64 plus `"__base64": true`. A non-UTF-8 name is `{"__base64": "..."}`.
// A `marshal_dump`/`_dump_data` wrapper is a `user_marshal` / `data` object around its body.
//
// marshal-rs-capi never transcodes. A `Str`'s bytes may be in any encoding, and a UTF-8 tag does not guarantee
// valid bytes (RPG Maker's compressed script data is tagged UTF-8 but does not validate). Unvalidated bytes in
// a JSON string would make invalid JSON, hence the base64 fallback.

// Nesting deeper than this (or a self-containing value) is cut off instead of overflowing the C stack.
// Same limit as marshal_rs::ser::MAX_NESTING on the Rust side.
#define MAX_NESTING 256

static yyjson_mut_val* base64_to_json(yyjson_mut_doc* doc, const uint8_t* data, size_t len) {
    static const char alphabet[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    size_t out_len = (len + 2) / 3 * 4;
    char* text = malloc(out_len + 1);
    size_t o = 0;
    for (size_t i = 0; i < len; i += 3) {
        uint32_t word = (uint32_t)data[i] << 16;
        if (i + 1 < len) {
            word |= (uint32_t)data[i + 1] << 8;
        }
        if (i + 2 < len) {
            word |= data[i + 2];
        }
        text[o++] = alphabet[(word >> 18) & 63];
        text[o++] = alphabet[(word >> 12) & 63];
        text[o++] = i + 1 < len ? alphabet[(word >> 6) & 63] : '=';
        text[o++] = i + 2 < len ? alphabet[word & 63] : '=';
    }
    yyjson_mut_val* value = yyjson_mut_strncpy(doc, text, out_len);
    free(text);
    return value;
}

// A name: a string if the bytes are valid UTF-8, else {"__base64": "..."} - never validated away or replaced.
static yyjson_mut_val* name_to_json(yyjson_mut_doc* doc, const uint8_t* ptr, size_t len) {
    if (is_valid_utf8(ptr, len)) {
        return yyjson_mut_strncpy(doc, (const char*)ptr, len);
    }
    yyjson_mut_val* object = yyjson_mut_obj(doc);
    yyjson_mut_obj_add_val(doc, object, "__base64", base64_to_json(doc, ptr, len));
    return object;
}

static void add_name(yyjson_mut_doc* doc, yyjson_mut_val* object, const char* key, const uint8_t* ptr, size_t len) {
    yyjson_mut_obj_add_val(doc, object, key, name_to_json(doc, ptr, len));
}

// A `__value` made of bytes (str, float, symbol, class, module): plain text if valid UTF-8, else base64 plus
// `"__base64": true`.
static void add_text_value(yyjson_mut_doc* doc, yyjson_mut_val* object, const uint8_t* ptr, size_t len) {
    if (is_valid_utf8(ptr, len)) {
        yyjson_mut_obj_add_strncpy(doc, object, "__value", (const char*)ptr, len);
    } else {
        yyjson_mut_obj_add_val(doc, object, "__value", base64_to_json(doc, ptr, len));
        yyjson_mut_obj_add_bool(doc, object, "__base64", true);
    }
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
        case MRS_KIND_INVALID:
            break;
    }
    return "unknown";
}

static yyjson_mut_val* to_json(yyjson_mut_doc* doc, const MrsArena* arena, MrsValueId id, int depth);

// `[name, value]` pairs for an object's members or a value's extra instance variables.
static yyjson_mut_val* pair_to_json(yyjson_mut_doc* doc, const MrsArena* arena, const uint8_t* name, size_t name_len,
                                    MrsValueId value, int depth) {
    yyjson_mut_val* pair = yyjson_mut_arr(doc);
    yyjson_mut_arr_add_val(pair, name_to_json(doc, name, name_len));
    yyjson_mut_arr_add_val(pair, to_json(doc, arena, value, depth + 1));
    return pair;
}

// `__extensions` and `__ivars`, which any kind of value can have.
static void add_extras(yyjson_mut_doc* doc, yyjson_mut_val* out, const MrsArena* arena, MrsValueId id, int depth) {
    uint32_t extensions = mrs_extension_count(arena, id);
    if (extensions > 0) {
        yyjson_mut_val* array = yyjson_mut_arr(doc);
        for (uint32_t i = 0; i < extensions; i++) {
            const uint8_t* ptr = NULL;
            size_t len = 0;
            if (mrs_extension_at(arena, id, i, &ptr, &len)) {
                yyjson_mut_arr_add_val(array, name_to_json(doc, ptr, len));
            }
        }
        yyjson_mut_obj_add_val(doc, out, "__extensions", array);
    }
    uint32_t ivars = mrs_extra_ivar_count(arena, id);
    if (ivars > 0) {
        yyjson_mut_val* array = yyjson_mut_arr(doc);
        for (uint32_t i = 0; i < ivars; i++) {
            const uint8_t* ptr = NULL;
            size_t len = 0;
            if (mrs_extra_ivar_name_at(arena, id, i, &ptr, &len)) {
                yyjson_mut_arr_add_val(
                    array, pair_to_json(doc, arena, ptr, len, mrs_extra_ivar_value_at(arena, id, i), depth));
            }
        }
        yyjson_mut_obj_add_val(doc, out, "__ivars", array);
    }
}

static yyjson_mut_val* to_json(yyjson_mut_doc* doc, const MrsArena* arena, MrsValueId id, int depth) {
    if (depth >= MAX_NESTING) {
        fprintf(stderr, "nested too deeply (or it refers back to itself): cut off\n");
        return yyjson_mut_null(doc);
    }

    uint8_t flags = mrs_flags(arena, id);

    // A `marshal_dump` / `_dump_data` wrapper: its own class around the returned value. Everything else
    // reads through the wrapper, so this check comes first.
    if (flags & (MRS_FLAG_DATA | MRS_FLAG_USER_MARSHAL)) {
        yyjson_mut_val* out = yyjson_mut_obj(doc);
        const char* type = (flags & MRS_FLAG_DATA) ? "data" : "user_marshal";
        yyjson_mut_obj_add_strncpy(doc, out, "__type", type, strlen(type));
        const uint8_t* class_ptr = NULL;
        size_t class_len = 0;
        if (mrs_class_name(arena, id, &class_ptr, &class_len)) {
            add_name(doc, out, "__class", class_ptr, class_len);
        }
        add_extras(doc, out, arena, id, depth);
        MrsValueId body = mrs_body(arena, id);
        yyjson_mut_obj_add_val(doc, out, "__value", to_json(doc, arena, body, depth + 1));
        return out;
    }

    MrsKind kind = mrs_kind(arena, id);

    // Trivial kinds are bare JSON primitives, as in the `serde` envelope (src/ser.rs); no reason to differ.
    if (kind == MRS_KIND_NIL || kind == MRS_KIND_INVALID) {
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
        add_name(doc, out, "__class", class_ptr, class_len);
    }

    if (flags & (MRS_FLAG_USER_CLASS | MRS_FLAG_USER_DEFINED | MRS_FLAG_OLD_MODULE)) {
        yyjson_mut_val* names = yyjson_mut_arr(doc);
        if (flags & MRS_FLAG_OLD_MODULE) {
            yyjson_mut_arr_add_str(doc, names, "old_module");
        }
        if (flags & MRS_FLAG_USER_CLASS) {
            yyjson_mut_arr_add_str(doc, names, "user_class");
        }
        if (flags & MRS_FLAG_USER_DEFINED) {
            yyjson_mut_arr_add_str(doc, names, "user_defined");
        }
        yyjson_mut_obj_add_val(doc, out, "__flags", names);
    }
    add_extras(doc, out, arena, id, depth);

    // Declared encoding of a Str/Regexp (unless the implicit ASCII-8BIT), so a consumer can transcode;
    // matches `src/ser.rs`'s `__encoding` field.
    if (kind == MRS_KIND_STR || kind == MRS_KIND_REGEXP) {
        uint8_t encoding_id = 0;
        const uint8_t* enc_ptr = NULL;
        size_t enc_len = 0;
        if (mrs_encoding_id(arena, id, &encoding_id) && encoding_id != MRS_ENCODING_ASCII_8BIT &&
            mrs_encoding_name(arena, id, &enc_ptr, &enc_len)) {
            add_name(doc, out, "__encoding", enc_ptr, enc_len);
        }
    }

    switch (kind) {
        case MRS_KIND_BIGNUM: {
            uint8_t* ptr = NULL;
            size_t len = 0;
            if (mrs_as_bignum_decimal(arena, id, &ptr, &len)) {
                yyjson_mut_obj_add_strncpy(doc, out, "__value", (const char*)ptr, len);
                mrs_buffer_free(ptr, len);
            } else {
                fprintf(stderr, "bignum too large to convert to decimal: left out\n");
            }
            break;
        }
        case MRS_KIND_FLOAT: {
            const uint8_t* ptr = NULL;
            size_t len = 0;
            if (mrs_as_float_text(arena, id, &ptr, &len)) {
                add_text_value(doc, out, ptr, len);
            }
            break;
        }
        case MRS_KIND_BYTES: {
            const uint8_t* ptr = NULL;
            size_t len = 0;
            if (mrs_as_bytes(arena, id, &ptr, &len)) {
                yyjson_mut_obj_add_val(doc, out, "__value", base64_to_json(doc, ptr, len));
            }
            break;
        }
        case MRS_KIND_STR: {
            const uint8_t* ptr = NULL;
            size_t len = 0;
            if (mrs_as_bytes(arena, id, &ptr, &len)) {
                add_text_value(doc, out, ptr, len);
            }
            break;
        }
        case MRS_KIND_SYMBOL: {
            const uint8_t* ptr = NULL;
            size_t len = 0;
            if (mrs_as_bytes(arena, id, &ptr, &len)) {
                add_text_value(doc, out, ptr, len);
            }
            break;
        }
        case MRS_KIND_REGEXP: {
            const uint8_t* ptr = NULL;
            size_t len = 0;
            uint8_t options = 0;
            if (mrs_as_regexp(arena, id, &ptr, &len, &options)) {
                yyjson_mut_val* regexp = yyjson_mut_arr(doc);
                yyjson_mut_arr_add_val(regexp, base64_to_json(doc, ptr, len));
                yyjson_mut_arr_add_uint(doc, regexp, options);
                yyjson_mut_obj_add_val(doc, out, "__value", regexp);
            }
            break;
        }
        case MRS_KIND_ARRAY: {
            uint32_t len = mrs_array_len(arena, id);
            yyjson_mut_val* array = yyjson_mut_arr(doc);
            for (uint32_t i = 0; i < len; i++) {
                yyjson_mut_arr_add_val(array, to_json(doc, arena, mrs_array_get(arena, id, i), depth + 1));
            }
            yyjson_mut_obj_add_val(doc, out, "__value", array);
            break;
        }
        case MRS_KIND_HASH: {
            // Represented as an array of [key, value] pairs, not a JSON object: Ruby hash keys aren't always
            // strings.
            uint32_t len = mrs_hash_len(arena, id);
            yyjson_mut_val* pairs = yyjson_mut_arr(doc);
            for (uint32_t i = 0; i < len; i++) {
                yyjson_mut_val* pair = yyjson_mut_arr(doc);
                yyjson_mut_arr_add_val(pair, to_json(doc, arena, mrs_hash_key_at(arena, id, i), depth + 1));
                yyjson_mut_arr_add_val(pair, to_json(doc, arena, mrs_hash_value_at(arena, id, i), depth + 1));
                yyjson_mut_arr_add_val(pairs, pair);
            }
            yyjson_mut_obj_add_val(doc, out, "__value", pairs);
            MrsValueId fallback = mrs_hash_default(arena, id);
            if (fallback != MRS_VALUE_ID_NONE) {
                yyjson_mut_obj_add_val(doc, out, "__default", to_json(doc, arena, fallback, depth + 1));
            }
            break;
        }
        case MRS_KIND_STRUCT:
        case MRS_KIND_OBJECT: {
            uint32_t len = mrs_members_len(arena, id);
            yyjson_mut_val* members = yyjson_mut_arr(doc);
            for (uint32_t i = 0; i < len; i++) {
                const uint8_t* name_ptr = NULL;
                size_t name_len = 0;
                if (mrs_member_name_at(arena, id, i, &name_ptr, &name_len)) {
                    yyjson_mut_arr_add_val(members, pair_to_json(doc, arena, name_ptr, name_len,
                                                                 mrs_member_value_at(arena, id, i), depth));
                }
            }
            yyjson_mut_obj_add_val(doc, out, "__members", members);
            break;
        }
        case MRS_KIND_CLASS:
        case MRS_KIND_MODULE: {
            const uint8_t* ptr = NULL;
            size_t len = 0;
            if (mrs_as_path(arena, id, &ptr, &len)) {
                add_text_value(doc, out, ptr, len);
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

    MrsLoadError error = { MRS_ERROR_OK, 0 };
    MrsArena* arena = mrs_load_ex(bytes, (size_t)size, &error);
    if (!arena) {
        fprintf(stderr, "%s: failed to load (error code %d at byte %zu)\n", argv[1], (int)error.code, error.offset);
        free(bytes);
        return 1;
    }

    yyjson_mut_doc* doc = yyjson_mut_doc_new(NULL);
    yyjson_mut_val* root = to_json(doc, arena, mrs_root(arena), 0);
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
