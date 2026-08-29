# marshal-rs-wasm

WebAssembly bindings for [`marshal-rs`](https://github.com/savannstm/marshal-rs), a Ruby-lang `Marshal` implementation, generated via [`wasm-bindgen`](https://github.com/wasm-bindgen/wasm-bindgen).

Ruby's Marshal format has no fixed schema, so `marshal-rs`'s `Arena` DOM isn't wrapped struct-by-struct on the JS side. Instead it crosses the boundary as a plain JavaScript value via [`serde-wasm-bindgen`](https://github.com/RReverser/serde-wasm-bindgen), reusing the root crate's `serde` feature: an `Arena` serializes to (and deserializes from) a self-describing, JSON-shaped envelope. The value is untyped (`any`) on the TypeScript side.

## Install

```bash
npm install marshal-rs-wasm
```

## API

```ts
function load(bytes: Uint8Array): any;
function dump(value: any): Uint8Array;
```

`load` parses a Marshal 4.8 byte stream into the envelope described below; it throws if `bytes` isn't well-formed. `dump` serializes an envelope value back to Marshal bytes; it throws if `value` isn't shaped like one.

### Envelope shape

`nil`/`true`/`false`/Fixnum decode as bare JS primitives (`null`/`true`/`false`/a number). Every other value becomes a plain object:

```js
{ __type: "array", __class: "MyArray", __flags: ["user_class"], __value: [1, 2, 3] }
```

- `__type` (always present): one of `bignum`, `float`, `bytes`, `str`, `symbol`, `regexp`, `array`, `hash`, `struct`, `object`, `class`, `module`.
- `__class` (omitted if none): the declared class/module name.
- `__flags` (omitted if empty): any of `old_module`, `user_class`, `data`, `user_marshal`, `user_defined`.
- `__extensions` (omitted if empty): `Module#extend`ed module names.
- `__encoding` (`str`/`regexp` only, omitted for the default `ASCII-8BIT`): the declared encoding's name (e.g. `"UTF-8"`, `"Shift_JIS"`) - this library never transcodes, so a `str`'s `__value` is exactly the original bytes, tagged with whatever encoding they were declared as.
- `__value`: the kind-specific payload. A hash's is an array of `[key, value]` pairs (not a JS object - hash keys aren't always strings). A `str`'s is a plain JS string when its bytes happen to validate as UTF-8 (regardless of `__encoding`), or a byte array otherwise - `dump` accepts either shape back.
- `__members` (struct/object only, instead of `__value`): an array of `[name, value]` pairs.
- `__default` (hash only, when present): the hash's default value.

Object links and cycles are not preserved across the JS boundary - shared/self-referential Ruby objects are flattened into independent copies on `load`, and `dump` always emits fresh objects.

## Usage

```ts
import init, { load, dump } from "marshal-rs-wasm";

await init(); // browsers & Deno: no-arg init() works out of the box

const bytes = new Uint8Array(await Deno.readFile("./data.rvdata2"));
const value = load(bytes);
console.log(value); // e.g. { __type: "array", __value: [1, { __type: "str", __value: "two" }] }

value.__value.push({ __type: "str", __value: "three" });
const out = dump(value);
```

Under Node/Bun, `init()`'s default `fetch()`-based loading doesn't apply - pass the `.wasm` bytes explicitly instead:

```ts
import { readFile } from "node:fs/promises";
import init, { load, dump } from "marshal-rs-wasm";

await init(
  await readFile(new URL("./marshal_rs_wasm_bg.wasm", import.meta.resolve("marshal-rs-wasm"))),
);
```

## Building

```bash
wasm-pack build --release --target web
```

Requires the `wasm32-unknown-unknown` rustup target (`rustup target add wasm32-unknown-unknown`) and `wasm-pack` (`cargo binstall wasm-pack`). Output goes to `pkg/` (gitignored): the compiled `.wasm`, a JS glue module, a `.d.ts`, and the `package.json` this crate publishes to npm from.

## Testing

```bash
wasm-pack test --node
```

Exercises the `serde-wasm-bindgen` conversion boundary against a real Marshal byte fixture (reused from the root crate's own verified test suite) - see [`tests/smoke.rs`](https://github.com/savannstm/marshal-rs/blob/master/crates/marshal-rs-wasm/tests/smoke.rs).
