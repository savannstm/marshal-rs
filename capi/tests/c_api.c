#include <marshal-rs/marshal.h>

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
        MrsError err = MRS_ERROR_UNSUPPORTED;
        MrsArena* arena = mrs_load(bytes, sizeof(bytes), &err);
        CHECK(arena != NULL);
        CHECK(err == MRS_ERROR_OK);
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
        MrsError err = MRS_ERROR_OK;
        MrsArena* arena = mrs_load(bytes, sizeof(bytes), &err);
        CHECK(arena == NULL);
        CHECK(err == MRS_ERROR_INVALID_HEADER);
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
    MrsError err = MRS_ERROR_UNSUPPORTED;
    MrsArena* arena = mrs_load(HASH_FIXTURE, sizeof(HASH_FIXTURE), &err);
    CHECK(arena != NULL);
    CHECK(err == MRS_ERROR_OK);

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
 * caller passes a path on argv[1]. */
static void test_load_details_and_many(void) {
    /* An empty buffer may be passed as NULL, 0 and is just a bad header. */
    MrsLoadError err = { MRS_ERROR_OK, 0 };
    CHECK(mrs_load_ex(NULL, 0, &err) == NULL);
    CHECK(err.code == MRS_ERROR_INVALID_HEADER);

    /* A string that claims more bytes than there are: the error says where. */
    const uint8_t truncated[] = { 0x04, 0x08, '"', 0x0a, 'a' };
    CHECK(mrs_load_ex(truncated, sizeof(truncated), &err) == NULL);
    CHECK(err.code == MRS_ERROR_LENGTH_OVERFLOW || err.code == MRS_ERROR_UNEXPECTED_EOF);

    /* Two concatenated streams: 1, then nil. */
    const uint8_t two[] = { 0x04, 0x08, 'i', 0x06, 0x04, 0x08, '0' };
    MrsArenaList* list = mrs_load_many(two, sizeof(two), &err);
    CHECK(list != NULL);
    CHECK(mrs_arena_list_len(list) == 2);
    MrsArena* first = mrs_arena_list_take(list, 0);
    CHECK(first != NULL);
    CHECK(mrs_arena_list_take(list, 0) == NULL);
    CHECK(mrs_arena_list_take(list, 2) == NULL);
    int64_t number = 0;
    CHECK(mrs_as_i64(first, mrs_root(first), &number) && number == 1);
    mrs_arena_free(first);
    mrs_arena_list_free(list);
}

static void test_invalid_ids_are_harmless(void) {
    const uint8_t bytes[] = { 0x04, 0x08, '[', 0x06, '0' };
    MrsArena* arena = mrs_load(bytes, sizeof(bytes), NULL);
    CHECK(arena != NULL);
    const MrsValueId bogus = 123456;
    CHECK(mrs_kind(arena, bogus) == MRS_KIND_INVALID);
    CHECK(mrs_kind(arena, MRS_VALUE_ID_NONE) == MRS_KIND_INVALID);
    int64_t number = 0;
    CHECK(!mrs_as_i64(arena, bogus, &number));
    CHECK(mrs_array_len(arena, bogus) == 0);
    CHECK(mrs_array_get(arena, bogus, 0) == MRS_VALUE_ID_NONE);
    CHECK(mrs_hash_get_symbol(arena, bogus, NULL, 0) == MRS_VALUE_ID_NONE);
    CHECK(mrs_flags(arena, bogus) == 0);
    mrs_arena_free(arena);
}

