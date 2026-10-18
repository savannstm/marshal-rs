//! Streaming `serde` `Serialize`/`Deserialize` for [`Arena`].
//!
//! Drives the serde `Serializer`/`Deserializer` directly, so any format works, not just JSON.
//!
//! `Nil`/`True`/`False`/`Fixnum` are bare primitives. Every other value is an object:
//!
//! ```json
//! {"__type": "array", "__class": "MyArray", "__flags": ["user_class"], "__value": [1, 2, 3]}
//! ```
//!
//! - `__type` (always present, always first): one of `bignum`, `float`,
//!   `bytes`, `str`, `symbol`, `regexp`, `array`, `hash`, `struct`,
//!   `object`, `class`, `module`, `user_marshal`, `data` (the last two are
//!   `marshal_dump` / `_dump_data` wrappers, whose `__value` is the body).
//! - `__class` (omitted if none): the declared class/module name.
//! - `__flags` (omitted if empty): any of `old_module`, `user_class`,
//!   `user_defined`.
//! - `__extensions` (omitted if empty): `Module#extend`ed module names.
//! - `__encoding` (`str`/`regexp`/`symbol` only, omitted for the default
//!   `ASCII-8BIT`, and for a symbol whose tag is the one its bytes get anyway - see
//!   [`default_symbol_encoding`](crate::encoding::default_symbol_encoding)): the declared encoding's name (e.g. `"UTF-8"`,
//!   `"Shift_JIS"`); nothing is transcoded, so a `str`'s `__value` is exactly
//!   its original bytes (see [`mod@crate::encoding`]).
//! - `__value`: the kind-specific payload - a hash's is a JSON array of
//!   `[key, value]` pairs (not a JSON object: hash keys aren't always
//!   strings). A `str`'s, `float`'s, `symbol`'s and `class`/`module`'s is plain
//!   text when its bytes happen to validate as UTF-8 (regardless of
//!   `__encoding`), or base64 text with `"__base64": true` (a byte array in
//!   [`BinaryMode::Bytes`]) otherwise - deserializing accepts either shape.
//!   Nothing is ever validated or replaced. A name (`__class`, `__extensions`,
//!   `__encoding`, member and ivar names) that is not UTF-8 is written as
//!   `{"__base64": "..."}` instead of a string.
//! - `__members` (struct/object only, instead of `__value`): a JSON array
//!   of `[name, value]` pairs.
//! - `__default` (hash only, when present): the hash's default value.
//!
//! Keys may come in any order. If `__type` is not first the object is buffered before it is read.
//! Object links and cycles are not preserved: shared structure becomes independent copies.

use crate::{
    arena::{Arena, Flags, Kind, SymId, ValueId},
    base64, bignum,
    value::ValueRef,
};
use alloc::{fmt, string::String, vec::Vec};
use core::cell::Cell;
use core::marker::PhantomData;
use serde::de::value::{MapDeserializer, SeqDeserializer};
use serde::de::{self, DeserializeSeed, Deserializer, IntoDeserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};

fn type_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Bignum => "bignum",
        Kind::Float => "float",
        Kind::Bytes => "bytes",
        Kind::Str => "str",
        Kind::Symbol => "symbol",
        Kind::Regexp => "regexp",
        Kind::Array => "array",
        Kind::Hash => "hash",
        Kind::Struct => "struct",
        Kind::Object => "object",
        Kind::Class => "class",
        Kind::Module => "module",
        Kind::Nil | Kind::True | Kind::False | Kind::Fixnum => {
            unreachable!("trivial kinds serialize as bare primitives")
        }
    }
}

/// A name (`__class`, member, ivar). Plain text when its bytes are UTF-8, else `{"__base64": ...}`.
/// A name with a non-default encoding tag is wrapped as `{"__name" | "__base64", "__encoding": ...}`.
struct Name<'b>(&'b [u8], BinaryMode, Option<&'b [u8]>);

impl Name<'_> {
    /// The bytes alone, without a tag.
    fn bare<S: Serializer>(bytes: &[u8], mode: BinaryMode, serializer: S) -> Result<S::Ok, S::Error> {
        if let Ok(text) = core::str::from_utf8(bytes) {
            return serializer.serialize_str(text);
        }
        match mode {
            BinaryMode::Base64 => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("__base64", &base64::encode(bytes))?;
                map.end()
            }
            BinaryMode::Bytes => serializer.serialize_bytes(bytes),
        }
    }
}

/// The bytes of a name, in the shape [`Name::bare`] gives them, as one entry of a tagged name.
struct BareName<'b>(&'b [u8], BinaryMode);
impl Serialize for BareName<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Name::bare(self.0, self.1, serializer)
    }
}

impl Serialize for Name<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Some(encoding) = self.2 else {
            return Name::bare(self.0, self.1, serializer);
        };
        let mut map = serializer.serialize_map(Some(2))?;
        match (core::str::from_utf8(self.0), self.1) {
            (Err(_), BinaryMode::Base64) => map.serialize_entry("__base64", &base64::encode(self.0))?,
            _ => map.serialize_entry("__name", &BareName(self.0, self.1))?,
        }
        map.serialize_entry("__encoding", &BareName(encoding, self.1))?;
        map.end()
    }
}

/// Reads a [`Name`] back, whichever shape it was written in.
struct NameBuf {
    bytes: Vec<u8>,
    encoding: Option<Vec<u8>>,
}

