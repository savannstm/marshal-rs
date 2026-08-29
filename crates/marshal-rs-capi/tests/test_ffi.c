#include "../assets/marshal.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int failures = 0;

#define CHECK(cond) \
    do { \
        if (!(cond)) { \
            fprintf(stderr, "FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond); \
            failures++; \
        } \
    } while (0)

static void test_scalars_and_invalid_input(void) {
    /* nil */
    {
        const uint8_t bytes[] = { 0x04, 0x08, '0' };
        MrsError err = MRS_UNSUPPORTED;
        MrsArena* arena = mrs_load(bytes, sizeof(bytes), &err);
        CHECK(arena != NULL);
        CHECK(err == MRS_OK);
        CHECK(mrs_kind(arena, mrs_root(arena)) == MRS_KIND_NIL);
        mrs_arena_free(arena);
    }

    /* true / false */
    {
        const uint8_t bytes[] = { 0x04, 0x08, 'T' };
        MrsArena* arena = mrs_load(bytes, sizeof(bytes), NULL);
        CHECK(arena != NULL);
        bool value = false;
        CHECK(mrs_as_bool(arena, mrs_root(arena), &value));
        CHECK(value == true);
        mrs_arena_free(arena);
    }

    /* fixnum */
    {
        const uint8_t bytes[] = { 0x04, 0x08, 'i', 0x02, 0x2C, 0x01 }; /* 300 */
        MrsArena* arena = mrs_load(bytes, sizeof(bytes), NULL);
        CHECK(arena != NULL);
        int64_t value = 0;
        CHECK(mrs_as_i64(arena, mrs_root(arena), &value));
        CHECK(value == 300);
        mrs_arena_free(arena);
    }

    /* invalid header -> NULL + correct error code, never a crash */
    {
        const uint8_t bytes[] = { 0x04, 0x07, '0' };
        MrsError err = MRS_OK;
        MrsArena* arena = mrs_load(bytes, sizeof(bytes), &err);
        CHECK(arena == NULL);
        CHECK(err == MRS_INVALID_HEADER);
    }

    /* null out_error pointer must not crash mrs_load on failure */
    {
        const uint8_t bytes[] = { 0x04, 0x07, '0' };
        MrsArena* arena = mrs_load(bytes, sizeof(bytes), NULL);
        CHECK(arena == NULL);
    }

    /* mrs_arena_free(NULL) is a documented no-op */
    mrs_arena_free(NULL);
}

/* Marshal.dump({ name: "Alice", hp: 30, tags: [:hero, :fire] }) - the same
 * fixture as examples/basic.rs, reused here so both surfaces are checked
 * against the same known-good bytes. */
static const uint8_t HASH_FIXTURE[] = {
    0x04, 0x08, 0x7b, 0x08, 0x3a, 0x09, 'n',  'a',  'm', 'e',  0x49, 0x22, 0x0a, 'A',  'l', 'i', 'c',
    'e',  0x06, 0x3a, 0x06, 'E',  0x54, 0x3a, 0x07, 'h', 'p',  0x69, 0x23, 0x3a, 0x09, 't', 'a', 'g',
    's',  0x5b, 0x07, 0x3a, 0x09, 'h',  'e',  'r',  'o', 0x3a, 0x09, 'f',  'i',  'r',  'e',
};

static MrsValueId hash_lookup_symbol(const MrsArena* arena, MrsValueId hash, const char* name) {
    size_t name_len = strlen(name);
    uint32_t len = mrs_hash_len(arena, hash);
    for (uint32_t i = 0; i < len; i++) {
        MrsValueId key = mrs_hash_key_at(arena, hash, i);
        if (mrs_kind(arena, key) != MRS_KIND_SYMBOL) {
            continue;
        }
        const uint8_t* ptr;
        size_t len_out;
        CHECK(mrs_as_bytes(arena, key, &ptr, &len_out));
        if (len_out == name_len && memcmp(ptr, name, name_len) == 0) {
            return mrs_hash_value_at(arena, hash, i);
        }
    }
    return MRS_VALUE_ID_NONE;
}

static void test_hash_traversal_and_dump_roundtrip(void) {
    MrsError err = MRS_UNSUPPORTED;
    MrsArena* arena = mrs_load(HASH_FIXTURE, sizeof(HASH_FIXTURE), &err);
    CHECK(arena != NULL);
    CHECK(err == MRS_OK);

    MrsValueId root = mrs_root(arena);
    CHECK(mrs_kind(arena, root) == MRS_KIND_HASH);
    CHECK(mrs_hash_len(arena, root) == 3);

    MrsValueId name = hash_lookup_symbol(arena, root, "name");
    CHECK(name != MRS_VALUE_ID_NONE);
    CHECK(mrs_kind(arena, name) == MRS_KIND_STR);
    {
        const uint8_t* ptr;
        size_t len;
        CHECK(mrs_as_bytes(arena, name, &ptr, &len));
        CHECK(len == 5 && memcmp(ptr, "Alice", 5) == 0);
    }

    MrsValueId hp = hash_lookup_symbol(arena, root, "hp");
    CHECK(hp != MRS_VALUE_ID_NONE);
    int64_t hp_value = 0;
    CHECK(mrs_as_i64(arena, hp, &hp_value));
    CHECK(hp_value == 30);

    MrsValueId tags = hash_lookup_symbol(arena, root, "tags");
    CHECK(tags != MRS_VALUE_ID_NONE);
    CHECK(mrs_kind(arena, tags) == MRS_KIND_ARRAY);
    CHECK(mrs_array_len(arena, tags) == 2);
    {
        MrsValueId hero = mrs_array_get(arena, tags, 0);
        const uint8_t* ptr;
        size_t len;
        CHECK(mrs_kind(arena, hero) == MRS_KIND_SYMBOL);
        CHECK(mrs_as_bytes(arena, hero, &ptr, &len));
        CHECK(len == 4 && memcmp(ptr, "hero", 4) == 0);
    }
    /* out-of-range index reports "no value", not a crash */
    CHECK(mrs_array_get(arena, tags, 99) == MRS_VALUE_ID_NONE);

    /* round-trip: dump must reproduce the exact original bytes */
    size_t out_len = 0;
    uint8_t* dumped = mrs_dump(arena, &out_len);
    CHECK(dumped != NULL);
    CHECK(out_len == sizeof(HASH_FIXTURE));
    CHECK(memcmp(dumped, HASH_FIXTURE, out_len) == 0);
    mrs_buffer_free(dumped, out_len);

    mrs_arena_free(arena);
}

static void test_object_and_class_name(void) {
    /* Marshal.dump: an Object with one ivar */
    const uint8_t bytes[] = { 0x04, 0x08, 'o',  0x3a, 0x11, 'C', 'u', 's', 't', 'o',  'm',  'O',  'b',  'j', 'e',
                              'c',  't',  0x06, 0x3a, 0x0a, '@', 'd', 'a', 't', 'a',  0x49, 0x22, 0x10, 'o', 'b',
                              'j',  'e',  'c',  't',  ' ',  'd', 'a', 't', 'a', 0x06, 0x3a, 0x06, 'E',  0x54 };
    MrsArena* arena = mrs_load(bytes, sizeof(bytes), NULL);
    CHECK(arena != NULL);
    MrsValueId root = mrs_root(arena);
    CHECK(mrs_kind(arena, root) == MRS_KIND_OBJECT);

    const uint8_t* class_ptr;
    size_t class_len;
    CHECK(mrs_class_name(arena, root, &class_ptr, &class_len));
    CHECK(class_len == 12 && memcmp(class_ptr, "CustomObject", 12) == 0);

    CHECK(mrs_members_len(arena, root) == 1);
    const uint8_t* member_name;
    size_t member_name_len;
    CHECK(mrs_member_name_at(arena, root, 0, &member_name, &member_name_len));
    CHECK(member_name_len == 5 && memcmp(member_name, "@data", 5) == 0);

    MrsValueId value = mrs_member_value_at(arena, root, 0);
    const uint8_t* value_ptr;
    size_t value_len;
    CHECK(mrs_as_bytes(arena, value, &value_ptr, &value_len));
    CHECK(value_len == 11 && memcmp(value_ptr, "object data", 11) == 0);

    /* dump must reproduce the exact original bytes */
    size_t out_len = 0;
    uint8_t* dumped = mrs_dump(arena, &out_len);
    CHECK(out_len == sizeof(bytes));
    CHECK(memcmp(dumped, bytes, out_len) == 0);
    mrs_buffer_free(dumped, out_len);

    mrs_arena_free(arena);
}

static void test_encoding(void) {
    /* GBK-encoded "\xba\xba\xd7\xd6\xc4\xda" ("汉字内"), tagged :encoding =>
     * "GBK" - never transcoded, only the tag is understood. */
    const uint8_t bytes[] = { 0x04, 0x08, 'I', 0x22, 0x0b, 0xBA, 0xBA, 0xD7, 0xD6, 0xC4, 0xDA, 0x06, 0x3a, 0x0d,
                              'e',  'n',  'c', 'o',  'd',  'i',  'n',  'g',  0x22, 0x08, 'G',  'B',  'K' };
    MrsArena* arena = mrs_load(bytes, sizeof(bytes), NULL);
    CHECK(arena != NULL);
    MrsValueId root = mrs_root(arena);
    CHECK(mrs_kind(arena, root) == MRS_KIND_STR);

    const uint8_t* raw_ptr;
    size_t raw_len;
    CHECK(mrs_as_bytes(arena, root, &raw_ptr, &raw_len));
    CHECK(raw_len == 6 && memcmp(raw_ptr, "\xBA\xBA\xD7\xD6\xC4\xDA", 6) == 0);

    uint8_t encoding_id;
    CHECK(mrs_encoding_id(arena, root, &encoding_id));
    CHECK(encoding_id != MRS_ENCODING_ASCII_8BIT && encoding_id != MRS_ENCODING_CUSTOM);

    const uint8_t* name_ptr;
    size_t name_len;
    CHECK(mrs_encoding_name(arena, root, &name_ptr, &name_len));
    CHECK(name_len == 3 && memcmp(name_ptr, "GBK", 3) == 0);

    /* dump must reproduce the exact original bytes - the declared encoding
     * round-trips byte-exact, not just the raw content. */
    size_t out_len = 0;
    uint8_t* dumped = mrs_dump(arena, &out_len);
    CHECK(out_len == sizeof(bytes));
    CHECK(memcmp(dumped, bytes, out_len) == 0);
    mrs_buffer_free(dumped, out_len);

    mrs_arena_free(arena);
}

/* Round-trips a real RPG Maker save/data file through the C API, if the
 * caller passes a path on argv[1] - the same fixtures the Rust test suite
 * (tests/roundtrip.rs, examples/fixture_check.rs) is validated against. */
static int test_real_fixture(const char* path) {
    FILE* f = fopen(path, "rb");
    if (!f) {
        fprintf(stderr, "could not open %s\n", path);
        return 1;
    }
    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);
    uint8_t* buf = malloc((size_t)size);
    if (fread(buf, 1, (size_t)size, f) != (size_t)size) {
        fprintf(stderr, "short read on %s\n", path);
        free(buf);
        fclose(f);
        return 1;
    }
    fclose(f);

    MrsError err = MRS_UNSUPPORTED;
    MrsArena* arena = mrs_load(buf, (size_t)size, &err);
    CHECK(arena != NULL);
    if (!arena) {
        fprintf(stderr, "%s: load failed with error %d\n", path, (int)err);
        free(buf);
        return 1;
    }

    size_t out_len = 0;
    uint8_t* dumped = mrs_dump(arena, &out_len);
    CHECK(dumped != NULL);
    int exact = (out_len == (size_t)size) && memcmp(dumped, buf, out_len) == 0;
    CHECK(exact);
    printf("%s: %s (orig=%ld redump=%zu)\n", path, exact ? "EXACT" : "DIFFERS", size, out_len);

    mrs_buffer_free(dumped, out_len);
    mrs_arena_free(arena);
    free(buf);
    return 0;
}

int main(int argc, char** argv) {
    test_scalars_and_invalid_input();
    test_hash_traversal_and_dump_roundtrip();
    test_object_and_class_name();
    test_encoding();

    for (int i = 1; i < argc; i++) {
        test_real_fixture(argv[i]);
    }

    if (failures == 0) {
        printf("all C API checks passed\n");
        return 0;
    }
    fprintf(stderr, "%d check(s) failed\n", failures);
    return 1;
}
