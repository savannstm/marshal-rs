#![no_main]
use libfuzzer_sys::fuzz_target;
use marshal_rs::{dump, load, load_many, Arena};

/// Walks every value reachable from the root without recursion (the arena may be cyclic or very deep).
fn walk(arena: &Arena<'_>) -> usize {
    let mut seen = vec![false; arena.len()];
    let mut stack = vec![arena.root()];
    let mut visited = 0;

    while let Some(id) = stack.pop() {
        if std::mem::replace(&mut seen[id as usize], true) {
            continue;
        }
        visited += 1;

        let value = marshal_rs::ValueRef::new(arena, id);
        let _ = (value.kind(), value.class_name(), value.as_i64(), value.as_f64(), value.as_str());
        let _ = (value.as_bigint_decimal(), value.encoding_name());
        stack.extend(value.array().map(|child| child.id()));
        stack.extend(value.entries().flat_map(|(key, value)| [key.id(), value.id()]));
        stack.extend(value.members().map(|(_, value)| value.id()));
    }
    visited
}

fuzz_target!(|data: &[u8]| {
    let _ = load_many(data);

    let Ok(arena) = load(data) else {
        return;
    };
    walk(&arena);

    // Whatever loads must dump to something that loads again and dumps to the same bytes.
    let first = dump(&arena);
    let reloaded = load(&first).expect("own output must load");
    assert!(dump(&reloaded) == first, "dump is not stable");

    drop(arena.into_owned());
});