impl NameBuf {
    const fn bare(bytes: Vec<u8>) -> Self {
        Self { bytes, encoding: None }
    }
}

impl<'de> serde::Deserialize<'de> for NameBuf {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(NameVisitor)
    }
}

struct NameVisitor;
impl<'de> Visitor<'de> for NameVisitor {
    type Value = NameBuf;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a name: a string, {{\"__base64\": ...}} or an array of bytes")
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<NameBuf, E> {
        Ok(NameBuf::bare(v.as_bytes().to_vec()))
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<NameBuf, E> {
        Ok(NameBuf::bare(v.into_bytes()))
    }
    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<NameBuf, E> {
        Ok(NameBuf::bare(v.to_vec()))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<NameBuf, A::Error> {
        BytesVisitor.visit_seq(seq).map(NameBuf::bare)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<NameBuf, A::Error> {
        const SHAPE: &str = "a name object has the name (\"__name\" or \"__base64\") and optionally an \"__encoding\"";
        let mut bytes = None;
        let mut encoding = None;
        while let Some(key) = map.next_key::<String>()? {
            match (key.as_str(), bytes.is_some(), encoding.is_some()) {
                ("__base64", false, _) => {
                    bytes = Some(
                        base64::decode(&map.next_value::<String>()?)
                            .ok_or_else(|| de::Error::custom("invalid base64"))?,
                    );
                }
                ("__name", false, _) => bytes = Some(map.next_value::<NameBuf>()?.bytes),
                ("__encoding", _, false) => encoding = Some(map.next_value::<NameBuf>()?.bytes),
                _ => return Err(de::Error::custom(SHAPE)),
            }
        }
        bytes
            .map(|bytes| NameBuf { bytes, encoding })
            .ok_or_else(|| de::Error::custom(SHAPE))
    }
}

fn flag_names(v: ValueRef<'_, '_>) -> Vec<&'static str> {
    let mut out = Vec::new();
    if v.is_old_module() {
        out.push("old_module");
    }
    if v.is_user_class() {
        out.push("user_class");
    }
    if v.is_user_defined() {
        out.push("user_defined");
    }
    out
}

/// Maximum envelope nesting, in either direction. Serialization and deserialization both recurse,
/// and a cyclic link would otherwise never stop.
pub const MAX_NESTING: usize = 256;

/// Average visits allowed per arena value. Shared links can make a tiny stream expand to 2^60 values.
const SERIALIZE_EXPANSION: usize = 64;

/// Floor for [`SERIALIZE_EXPANSION`]'s limit, so small values always have room.
const MIN_SERIALIZE_NODES: usize = 1 << 16;

/// What one top-level serialization has used so far, and how much it may.
struct Budget {
    depth: Cell<usize>,
    nodes: Cell<usize>,
    max_nodes: usize,
    binary: BinaryMode,
}

impl Budget {
    fn for_arena(arena_len: usize, binary: BinaryMode) -> Self {
        Self {
            depth: Cell::new(0),
            nodes: Cell::new(0),
            max_nodes: arena_len.saturating_mul(SERIALIZE_EXPANSION).max(MIN_SERIALIZE_NODES),
            binary,
        }
    }
}

/// How binary payloads (`bytes`, a non-UTF-8 `str`, a regexp source) are written.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BinaryMode {
    /// A base64 string: compact and readable in text formats such as JSON and YAML. The default.
    #[default]
    Base64,
    /// The format's own byte type (`serialize_bytes`): a `Uint8Array` in `serde-wasm-bindgen`, a byte
    /// string in CBOR or `MessagePack`. Text formats write it as an array of numbers, so only pick it for a
    /// format that has one.
    Bytes,
}

/// An [`Arena`] to serialize with a choice of [`BinaryMode`]; plain `Arena` serialization uses
/// [`BinaryMode::Base64`]. Deserializing accepts either form, whichever was used to write it.
pub struct Envelope<'x, 'a> {
    pub arena: &'x Arena<'a>,
    pub binary: BinaryMode,
}

impl Serialize for Envelope<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let budget = Budget::for_arena(self.arena.len(), self.binary);
        Bounded {
            value: ValueRef::root(self.arena),
            budget: &budget,
        }
        .serialize(serializer)
    }
}

/// Serializes as the format's byte type.
struct RawBytes<'b>(&'b [u8]);

impl Serialize for RawBytes<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self.0)
    }
}

/// Holds a level of [`Budget::depth`] until dropped.
struct Level<'s>(&'s Budget);

impl<'s> Level<'s> {
    fn enter(budget: &'s Budget) -> Result<Self, &'static str> {
        let nodes = budget.nodes.get() + 1;
        let depth = budget.depth.get() + 1;
        if nodes > budget.max_nodes {
            return Err("too many values to serialize (shared values expand a stream into a huge tree)");
        }
        if depth > MAX_NESTING {
            return Err("nested too deeply to serialize (or it refers back to itself)");
        }
        budget.nodes.set(nodes);
        budget.depth.set(depth);
        Ok(Self(budget))
    }
}

impl Drop for Level<'_> {
    fn drop(&mut self) {
        self.0.depth.set(self.0.depth.get() - 1);
    }
}

impl Serialize for Arena<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ValueRef::root(self).serialize(serializer)
    }
}

