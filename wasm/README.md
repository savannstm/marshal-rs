# marshal-rs-wasm

WebAssembly bindings for [`marshal-rs`](https://github.com/savannstm/marshal-rs), a Ruby `Marshal` implementation.

## Install

```bash
npm install marshal-rs-wasm
```

## API

```ts
function load(bytes: Uint8Array): any;
function dump(value: any): Uint8Array;
```

`load` parses a Marshal 4.8 stream into the envelope below and throws on malformed bytes; `dump` serializes an envelope back and throws if `value` isn't shaped like one.

### Envelope shape

`nil`/`true`/`false`/Fixnum decode as bare JS primitives (`null`/`true`/`false`/a number). Every other value becomes a plain object:

```js
{ __type: "array", __class: "MyArray", __flags: ["user_class"], __value: [1, 2, 3] }
```

- `__type` (always present): one of `bignum`, `float`, `bytes`, `str`, `symbol`, `regexp`, `array`, `hash`, `struct`, `object`, `class`, `module`.
- `__class` (omitted if none): the declared class/module name.
- `__flags` (omitted if empty): any of `old_module`, `user_class`, `data`, `user_marshal`, `user_defined`.
- `__extensions` (omitted if empty): `Module#extend`ed module names.
- `__encoding` (`str`/`regexp` only, omitted for the default `ASCII-8BIT`): the declared encoding's name (e.g. `"UTF-8"`, `"Shift_JIS"`). Nothing is transcoded, so a `str`'s `__value` is exactly the original bytes.
- `__value`: the kind-specific payload. A hash's is an array of `[key, value]` pairs (not a JS object - keys aren't always strings). A `str`'s is a plain JS string when its bytes validate as UTF-8 (regardless of `__encoding`), or a byte array otherwise; `dump` accepts either shape back.
- `__members` (struct/object only, instead of `__value`): an array of `[name, value]` pairs.
- `__default` (hash only, when present): the hash's default value.

Object links and cycles are not preserved: shared or self-referential Ruby objects become independent copies on `load`, and `dump` always emits fresh objects.

## Usage

```ts
import init, { load, dump } from "marshal-rs-wasm";

await init(); // browsers & Deno

const bytes = new Uint8Array(await Deno.readFile("./data.rvdata2"));
const value = load(bytes);
console.log(value); // e.g. { __type: "array", __value: [1, { __type: "str", __value: "two" }] }

value.__value.push({ __type: "str", __value: "three" });
const out = dump(value);
```

Under Node/Bun, pass the `.wasm` bytes to `init()` explicitly:

```ts
import { readFile } from "node:fs/promises";
import init, { load, dump } from "marshal-rs-wasm";

await init(
  await readFile(new URL("./marshal_rs_wasm_bg.wasm", import.meta.resolve("marshal-rs-wasm"))),
);
```

## Build from source

```bash
wasm-pack build --release --target web   # output in pkg/
```
