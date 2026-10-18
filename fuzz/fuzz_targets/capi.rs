#![no_main]
//! Drives the C ABI the way a C caller does. Loads the input and walks every value through the accessors, which must
//! terminate on cyclic input, then checks that its dump loads back to the same bytes. Builds an arena from the input's
//! chunks and checks the same round trip.
use libfuzzer_sys::fuzz_target;
use marshal_rs_capi::{
    MRS_VALUE_ID_NONE, MrsArena, MrsKind, MrsValueId, mrs_arena_free, mrs_arena_new, mrs_array_get, mrs_array_len,
    mrs_as_bytes, mrs_body, mrs_buffer_free, mrs_dump, mrs_extra_ivar_count, mrs_extra_ivar_value_at, mrs_hash_key_at,
    mrs_hash_len, mrs_hash_value_at, mrs_kind, mrs_load, mrs_member_name_at, mrs_member_value_at, mrs_members_len,
    mrs_push_array, mrs_push_bytes, mrs_push_string, mrs_push_symbol, mrs_root, mrs_set_root,
};
use std::{collections::HashSet, hint::black_box, ptr, slice};

/// Copies out the dump of `arena` and frees the C buffer.
fn dump(arena: &MrsArena) -> Vec<u8> {
    let mut len = 0usize;
    // SAFETY: `arena` is a valid handle, and `len` is a writable `usize`.
    let dumped = unsafe { mrs_dump(arena, &mut len) };
    // SAFETY: `mrs_dump` returns `len` initialized bytes, which stay valid until `mrs_buffer_free` below.
    let bytes = unsafe { slice::from_raw_parts(dumped, len) }.to_vec();
    mrs_buffer_free(dumped, len);
    bytes
}

/// Loads `bytes` and dumps the result, or returns `None` if the C API refuses them.
fn load_and_dump(bytes: &[u8]) -> Option<Vec<u8>> {
    // SAFETY: `bytes` is a live slice, so its pointer is readable for `bytes.len()` bytes. A null `out_error` is allowed.
    let arena = unsafe { mrs_load(bytes.as_ptr(), bytes.len(), ptr::null_mut()) };
    if arena.is_null() {
        return None;
    }
    // SAFETY: `arena` is non-null and was just returned by `mrs_load`, so it is a live arena that nothing else holds.
    let dumped = dump(unsafe { &*arena });
    // SAFETY: `arena` is the non-null handle from `mrs_load` above, and nothing has freed it yet.
    unsafe { mrs_arena_free(arena) };
    Some(dumped)
}

/// Reads the bytes an accessor lent out, so the sanitizer checks that the pointer and length are valid.
fn touch(text: *const u8, len: usize) {
    if len == 0 {
        return;
    }
    // SAFETY: the accessor that wrote `(text, len)` lent `len` readable bytes, which live as long as the arena.
    let bytes = unsafe { slice::from_raw_parts(text, len) };
    black_box(bytes.iter().fold(0u8, |sum, &byte| sum ^ byte));
}

