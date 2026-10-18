#![cfg(target_arch = "wasm32")]

// Runs in a browser, for `wasm-pack test --headless --chrome`.
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

mod common;