/// Errors on nesting deeper than [`MAX_NESTING`] or on expansion beyond the budget, instead of overflowing or hanging.
impl Serialize for ValueRef<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let budget = Budget::for_arena(self.arena().len(), BinaryMode::Base64);
        Bounded {
            value: *self,
            budget: &budget,
        }
        .serialize(serializer)
    }
}

/// A [`ValueRef`] being serialized as part of a call that is counting its [`Budget`].
#[derive(Clone, Copy)]
struct Bounded<'s, 'r, 'a> {
    value: ValueRef<'r, 'a>,
    budget: &'s Budget,
}

impl<'r, 'a> Bounded<'_, 'r, 'a> {
    /// Writes `__value` as the binary payload `bytes`, in the mode the serialization was asked for.
    fn serialize_binary<M: SerializeMap>(&self, map: &mut M, bytes: &[u8]) -> Result<(), M::Error> {
        match self.budget.binary {
            BinaryMode::Base64 => map.serialize_entry("__value", &base64::encode(bytes)),
            BinaryMode::Bytes => map.serialize_entry("__value", &RawBytes(bytes)),
        }
    }

    /// `__extensions` and `__ivars`, the parts of an envelope any kind of value can have.
    fn write_extras<M: SerializeMap>(&self, map: &mut M) -> Result<(), M::Error> {
        let value = self.value;
        let extensions: Vec<Name<'_>> = value.extension_syms().map(|module| self.sym_name(module)).collect();
        if !extensions.is_empty() {
            map.serialize_entry("__extensions", &extensions)?;
        }
        if value.extra_ivars().next().is_some() {
            map.serialize_entry("__ivars", &IvarsSeq(*self))?;
        }
        Ok(())
    }

    const fn name<'n>(&self, bytes: &'n [u8]) -> Name<'n> {
        Name(bytes, self.budget.binary, None)
    }

    /// The name `sym` stands for, with its encoding tag when it has an unusual one.
    fn sym_name(&self, sym: crate::arena::SymId) -> Name<'r> {
        let (bytes, tag) = self.value.name_of(sym);
        Name(bytes, self.budget.binary, tag)
    }

    const fn child(&self, value: ValueRef<'r, 'a>) -> Self {
        Self {
            value,
            budget: self.budget,
        }
    }
}

impl Serialize for Bounded<'_, '_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let _level = Level::enter(self.budget).map_err(serde::ser::Error::custom)?;
        let value = self.value;

        // A `marshal_dump`/`_dump_data` wrapper holds its own class around the value it dumped.
        if let Some(body) = value.body() {
            let mut map = serializer.serialize_map(None)?;
            map.serialize_entry("__type", if value.is_data() { "data" } else { "user_marshal" })?;
            map.serialize_entry("__class", &value.class_sym().map(|class| self.sym_name(class)))?;
            self.write_extras(&mut map)?;
            map.serialize_entry("__value", &self.child(body))?;
            return map.end();
        }

        match value.kind() {
            Kind::Nil => serializer.serialize_unit(),
            Kind::True => serializer.serialize_bool(true),
            Kind::False => serializer.serialize_bool(false),
            Kind::Fixnum => serializer.serialize_i64(value.as_i64().unwrap()),
            kind => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("__type", type_name(kind))?;
                if let Some(class) = value.class_sym() {
                    map.serialize_entry("__class", &self.sym_name(class))?;
                }
                let flags = flag_names(value);
                if !flags.is_empty() {
                    map.serialize_entry("__flags", &flags)?;
                }
                self.write_extras(&mut map)?;
                if matches!(kind, Kind::Str | Kind::Regexp) {
                    let encoding = value
                        .encoding_id()
                        .filter(|&id| id != crate::encoding::ENCODING_ASCII_8BIT)
                        .and_then(|_| value.encoding_name());
                    if let Some(name) = encoding {
                        map.serialize_entry("__encoding", &self.name(name))?;
                    }
                } else if let (Kind::Symbol, Some(tag), Some(bytes)) =
                    (kind, value.symbol_tag(), value.as_symbol_bytes())
                {
                    // Only a tag other than the one the bytes would be given anyway is worth saying; that
                    // includes "none" (`ASCII-8BIT`) for binary bytes that happen to be UTF-8.
                    if tag != crate::encoding::default_symbol_encoding(bytes) {
                        let name = if tag == crate::encoding::ENCODING_CUSTOM {
                            value.encoding_name()
                        } else {
                            crate::encoding::encoding_name(tag)
                        };
                        if let Some(name) = name {
                            map.serialize_entry("__encoding", &self.name(name))?;
                        }
                    }
                }
                match kind {
                    Kind::Bignum => {
                        let decimal = value
                            .as_bigint_decimal()
                            .ok_or_else(|| serde::ser::Error::custom("bignum too large to convert to decimal"))?;
                        map.serialize_entry("__value", &decimal)?;
                    }
                    // Binary payloads travel as base64; a byte array costs 3-4 characters per byte in JSON.
                    Kind::Bytes => self.serialize_binary(&mut map, value.as_bytes().unwrap())?,
                    // Plain text when the bytes validate as UTF-8 (the common case), else base64 flagged `__base64`.
                    // Floats, symbols and class paths take the same path: their bytes may not be UTF-8.
                    Kind::Str | Kind::Float | Kind::Symbol | Kind::Class | Kind::Module => {
                        let bytes = match kind {
                            Kind::Float => value.as_float_bytes(),
                            Kind::Symbol => value.as_symbol_bytes(),
                            Kind::Class | Kind::Module => value.as_path(),
                            _ => value.as_bytes(),
                        }
                        .unwrap();
                        if let Ok(text) = core::str::from_utf8(bytes) {
                            map.serialize_entry("__value", text)?;
                        } else {
                            self.serialize_binary(&mut map, bytes)?;
                            if self.budget.binary == BinaryMode::Base64 {
                                map.serialize_entry("__base64", &true)?;
                            }
                        }
                    }
                    Kind::Regexp => {
                        let (source, options) = value.as_regexp().unwrap();
                        match self.budget.binary {
                            BinaryMode::Base64 => map.serialize_entry("__value", &(base64::encode(source), options))?,
                            BinaryMode::Bytes => map.serialize_entry("__value", &(RawBytes(source), options))?,
                        }
                    }
                    Kind::Array => {
                        map.serialize_entry("__value", &ArraySeq(*self))?;
                    }
                    Kind::Hash => {
                        map.serialize_entry("__value", &HashSeq(*self))?;
                        if let Some(default) = value.default_value() {
                            map.serialize_entry("__default", &self.child(default))?;
                        }
                    }
                    Kind::Struct | Kind::Object => {
                        map.serialize_entry("__members", &MembersSeq(*self))?;
                    }
                    Kind::Nil | Kind::True | Kind::False | Kind::Fixnum => {
                        unreachable!()
                    }
                }
                map.end()
            }
        }
    }
}

