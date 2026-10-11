# FFI test

`test_ffi.c` exercises the C API against real Marshal files: scalars, hash/array/object traversal, error codes on malformed input, and a byte-exact dump round-trip. With no file argument it runs only the tests that need no game files.

```bash
cd capi
cargo cinstall --release --prefix="$PWD/target/capi-install"
```

```bash
# Linux/macOS (DYLD_LIBRARY_PATH on macOS)
clang tests/test_ffi.c -I target/capi-install/include -L target/capi-install/lib -l marshal_rs -o target/capi-install/test_ffi
LD_LIBRARY_PATH=target/capi-install/lib ./target/capi-install/test_ffi Map001.rvdata2

# Windows (clang targeting MSVC); put target/capi-install/bin on PATH so the loader finds the dll
clang tests/test_ffi.c -I target/capi-install/include -L target/capi-install/lib -l marshal_rs.dll -o target/capi-install/test_ffi.exe
./target/capi-install/test_ffi.exe Map001.rvdata2
```
