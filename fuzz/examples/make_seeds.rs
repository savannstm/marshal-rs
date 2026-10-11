//! JSON seeds for the `de` target, made from the real Marshal seeds. Run from `fuzz/`: `cargo run --example make_seeds`
fn main() {
    std::fs::create_dir_all("corpus/de").unwrap();
    for entry in std::fs::read_dir("corpus/load").unwrap().flatten() {
        let bytes = std::fs::read(entry.path()).unwrap();
        if bytes.len() > 4000 {
            continue;
        }
        if let Ok(arena) = marshal_rs::load(&bytes) {
            if let Ok(json) = serde_json::to_string(&arena) {
                std::fs::write(format!("corpus/de/{}.json", entry.file_name().to_string_lossy()), json).unwrap();
            }
        }
    }
    for (name, text) in [("bignum", "123456789012345678901234567890"), ("negative", "-42")] {
        let _ = std::fs::write(format!("corpus/bignum/{name}"), text);
    }
}