struct ArraySeq<'s, 'r, 'a>(Bounded<'s, 'r, 'a>);
impl Serialize for ArraySeq<'_, '_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.value.len()))?;
        for item in self.0.value.array() {
            seq.serialize_element(&self.0.child(item))?;
        }
        seq.end()
    }
}

struct HashSeq<'s, 'r, 'a>(Bounded<'s, 'r, 'a>);
impl Serialize for HashSeq<'_, '_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.value.len()))?;
        for (k, v) in self.0.value.entries() {
            seq.serialize_element(&(self.0.child(k), self.0.child(v)))?;
        }
        seq.end()
    }
}

struct IvarsSeq<'s, 'r, 'a>(Bounded<'s, 'r, 'a>);
impl Serialize for IvarsSeq<'_, '_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(None)?;
        for (name, value) in self.0.value.extra_ivar_syms() {
            let name = self.0.sym_name(name);
            // The reader refuses an empty ivar name, so the envelope can't carry one.
            if name.0.is_empty() {
                return Err(serde::ser::Error::custom("an instance variable has an empty name"));
            }
            seq.serialize_element(&(name, self.0.child(value)))?;
        }
        seq.end()
    }
}

struct MembersSeq<'s, 'r, 'a>(Bounded<'s, 'r, 'a>);
impl Serialize for MembersSeq<'_, '_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.value.len()))?;
        for (name, value) in self.0.value.member_syms() {
            seq.serialize_element(&(self.0.sym_name(name), self.0.child(value)))?;
        }
        seq.end()
    }
}

impl<'de> serde::Deserialize<'de> for Arena<'static> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut ctx = Ctx {
            arena: Arena::builder(),
            depth: 0,
        };
        let root = ValueSeed { arena: &mut ctx }.deserialize(deserializer)?;
        ctx.arena.set_root(root);
        Ok(ctx.arena)
    }
}

/// The arena being built, and how deeply nested the envelope being read currently is.
struct Ctx {
    arena: Arena<'static>,
    depth: usize,
}

impl core::ops::Deref for Ctx {
    type Target = Arena<'static>;
    fn deref(&self) -> &Arena<'static> {
        &self.arena
    }
}

impl core::ops::DerefMut for Ctx {
    fn deref_mut(&mut self) -> &mut Arena<'static> {
        &mut self.arena
    }
}

struct ValueSeed<'x> {
    arena: &'x mut Ctx,
}

impl<'de> DeserializeSeed<'de> for ValueSeed<'_> {
    type Value = ValueId;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<ValueId, D::Error> {
        deserializer.deserialize_any(ValueVisitor { arena: self.arena })
    }
}

struct ValueVisitor<'x> {
    arena: &'x mut Ctx,
}

enum Payload {
    None,
    Text(String),
    Bytes(Vec<u8>),
    Regexp(Vec<u8>, u8),
    /// The single value inside a `data`/`user_marshal` wrapper.
    Id(ValueId),
    Ids(Vec<ValueId>),
    Pairs(Vec<(ValueId, ValueId)>),
    Members(Vec<(SymId, ValueId)>),
}

