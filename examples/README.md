# Examples

Run any of these with `cargo run --example <name> [--features ...] -- <args>`.

## `basic.rs`

Loads a hand-built Marshal byte stream, reads a few fields through `ValueRef`, and dumps it back to confirm a byte-exact round trip.

```bash
cargo run --example basic
```

## `yaml.rs`

Loads a Marshal file and re-emits it as YAML, over the same envelope as the `serde` JSON support (see `src/ser.rs`).

```bash
cargo run --example yaml -- path/to/Map001.rvdata2 path/to/Map001.yaml
```

## `fixture_check.rs`

Loads every RPG Maker (XP/VX/VXAce) data file under a directory, dumps it back, and reports whether the round trip is byte-exact. A second directory also receives every re-dumped file.

```bash
cargo run --release --example fixture_check -- <dir> [out_dir]
```
