#![no_main]
//! Whatever the serde envelope writes must read back to the same JSON. JSON has no object links, so shared values
//! come back as copies and their dump can differ; the JSON text itself must not change.
use libfuzzer_sys::fuzz_target;
use marshal_rs::{Arena, load};

fuzz_target!(|data: &[u8]| {
    if let Ok(arena) = load(data) {
        // Deep and cyclic values are refused by design; only what serializes must come back.
        let Ok(json) = serde_json::to_string(&arena) else {
            return;
        };
        let back: Arena<'static> = serde_json::from_str(&json).expect("own JSON must parse");
        let again = serde_json::to_string(&back).expect("own JSON must serialize");
        assert!(again == json, "JSON round trip changed the value");
    }
});