impl<'de> Visitor<'de> for ValueVisitor<'_> {
    type Value = ValueId;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a value produced by marshal-rs's serde envelope")
    }

    fn visit_unit<E: de::Error>(self) -> Result<ValueId, E> {
        Ok(self.arena.push_nil())
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<ValueId, E> {
        Ok(self.arena.push_bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<ValueId, E> {
        let v = i32::try_from(v).map_err(|_| de::Error::custom("fixnum out of i32 range"))?;
        Ok(self.arena.push_fixnum(v))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<ValueId, E> {
        let v = i32::try_from(v).map_err(|_| de::Error::custom("fixnum out of i32 range"))?;
        Ok(self.arena.push_fixnum(v))
    }

    #[allow(clippy::too_many_lines)] // one linear pass over the envelope's keys
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ValueId, A::Error> {
        let arena = self.arena;
        if arena.depth >= MAX_NESTING {
            return Err(de::Error::custom("nested too deeply to deserialize"));
        }
        arena.depth += 1;
        let result = (|| -> Result<ValueId, A::Error> {
            let Some(first_key) = map.next_key::<String>()? else {
                return Err(de::Error::custom("expected a non-empty envelope object"));
            };
            if first_key != "__type" {
                return envelope_out_of_order(arena, first_key, map);
            }
            let type_name: String = map.next_value()?;

            let mut class: Option<SymId> = None;
            let mut old_module = false;
            let mut user_class = false;
            let mut user_defined = false;
            let mut extensions: Vec<SymId> = Vec::new();
            let mut ivars: Vec<(SymId, ValueId)> = Vec::new();
            let mut payload = Payload::None;
            let mut default: Option<ValueId> = None;
            let mut encoding: Option<Vec<u8>> = None;
            let mut base64_text = false;

            while let Some(key) = map.next_key::<String>()? {
                match key.as_str() {
                    "__class" => {
                        let name = map.next_value::<NameBuf>()?;
                        class = Some(arena.arena.name_symbol(name.bytes, name.encoding.as_deref()));
                    }
                    "__flags" => {
                        for name in map.next_value::<Vec<String>>()? {
                            match name.as_str() {
                                "old_module" => old_module = true,
                                "user_class" => user_class = true,
                                "user_defined" => user_defined = true,
                                other => {
                                    return Err(de::Error::custom(alloc::format!("unknown flag \"{other}\"")));
                                }
                            }
                        }
                    }
                    "__extensions" => {
                        extensions = map
                            .next_value::<Vec<NameBuf>>()?
                            .into_iter()
                            .map(|name| arena.arena.name_symbol(name.bytes, name.encoding.as_deref()))
                            .collect();
                    }
                    "__value" => payload = deserialize_value_payload(&type_name, arena, &mut map)?,
                    "__ivars" => ivars = map.next_value_seed(MembersSeed { arena: &mut *arena })?,
                    "__members" => {
                        payload = Payload::Members(map.next_value_seed(MembersSeed { arena: &mut *arena })?);
                    }
                    "__default" => default = Some(map.next_value_seed(ValueSeed { arena: &mut *arena })?),
                    "__base64" => base64_text = map.next_value()?,
                    "__encoding" => encoding = Some(map.next_value::<NameBuf>()?.bytes),
                    _ => {
                        let _ = map.next_value::<de::IgnoredAny>()?;
                    }
                }
            }

            // `user_class` is Ruby's marker for a subclass of a built-in type, `user_defined` for a
            // `_dump` string; both need the class, and only fit these types.
            if user_class && !matches!(type_name.as_str(), "bytes" | "str" | "regexp" | "array" | "hash") {
                return Err(de::Error::custom(
                    "\"user_class\" is only valid on \"bytes\", \"str\", \"regexp\", \"array\" and \"hash\"",
                ));
            }
            if user_defined && !matches!(type_name.as_str(), "bytes" | "str") {
                return Err(de::Error::custom(
                    "\"user_defined\" is only valid on \"bytes\" and \"str\"",
                ));
            }
            if user_class && user_defined {
                return Err(de::Error::custom(
                    "\"user_class\" and \"user_defined\" exclude each other",
                ));
            }
            if (user_class || user_defined) && class.is_none() {
                return Err(de::Error::custom(
                    "\"user_class\" and \"user_defined\" need a \"__class\"",
                ));
            }

            // A `Str` that is not valid UTF-8 carries its bytes as base64 text.
            if base64_text {
                if let Payload::Text(text) = &payload {
                    payload = Payload::Bytes(base64::decode(text).ok_or_else(|| de::Error::custom("invalid base64"))?);
                }
            }

            let id = build_from_payload(
                arena,
                &type_name,
                class,
                old_module,
                default,
                encoding.as_deref(),
                payload,
            )
            .map_err(de::Error::custom)?;

            if user_class {
                if let Some(class) = class {
                    arena.set_user_class_sym(id, class);
                }
            }
            if user_defined {
                if let Some(class) = class {
                    arena.set_user_defined_sym(id, class);
                }
            }
            for module in extensions {
                if !arena.push_extension_sym(id, module) {
                    return Err(de::Error::custom("this value cannot have \"__extensions\""));
                }
            }
            for (name, value) in ivars {
                if !arena.push_extra_ivar_sym(id, name, value) {
                    return Err(de::Error::custom(
                        "this value cannot have \"__ivars\", or one has an empty name",
                    ));
                }
            }

            Ok(id)
        })();
        arena.depth -= 1;
        result
    }
}

/// An envelope whose `__type` is not its first key (JSON objects are unordered, and tools sort keys): what
/// `__value` means depends on the type, so the whole object is buffered, `__type` moved to the front,
/// and the entries read again in the usual way.
fn envelope_out_of_order<'de, A: MapAccess<'de>>(
    arena: &mut Ctx,
    first_key: String,
    mut map: A,
) -> Result<ValueId, A::Error> {
    let depth = arena.depth;
    let mut entries = Vec::new();
    entries.push((first_key, map.next_value_seed(RawSeed { depth })?));
    while let Some(key) = map.next_key::<String>()? {
        entries.push((key, map.next_value_seed(RawSeed { depth })?));
    }
    let at = entries
        .iter()
        .position(|(key, _)| key == "__type")
        .ok_or_else(|| de::Error::custom("envelope object has no \"__type\""))?;
    let type_entry = entries.remove(at);
    entries.insert(0, type_entry);

    // `visit_map` counts this level again; it was already counted by the caller.
    arena.depth -= 1;
    let result =
        ValueVisitor { arena: &mut *arena }.visit_map(MapDeserializer::<_, A::Error>::new(entries.into_iter()));
    arena.depth += 1;
    result
}

/// A buffered serde value, to be replayed through the usual `Deserialize` code.
enum Raw {
    Unit,
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    Str(String),
    Bytes(Vec<u8>),
    Seq(Vec<Self>),
    Map(Vec<(String, Self)>),
}

/// Reads any value into a [`Raw`], `depth` levels below the envelope root.
struct RawSeed {
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for RawSeed {
    type Value = Raw;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Raw, D::Error> {
        if self.depth >= MAX_NESTING {
            return Err(de::Error::custom("nested too deeply to deserialize"));
        }
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for RawSeed {
    type Value = Raw;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a value produced by marshal-rs's serde envelope")
    }
    fn visit_unit<E: de::Error>(self) -> Result<Raw, E> {
        Ok(Raw::Unit)
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Raw, E> {
        Ok(Raw::Bool(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Raw, E> {
        Ok(Raw::I64(v))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Raw, E> {
        Ok(Raw::U64(v))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Raw, E> {
        Ok(Raw::F64(v))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Raw, E> {
        Ok(Raw::Str(String::from(v)))
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Raw, E> {
        Ok(Raw::Str(v))
    }
    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Raw, E> {
        Ok(Raw::Bytes(v.to_vec()))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut out = Vec::new();
        while let Some(item) = seq.next_element_seed(Self { depth: self.depth + 1 })? {
            out.push(item);
        }
        Ok(Raw::Seq(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut out = Vec::new();
        while let Some(key) = map.next_key::<String>()? {
            out.push((key, map.next_value_seed(Self { depth: self.depth + 1 })?));
        }
        Ok(Raw::Map(out))
    }
}

impl<E: de::Error> IntoDeserializer<'_, E> for Raw {
    type Deserializer = RawDeserializer<E>;
    fn into_deserializer(self) -> RawDeserializer<E> {
        RawDeserializer {
            raw: self,
            error: PhantomData,
        }
    }
}

struct RawDeserializer<E> {
    raw: Raw,
    error: PhantomData<E>,
}

impl<'de, E: de::Error> Deserializer<'de> for RawDeserializer<E> {
    type Error = E;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, E> {
        match self.raw {
            Raw::Unit => visitor.visit_unit(),
            Raw::Bool(v) => visitor.visit_bool(v),
            Raw::I64(v) => visitor.visit_i64(v),
            Raw::U64(v) => visitor.visit_u64(v),
            Raw::F64(v) => visitor.visit_f64(v),
            Raw::Str(v) => visitor.visit_string(v),
            Raw::Bytes(v) => visitor.visit_byte_buf(v),
            Raw::Seq(v) => {
                let mut seq = SeqDeserializer::new(v.into_iter());
                let value = visitor.visit_seq(&mut seq)?;
                seq.end()?;
                Ok(value)
            }
            Raw::Map(v) => {
                let mut map = MapDeserializer::new(v.into_iter());
                let value = visitor.visit_map(&mut map)?;
                map.end()?;
                Ok(value)
            }
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option unit
        unit_struct newtype_struct seq tuple tuple_struct map struct enum identifier ignored_any
    }
}

fn deserialize_value_payload<'de, A: MapAccess<'de>>(
    type_name: &str,
    arena: &mut Ctx,
    map: &mut A,
) -> Result<Payload, A::Error> {
    Ok(match type_name {
        "bignum" => Payload::Text(map.next_value()?),
        // A text-like `__value` is plain text when its bytes happened to
        // validate as UTF-8 at serialize time, or binary otherwise (see the
        // `Serialize` impl) - accept either shape.
        "str" | "float" | "symbol" | "class" | "module" => map.next_value_seed(TextOrBytesSeed)?,
        "bytes" => Payload::Bytes(map.next_value_seed(BytesSeed)?),
        "regexp" => {
            let (source, options) = map.next_value_seed(RegexpSeed)?;
            Payload::Regexp(source, options)
        }
        "data" | "user_marshal" => Payload::Id(map.next_value_seed(ValueSeed { arena })?),
        "array" => Payload::Ids(map.next_value_seed(SeqSeed { arena })?),
        "hash" => Payload::Pairs(map.next_value_seed(HashPairsSeed { arena })?),
        other => {
            return Err(de::Error::custom(alloc::format!("unknown __type \"{other}\"")));
        }
    })
}

fn build_from_payload(
    arena: &mut Arena<'static>,
    type_name: &str,
    class: Option<SymId>,
    old_module: bool,
    default: Option<ValueId>,
    encoding: Option<&[u8]>,
    payload: Payload,
) -> Result<ValueId, &'static str> {
    Ok(match (type_name, payload) {
        ("bignum", Payload::Text(s)) => {
            let (negative, magnitude) = bignum::decimal_to_le_bytes(&s).ok_or("invalid bignum decimal string")?;
            arena.push_bignum(negative, magnitude)
        }
        ("float", Payload::Text(s)) => arena.push_float(s.into_bytes()),
        ("float", Payload::Bytes(b)) => arena.push_float(b),
        ("bytes", Payload::Bytes(b)) => arena.push_bytes(b),
        ("str", Payload::Text(s)) => push_str_payload(arena, s.into_bytes(), encoding)?,
        ("str", Payload::Bytes(b)) => push_str_payload(arena, b, encoding)?,
        ("symbol", Payload::Text(s)) => push_symbol_payload(arena, s.into_bytes(), encoding),
        ("symbol", Payload::Bytes(b)) => push_symbol_payload(arena, b, encoding),
        // Untagged: a UTF-8 tag is in the envelope's `__ivars` (an `E`), so the path is not tagged by its bytes here.
        ("class", Payload::Text(s)) => arena.push_path(Kind::Class, s.into_bytes(), false).ok_or(EMPTY_PATH)?,
        ("class", Payload::Bytes(b)) => arena.push_path(Kind::Class, b, false).ok_or(EMPTY_PATH)?,
        ("module", Payload::Text(s)) => arena
            .push_path(Kind::Module, s.into_bytes(), old_module)
            .ok_or(EMPTY_PATH)?,
        ("module", Payload::Bytes(b)) => arena.push_path(Kind::Module, b, old_module).ok_or(EMPTY_PATH)?,
        ("data", Payload::Id(body)) => arena
            .push_wrapper_sym(Flags::DATA, class.ok_or(NEEDS_CLASS)?, body)
            .ok_or(INVALID_ID)?,
        ("user_marshal", Payload::Id(body)) => arena
            .push_wrapper_sym(Flags::USER_MARSHAL, class.ok_or(NEEDS_CLASS)?, body)
            .ok_or(INVALID_ID)?,
        ("regexp", Payload::Regexp(source, options)) => match encoding {
            Some(name) => arena.push_regexp_with_encoding_name(source, options, name),
            None => arena.push_regexp(source, options),
        },
        ("array", Payload::Ids(ids)) => arena.push_array(&ids).ok_or(INVALID_ID)?,
        ("hash", Payload::Pairs(pairs)) => arena.push_hash(&pairs, default).ok_or(INVALID_ID)?,
        ("struct", Payload::Members(members)) => arena
            .push_members_sym(Kind::Struct, class.ok_or(NEEDS_CLASS)?, &members)
            .ok_or(INVALID_ID)?,
        ("object", Payload::Members(members)) => arena
            .push_members_sym(Kind::Object, class.ok_or(NEEDS_CLASS)?, &members)
            .ok_or(INVALID_ID)?,
        _ => return Err("__type does not match its __value/__members payload"),
    })
}

const INVALID_ID: &str = "a value id outside the arena, or an empty class name";
const NEEDS_CLASS: &str = "this \"__type\" needs a \"__class\"";
const EMPTY_PATH: &str = "empty class or module path";

/// A symbol with a named encoding is tagged with it; without one it gets the tag its bytes would anyway.
fn push_symbol_payload(arena: &mut Arena<'static>, bytes: Vec<u8>, encoding: Option<&[u8]>) -> ValueId {
    match encoding {
        Some(name) => arena.push_symbol_with_encoding_name(bytes, name),
        None => arena.push_symbol(bytes),
    }
}

/// A named encoding tags the bytes with it; otherwise they default to
/// UTF-8, matching [`Arena::push_string`]'s own convention.
fn push_str_payload(
    arena: &mut Arena<'static>,
    bytes: Vec<u8>,
    encoding: Option<&[u8]>,
) -> Result<ValueId, &'static str> {
    Ok(match encoding {
        Some(name) => arena.push_str_with_encoding_name(bytes, name),
        None => arena
            .push_str_with_encoding(bytes, crate::encoding::ENCODING_UTF_8)
            .ok_or("unknown encoding")?,
    })
}

/// A `Str`'s `__value` deserializes from either a JSON string or an array
/// of bytes - see the `Serialize` impl and [`deserialize_value_payload`].
struct TextOrBytesSeed;
impl<'de> DeserializeSeed<'de> for TextOrBytesSeed {
    type Value = Payload;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Payload, D::Error> {
        deserializer.deserialize_any(TextOrBytesVisitor)
    }
}
struct TextOrBytesVisitor;
impl<'de> Visitor<'de> for TextOrBytesVisitor {
    type Value = Payload;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a UTF-8 string or an array of bytes")
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Payload, E> {
        Ok(Payload::Text(String::from(v)))
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Payload, E> {
        Ok(Payload::Text(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Payload, A::Error> {
        BytesVisitor.visit_seq(seq).map(Payload::Bytes)
    }
    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Payload, E> {
        Ok(Payload::Bytes(v.to_vec()))
    }
}

/// Binary data: a base64 string, or (as older versions wrote it, and as a `Uint8Array` arrives) raw bytes.
struct BytesSeed;
impl<'de> DeserializeSeed<'de> for BytesSeed {
    type Value = Vec<u8>;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Vec<u8>, D::Error> {
        deserializer.deserialize_any(BytesVisitor)
    }
}
struct BytesVisitor;
impl<'de> Visitor<'de> for BytesVisitor {
    type Value = Vec<u8>;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a base64 string or an array of bytes")
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Vec<u8>, E> {
        base64::decode(v).ok_or_else(|| E::custom("invalid base64"))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
        let mut out = Vec::new();
        while let Some(b) = seq.next_element::<u8>()? {
            out.push(b);
        }
        Ok(out)
    }
    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Vec<u8>, E> {
        Ok(v.to_vec())
    }
}

/// A regexp's `__value`: `[source, options]`.
struct RegexpSeed;
impl<'de> DeserializeSeed<'de> for RegexpSeed {
    type Value = (Vec<u8>, u8);
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(self)
    }
}
impl<'de> Visitor<'de> for RegexpSeed {
    type Value = (Vec<u8>, u8);
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a [source, options] pair")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let source = seq
            .next_element_seed(BytesSeed)?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let options = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(1, &self))?;
        Ok((source, options))
    }
}

struct SeqSeed<'x> {
    arena: &'x mut Ctx,
}
impl<'de> DeserializeSeed<'de> for SeqSeed<'_> {
    type Value = Vec<ValueId>;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Vec<ValueId>, D::Error> {
        deserializer.deserialize_seq(SeqVisitor { arena: self.arena })
    }
}
struct SeqVisitor<'x> {
    arena: &'x mut Ctx,
}
impl<'de> Visitor<'de> for SeqVisitor<'_> {
    type Value = Vec<ValueId>;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "an array of values")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<ValueId>, A::Error> {
        let arena = self.arena;
        let mut out = Vec::new();
        while let Some(id) = seq.next_element_seed(ValueSeed { arena: &mut *arena })? {
            out.push(id);
        }
        Ok(out)
    }
}

struct HashPairsSeed<'x> {
    arena: &'x mut Ctx,
}
impl<'de> DeserializeSeed<'de> for HashPairsSeed<'_> {
    type Value = Vec<(ValueId, ValueId)>;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(HashPairsVisitor { arena: self.arena })
    }
}
struct HashPairsVisitor<'x> {
    arena: &'x mut Ctx,
}
impl<'de> Visitor<'de> for HashPairsVisitor<'_> {
    type Value = Vec<(ValueId, ValueId)>;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "an array of [key, value] pairs")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let arena = self.arena;
        let mut out = Vec::new();
        while let Some(pair) = seq.next_element_seed(PairSeed { arena: &mut *arena })? {
            out.push(pair);
        }
        Ok(out)
    }
}
struct PairSeed<'x> {
    arena: &'x mut Ctx,
}
impl<'de> DeserializeSeed<'de> for PairSeed<'_> {
    type Value = (ValueId, ValueId);
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(PairVisitor { arena: self.arena })
    }
}
struct PairVisitor<'x> {
    arena: &'x mut Ctx,
}
impl<'de> Visitor<'de> for PairVisitor<'_> {
    type Value = (ValueId, ValueId);
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a [key, value] pair")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let arena = self.arena;
        let key = seq
            .next_element_seed(ValueSeed { arena: &mut *arena })?
            .ok_or_else(|| de::Error::invalid_length(0, &PairExpecting))?;
        let value = seq
            .next_element_seed(ValueSeed { arena: &mut *arena })?
            .ok_or_else(|| de::Error::invalid_length(1, &PairExpecting))?;
        Ok((key, value))
    }
}
struct PairExpecting;
impl de::Expected for PairExpecting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a [key, value] pair")
    }
}

