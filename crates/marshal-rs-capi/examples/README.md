# C API examples

Build the library first, then compile the example against it by hand.

## `dump_to_json.c`

Loads a Marshal file through the C API and writes it out as JSON, using [yyjson](https://github.com/ibireme/yyjson) (vendored here as `yyjson/yyjson.c`/`yyjson/yyjson.h`, MIT licensed - see `yyjson/LICENSE`).

Built against a genuinely `no_std` `marshal-rs-capi` (`--no-default-features`), registering `malloc`/`free`-backed hooks via `mrs_set_allocator` before making any other allocating call - see the top-level README's [`no_std` / FFI](../../../README.md#no_std--ffi) section for the allocator-hook contract.

```bash
cd crates/marshal-rs-capi
cargo cinstall --release --no-default-features --prefix="$PWD/target/capi-install"
```

**Linux/macOS:**

```bash
clang -std=c11 examples/dump_to_json.c \
    examples/yyjson/yyjson.c -I target/capi-install/include \
    -L target/capi-install/lib -l marshal_rs -o target/capi-install/dump_to_json
LD_LIBRARY_PATH=target/capi-install/lib ./target/capi-install/dump_to_json path/to/Map001.rvdata2 path/to/Map001.json
```

**Windows (MSVC):**

```bash
clang -std=c11 examples/dump_to_json.c \
    examples/yyjson/yyjson.c -I target/capi-install/include \
    -L target/capi-install/lib -l marshal_rs.dll -o target/capi-install/dump_to_json.exe
./target/capi-install/dump_to_json.exe path/to/Map001.rvdata2 path/to/Map001.json
```
