# FFI test

`test_ffi.c` exercises the C API against real Marshal files - scalars, hash/array/object traversal, error codes on malformed input, and a byte-exact dump round-trip.

Build and run it by hand:

```bash
cd crates/marshal-rs-capi
cargo cinstall --release --prefix="$PWD/target/capi-install"
```

**Linux/macOS:**

```bash
clang tests/test_ffi.c -I target/capi-install/include -L target/capi-install/lib -l marshal_rs -o target/capi-install/test_ffi
LD_LIBRARY_PATH=target/capi-install/lib ./target/capi-install/test_ffi path/to/Map001.rvdata2   # DYLD_LIBRARY_PATH on macOS
```

**Windows (MSVC):**

```bash
clang tests/test_ffi.c -I target/capi-install/include -L target/capi-install/lib -l marshal_rs.dll -o target/capi-install/test_ffi.exe
# put target/capi-install/bin on PATH (or copy the dll next to the exe) so the loader finds it
./target/capi-install/test_ffi.exe path/to/Map001.rvdata2
```

With no file argument it runs only the tests that need no game files.
