# C API examples

## `dump_to_json.c`

Loads a Marshal file through the C API and writes it out as JSON with [yyjson](https://github.com/ibireme/yyjson) (vendored in `yyjson/`, MIT licensed). It uses a `no_std` build and registers `malloc`/`free`-backed hooks with `mrs_set_allocator` first.

```bash
cd capi
cargo cinstall --release --no-default-features --prefix="$PWD/target/capi-install"
```

```bash
# Linux/macOS
clang -std=c11 examples/dump_to_json.c examples/yyjson/yyjson.c -I target/capi-install/include \
    -L target/capi-install/lib -l marshal_rs -o target/capi-install/dump_to_json
LD_LIBRARY_PATH=target/capi-install/lib ./target/capi-install/dump_to_json Map001.rvdata2 Map001.json

# Windows (clang targeting MSVC)
clang -std=c11 examples/dump_to_json.c examples/yyjson/yyjson.c -I target/capi-install/include \
    -L target/capi-install/lib -l marshal_rs.dll -o target/capi-install/dump_to_json.exe
./target/capi-install/dump_to_json.exe Map001.rvdata2 Map001.json
```
