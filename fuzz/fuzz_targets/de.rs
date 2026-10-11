#![no_main]
use libfuzzer_sys::fuzz_target;
use marshal_rs::{dump, load, Arena};

fuzz_target!(|data: &[u8]| {
    if let Ok(arena) = serde_json::from_slice::<Arena<'static>>(data) {
        let bytes = dump(&arena);
        let _ = load(&bytes);
    }
});