/// Visits every value reachable from `root` through the accessors. Cyclic input revisits ids, so each one is walked once.
fn walk(arena: &MrsArena, root: MrsValueId) {
    let mut seen = HashSet::new();
    let mut pending = vec![root];

    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }

        match mrs_kind(arena, id) {
            MrsKind::Array => pending.extend((0..mrs_array_len(arena, id)).map(|index| mrs_array_get(arena, id, index))),
            MrsKind::Hash => {
                for index in 0..mrs_hash_len(arena, id) {
                    pending.push(mrs_hash_key_at(arena, id, index));
                    pending.push(mrs_hash_value_at(arena, id, index));
                }
            }
            MrsKind::Object | MrsKind::Struct => {
                for index in 0..mrs_members_len(arena, id) {
                    let mut name: *const u8 = ptr::null();
                    let mut len = 0usize;
                    // SAFETY: `id` belongs to `arena`, and `name`/`len` are locals the accessor writes through.
                    if unsafe { mrs_member_name_at(arena, id, index, &mut name, &mut len) } {
                        touch(name, len);
                    }
                    pending.push(mrs_member_value_at(arena, id, index));
                }
                pending.extend((0..mrs_extra_ivar_count(arena, id)).map(|index| mrs_extra_ivar_value_at(arena, id, index)));
            }
            MrsKind::Bytes | MrsKind::Str | MrsKind::Symbol => {
                let mut text: *const u8 = ptr::null();
                let mut len = 0usize;
                // SAFETY: `id` belongs to `arena`, and `text`/`len` are locals the accessor writes through.
                if unsafe { mrs_as_bytes(arena, id, &mut text, &mut len) } {
                    touch(text, len);
                }
            }
            _ => {}
        }

        let body = mrs_body(arena, id);
        if body != MRS_VALUE_ID_NONE {
            pending.push(body);
        }
    }
}

/// Builds an arena from the input's chunks, as bytes, strings and symbols inside one array. Its dump must load back to
/// the same bytes.
fn build(data: &[u8]) {
    let arena = mrs_arena_new();
    // SAFETY: `mrs_arena_new` just returned this non-null arena, and only this function uses it before `mrs_arena_free`.
    let built = unsafe { &mut *arena };

    let mut items = Vec::new();
    for chunk in data.chunks(5).take(64) {
        let (chunk_ptr, chunk_len) = (chunk.as_ptr(), chunk.len());
        // SAFETY: `chunk` is a live slice, so `chunk_ptr` is readable for `chunk_len` bytes, and `built` is the live arena.
        let ids = unsafe {
            [
                mrs_push_bytes(built, chunk_ptr, chunk_len),
                mrs_push_string(built, chunk_ptr, chunk_len),
                mrs_push_symbol(built, chunk_ptr, chunk_len),
            ]
        };
        // Text that is not valid UTF-8 has no string value, so its id is `MRS_VALUE_ID_NONE`.
        items.extend(ids.into_iter().filter(|&id| id != MRS_VALUE_ID_NONE));
    }

    // SAFETY: `items` is a live slice of `items.len()` ids, and `built` is the live arena.
    let root = unsafe { mrs_push_array(built, items.as_ptr(), items.len()) };
    if root != MRS_VALUE_ID_NONE && mrs_set_root(built, root) {
        let dumped = dump(built);
        assert_eq!(
            load_and_dump(&dumped).as_deref(),
            Some(dumped.as_slice()),
            "built dump does not round-trip"
        );
    }

    // SAFETY: `arena` came from `mrs_arena_new` and is freed exactly once, after `built` is last used.
    unsafe { mrs_arena_free(arena) };
}

fuzz_target!(|data: &[u8]| {
    // Random bytes almost never start with the Marshal header, so the walk would rarely run. Half the inputs get one.
    let framed = [&[4u8, 8][..], data].concat();
    let input = if data.first().is_some_and(|byte| byte % 2 == 0) { &framed[..] } else { data };

    // SAFETY: `input` is a live slice, so its pointer is readable for `input.len()` bytes. A null `out_error` is allowed.
    let arena = unsafe { mrs_load(input.as_ptr(), input.len(), ptr::null_mut()) };
    if !arena.is_null() {
        // SAFETY: `arena` is non-null and was just returned by `mrs_load`, so it is a live arena until `mrs_arena_free`.
        let loaded = unsafe { &*arena };
        walk(loaded, mrs_root(loaded));
        let dumped = dump(loaded);
        assert_eq!(
            load_and_dump(&dumped).as_deref(),
            Some(dumped.as_slice()),
            "dump does not round-trip"
        );
        // SAFETY: `arena` is the non-null handle from `mrs_load`, and `loaded` is not used after this.
        unsafe { mrs_arena_free(arena) };
    }

    build(data);
});
