#![no_main]
use libfuzzer_sys::fuzz_target;
use marshal_rs::{dump, load, Arena};

fuzz_target!(|data: &[u8]| {
    if let Ok(arena) = serde_json::from_slice::<Arena<'static>>(data) {
        // Whatever the envelope reader accepts must dump to a stream that loads and dumps to the same bytes.
        let bytes = dump(&arena);
        let reloaded = load(&bytes).expect("own output must load");
        assert!(dump(&reloaded) == bytes, "dump is not stable");
    }
});
