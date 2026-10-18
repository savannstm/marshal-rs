# marshal-rs-wasm

WebAssembly bindings for [`marshal-rs`](https://github.com/savannstm/marshal-rs), a Ruby `Marshal` implementation.

> **I won't be continuing to publish this package on NPM.** NPM's 2FA is unbearable, and I don't want to keep up with this logging-in shitshow. I'm looking for someone to take over the package on NPM. Until then, [build from source](#build-from-source).

## Install

Not published to NPM, so [build it from source](#build-from-source).

## API

```ts
function load(bytes: Uint8Array): MarshalValue;
function loadMany(bytes: Uint8Array): MarshalValue[];
function dump(value: MarshalValue): Uint8Array;
function dumpMany(values: MarshalValue[]): Uint8Array;
```

`load` parses one Marshal 4.8 stream into the envelope below; `loadMany` parses a file made of several concatenated streams (an RPG Maker save is one) into an array, and `dumpMany` is its inverse. `dump` serializes an envelope back. The types (`MarshalValue`, `MarshalEnvelope`, `MarshalError`) ship in the generated `.d.ts`.

Everything throws a `MarshalError`: an `Error` with a `code` (`unexpected_eof`, `invalid_header`, `invalid_tag`, `symbol_table_full`, `unknown_symbol_link`, `unknown_object_link`, `length_overflow`, `negative_length`, `trailing_data`, `empty_name`, `unsupported`, or `invalid_envelope` for something that is not an envelope) and, where there is one, the byte `offset`.

### Envelope shape

`nil`/`true`/`false`/Fixnum decode as bare JS primitives (`null`/`true`/`false`/a number). Every other value becomes a plain object:

```js
{ __type: "array", __class: "MyArray", __flags: ["user_class"], __value: [1, 2, 3] }
```

- `__type` (always present): one of `bignum`, `float`, `bytes`, `str`, `symbol`, `regexp`, `array`, `hash`, `struct`, `object`, `class`, `module`, `user_marshal`, `data`. The last two are `marshal_dump` / `_dump_data` wrappers: `__class` is the wrapper's class and `__value` is the envelope of the value they returned.
- `__class` (omitted if none): the declared class/module name.
- `__flags` (omitted if empty): any of `old_module`, `user_class`, `user_defined`; an unknown flag is an error.
- `__extensions` (omitted if empty): `Module#extend`ed module names, in stream order.
- `__ivars` (omitted if empty): instance variables besides the encoding, an array of `[name, value]` pairs.
- `__encoding` (`str`, `regexp` and `symbol` only; omitted for a string or regexp in the default `ASCII-8BIT`, and for a symbol whose tag is the one its bytes get anyway): the declared encoding's name (e.g. `"UTF-8"`, `"Shift_JIS"`). Nothing is transcoded, so a `str`'s `__value` is exactly the original bytes.
- `__value`: the kind-specific payload. A hash's is an array of `[key, value]` pairs (keys aren't always strings). A `str`, `float`, `symbol` or `class`/`module` is a JS string when its bytes are valid UTF-8, otherwise a `Uint8Array`. Names (`__class`, `__extensions`, member and ivar names) follow the same rule, and so does `__encoding`. A name whose encoding tag is not the default one is `{"__name": <name>, "__encoding": <name>}` instead. A `bytes` value and a `regexp` source are always `Uint8Array`. `dump` also accepts a base64 string or a number array.
- `__members` (struct/object only, instead of `__value`): an array of `[name, value]` pairs.
- `__default` (hash only, when present): the hash's default value.

Keys may come in any order.

Object links and cycles are not preserved: shared or self-referential Ruby objects become independent copies on `load`, and `dump` always emits fresh objects.

## Usage

```ts
import { readFile } from "node:fs/promises";
import init, { load, dump } from "marshal-rs-wasm";

await init(await readFile(new URL("./marshal_rs_wasm_bg.wasm", import.meta.resolve("marshal-rs-wasm"))));

const bytes = new Uint8Array(await readFile("./data.rvdata2"));
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
