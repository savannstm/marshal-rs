#![no_main]
//! Whatever the serde envelope writes must read back to the same JSON. JSON has no object links, so shared values
//! come back as copies and their dump can differ; the JSON text itself must not change.
use libfuzzer_sys::fuzz_target;
use marshal_rs::{Arena, load};
use serde::Deserialize;

fuzz_target!(|data: &[u8]| {
    if let Ok(arena) = load(data) {
        // Deep and cyclic values are refused by design; only what serializes must come back.
        let Ok(json) = serde_json::to_string(&arena) else {
            return;
        };
        // The recursion limit is lifted so that depth serde_json would refuse by default does not fail the fuzzer.
        let mut de = serde_json::Deserializer::from_str(&json);
        de.disable_recursion_limit();
        let back = Arena::<'static>::deserialize(&mut de).expect("own JSON must parse");
        de.end().expect("own JSON must end");
        let again = serde_json::to_string(&back).expect("own JSON must serialize");
        assert!(again == json, "JSON round trip changed the value");
    }
});
