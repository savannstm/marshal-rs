#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(arena) = marshal_rs::load(data) {
        let _ = serde_json::to_string(&arena);
    }
});