static void test_building_and_wrappers(void) {
    /* Build [1, "hi", :sym], edit it, dump, load the dump back. */
    MrsArena* arena = mrs_arena_new();
    CHECK(arena != NULL);
    MrsValueId one = mrs_push_fixnum(arena, 1);
    MrsValueId text = mrs_push_string(arena, (const uint8_t*)"hi", 2);
    MrsValueId sym = mrs_push_symbol(arena, (const uint8_t*)"sym", 3);
    CHECK(mrs_push_string(arena, (const uint8_t*)"\xff", 1) == MRS_VALUE_ID_NONE);
    MrsValueId items[3] = { one, text, sym };
    MrsValueId array = mrs_push_array(arena, items, 3);
    CHECK(array != MRS_VALUE_ID_NONE);
    MrsValueId bogus[1] = { 123456 };
    CHECK(mrs_push_array(arena, bogus, 1) == MRS_VALUE_ID_NONE);
    CHECK(mrs_set_root(arena, array));
    CHECK(!mrs_set_root(arena, 123456));
    CHECK(mrs_set_array_value(arena, array, 0, mrs_push_fixnum(arena, 5)));
    CHECK(!mrs_set_array_value(arena, array, 3, one));
    CHECK(!mrs_set_array_value(arena, one, 0, one));
    CHECK(mrs_set_string_text(arena, text, (const uint8_t*)"yo", 2));
    CHECK(!mrs_set_fixnum_value(arena, text, 1));

    size_t out_len = 0;
    uint8_t* out = mrs_dump(arena, &out_len);
    MrsArena* back = mrs_load(out, out_len, NULL);
    CHECK(back != NULL);
    CHECK(mrs_array_len(back, mrs_root(back)) == 3);
    int64_t number = 0;
    CHECK(mrs_as_i64(back, mrs_array_get(back, mrs_root(back), 0), &number) && number == 5);
    mrs_arena_free(back);
    mrs_buffer_free(out, out_len);
    mrs_arena_free(arena);

    /* U:Foo wrapping o:Bar: reads see the body, the wrapper keeps its own class. */
    const uint8_t wrapped[] = { 0x04, 0x08, 'U', ':', 0x08, 'F', 'o', 'o', 'o', ':', 0x08, 'B', 'a', 'r', 0x00 };
    arena = mrs_load(wrapped, sizeof(wrapped), NULL);
    CHECK(arena != NULL);
    MrsValueId root = mrs_root(arena);
    CHECK(mrs_kind(arena, root) == MRS_KIND_OBJECT);
    const uint8_t* name = NULL;
    size_t name_len = 0;
    CHECK(mrs_class_name(arena, root, &name, &name_len) && name_len == 3 && memcmp(name, "Foo", 3) == 0);
    MrsValueId body = mrs_body(arena, root);
    CHECK(body != MRS_VALUE_ID_NONE);
    CHECK(mrs_class_name(arena, body, &name, &name_len) && name_len == 3 && memcmp(name, "Bar", 3) == 0);
    CHECK(mrs_body(arena, body) == MRS_VALUE_ID_NONE);
    mrs_arena_free(arena);

    /* Float text, and bytes after the value. */
    const uint8_t flt[] = { 0x04, 0x08, 'f', 0x08, '1', '.', '5' };
    arena = mrs_load(flt, sizeof(flt), NULL);
    CHECK(arena != NULL);
    CHECK(mrs_as_float_text(arena, mrs_root(arena), &name, &name_len) && name_len == 3);
    mrs_arena_free(arena);
    const uint8_t trailing[] = { 0x04, 0x08, '0', 'x' };
    MrsLoadError err = { MRS_ERROR_OK, 0 };
    CHECK(mrs_load_ex(trailing, sizeof(trailing), &err) == NULL);
    CHECK(err.code == MRS_ERROR_TRAILING_DATA && err.offset == 3);
}

static void test_flags_and_member_lookup(void) {
    /* U:Foo{} (a marshal_dump wrapper around an empty hash). */
    const uint8_t wrapped[] = { 0x04, 0x08, 'U', ':', 0x08, 'F', 'o', 'o', '{', 0x00 };
    MrsArena* arena = mrs_load(wrapped, sizeof(wrapped), NULL);
    CHECK(arena != NULL);
    CHECK((mrs_flags(arena, mrs_root(arena)) & MRS_FLAG_USER_MARSHAL) != 0);
    CHECK(mrs_hash_get_symbol(arena, mrs_root(arena), (const uint8_t*)"x", 1) == MRS_VALUE_ID_NONE);
    mrs_arena_free(arena);

    /* o:Foo with @a = 7 */
    const uint8_t object[] = { 0x04, 0x08, 'o', ':', 0x08, 'F', 'o', 'o', 0x06, ':', 0x07, '@', 'a', 'i', 0x0c };
    arena = mrs_load(object, sizeof(object), NULL);
    CHECK(arena != NULL);
    MrsValueId member = mrs_member_get(arena, mrs_root(arena), (const uint8_t*)"a", 1);
    int64_t number = 0;
    CHECK(member != MRS_VALUE_ID_NONE && mrs_as_i64(arena, member, &number) && number == 7);
    CHECK(mrs_extra_ivar_count(arena, mrs_root(arena)) == 0);
    mrs_arena_free(arena);
}

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

    MrsError err = MRS_ERROR_UNSUPPORTED;
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
    test_load_details_and_many();
    test_flags_and_member_lookup();
    test_invalid_ids_are_harmless();
    test_building_and_wrappers();

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
