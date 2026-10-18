# FFI test

`c_api.c` exercises the C API against real Marshal files: scalars, hash/array/object traversal, error codes on malformed input, and a byte-exact dump round-trip. With no file argument it runs only the tests that need no game files.

```bash
cd capi
cargo cinstall --release --prefix="$PWD/target/capi-install"
```

```bash
# Linux/macOS (DYLD_LIBRARY_PATH on macOS)
clang tests/c_api.c -I target/capi-install/include -L target/capi-install/lib -l marshal_rs -o target/capi-install/c_api
LD_LIBRARY_PATH=target/capi-install/lib ./target/capi-install/c_api Map001.rvdata2

# Windows (clang targeting MSVC); put target/capi-install/bin on PATH so the loader finds the dll
clang tests/c_api.c -I target/capi-install/include -L target/capi-install/lib -l marshal_rs.dll -o target/capi-install/c_api.exe
./target/capi-install/c_api.exe Map001.rvdata2
```
