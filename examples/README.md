# Examples

Run any of these with `cargo run --example <name> [--features ...] -- <args>`.

## `basic.rs`

Loads a hand-built Marshal byte stream, reads a few fields through `ValueRef`, and dumps it back to confirm a byte-exact round-trip.

```bash
cargo run --example basic
```

## `yaml.rs`

Loads a Marshal file and re-emits it as YAML (via `serde_norway`, over the same envelope the `serde`-feature JSON support uses - see `src/ser.rs`).

```bash
cargo run --example yaml -- path/to/Map001.rvdata2 path/to/Map001.yaml
```

## `fixture_check.rs`

Loads every real RPG Maker (XP/VX/VXAce) data file under a directory, dumps it back, and reports whether the round-trip is byte-exact. Pass a second directory to also write every re-dumped file there (e.g. for comparison against a reference Ruby re-dump).

```bash
cargo run --release --example fixture_check -- <dir> [out_dir]
```

## `crates/marshal-rs-capi/examples/dump_to_json.c`

The C API's example - see [`crates/marshal-rs-capi/examples/README.md`](../crates/marshal-rs-capi/examples/README.md).