struct MembersSeed<'x> {
    arena: &'x mut Ctx,
}
impl<'de> DeserializeSeed<'de> for MembersSeed<'_> {
    type Value = Vec<(SymId, ValueId)>;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(MembersVisitor { arena: self.arena })
    }
}
struct MembersVisitor<'x> {
    arena: &'x mut Ctx,
}
impl<'de> Visitor<'de> for MembersVisitor<'_> {
    type Value = Vec<(SymId, ValueId)>;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "an array of [name, value] pairs")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let arena = self.arena;
        let mut out = Vec::new();
        while let Some(pair) = seq.next_element_seed(MemberPairSeed { arena: &mut *arena })? {
            out.push(pair);
        }
        Ok(out)
    }
}
struct MemberPairSeed<'x> {
    arena: &'x mut Ctx,
}
impl<'de> DeserializeSeed<'de> for MemberPairSeed<'_> {
    type Value = (SymId, ValueId);
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(MemberPairVisitor { arena: self.arena })
    }
}
struct MemberPairVisitor<'x> {
    arena: &'x mut Ctx,
}
impl<'de> Visitor<'de> for MemberPairVisitor<'_> {
    type Value = (SymId, ValueId);
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a [name, value] pair")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let arena = self.arena;
        let name = seq
            .next_element::<NameBuf>()?
            .ok_or_else(|| de::Error::invalid_length(0, &PairExpecting))?;
        let name = arena.arena.name_symbol(name.bytes, name.encoding.as_deref());
        let value = seq
            .next_element_seed(ValueSeed { arena: &mut *arena })?
            .ok_or_else(|| de::Error::invalid_length(1, &PairExpecting))?;
        Ok((name, value))
    }
}
