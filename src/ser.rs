//! Streaming `serde` `Serialize`/`Deserialize` for [`Arena`].
//!
//! Drives the `Serializer`/`Deserializer` directly, so it works with any
//! serde data format, not just JSON.
//!
//! # Envelope
//!
//! `Nil`/`True`/`False`/`Fixnum` serialize as bare JSON primitives
//! (`null`/`true`/`false`/a number). Every other value becomes an object:
//!
//! ```json
//! {"__type": "array", "__class": "MyArray", "__flags": ["user_class"], "__value": [1, 2, 3]}
//! ```
//!
//! - `__type` (always present, always first): one of `bignum`, `float`,
//!   `bytes`, `str`, `symbol`, `regexp`, `array`, `hash`, `struct`,
//!   `object`, `class`, `module`.
//! - `__class` (omitted if none): the declared class/module name.
//! - `__flags` (omitted if empty): any of `old_module`, `user_class`,
//!   `data`, `user_marshal`, `user_defined`.
//! - `__extensions` (omitted if empty): `Module#extend`ed module names.
//! - `__encoding` (`str`/`regexp` only, omitted for the default
//!   `ASCII-8BIT`): the declared encoding's name (e.g. `"UTF-8"`,
//!   `"Shift_JIS"`) - this crate never transcodes, so a `str`'s `__value` is
//!   exactly the original bytes, tagged with whatever encoding they were
//!   declared as (see [`mod@crate::encoding`]).
//! - `__value`: the kind-specific payload - a hash's is a JSON array of
//!   `[key, value]` pairs (not a JSON object: hash keys aren't always
//!   strings). A `str`'s is plain text when its bytes happen to validate as
//!   UTF-8 (regardless of `__encoding`), or a byte array otherwise -
//!   deserializing accepts either shape.
//! - `__members` (struct/object only, instead of `__value`): a JSON array
//!   of `[name, value]` pairs.
//! - `__default` (hash only, when present): the hash's default value.
//!
//! Deserializing requires `__type` to be the object's first key (this is
//! `marshal-rs`'s own wire format, not a general-purpose JSON schema, and
//! that's what lets `__value`/`__members` be interpreted while still being
//! read as a single streaming pass); every other key may appear in any
//! order. Object links and cycles are not preserved across a JSON
//! round-trip - shared/self-referential structure is flattened into
//! independent copies.

use crate::{
    arena::{Arena, Flags, Kind, ValueId},
    bignum,
    value::ValueRef,
};
use alloc::{borrow::Cow, fmt, string::String, vec::Vec};
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
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

fn flag_names(v: ValueRef<'_, '_>) -> Vec<&'static str> {
    let mut out = Vec::new();
    if v.is_old_module() {
        out.push("old_module");
    }
    if v.is_user_class() {
        out.push("user_class");
    }
    if v.is_data() {
        out.push("data");
    }
    if v.is_user_marshal() {
        out.push("user_marshal");
    }
    if v.is_user_defined() {
        out.push("user_defined");
    }
    out
}

impl Serialize for Arena<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ValueRef::root(self).serialize(serializer)
    }
}

impl Serialize for ValueRef<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.kind() {
            Kind::Nil => serializer.serialize_unit(),
            Kind::True => serializer.serialize_bool(true),
            Kind::False => serializer.serialize_bool(false),
            Kind::Fixnum => serializer.serialize_i64(self.as_i64().unwrap()),
            kind => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("__type", type_name(kind))?;
                if let Some(class) = self.class_name() {
                    map.serialize_entry("__class", &String::from_utf8_lossy(class))?;
                }
                let flags = flag_names(*self);
                if !flags.is_empty() {
                    map.serialize_entry("__flags", &flags)?;
                }
                let extensions: Vec<String> = self
                    .extensions()
                    .map(|m| String::from_utf8_lossy(m).into_owned())
                    .collect();
                if !extensions.is_empty() {
                    map.serialize_entry("__extensions", &extensions)?;
                }
                if matches!(kind, Kind::Str | Kind::Regexp) {
                    let encoding = self
                        .encoding_id()
                        .filter(|&id| id != crate::encoding::ENCODING_ASCII_8BIT)
                        .and_then(|_| self.encoding_name());
                    if let Some(name) = encoding {
                        map.serialize_entry("__encoding", &String::from_utf8_lossy(name))?;
                    }
                }
                match kind {
                    Kind::Bignum => {
                        map.serialize_entry("__value", &self.as_bigint_decimal().unwrap())?;
                    }
                    Kind::Float => {
                        let text = String::from_utf8_lossy(self.as_float_bytes().unwrap());
                        map.serialize_entry("__value", &text)?;
                    }
                    Kind::Bytes => {
                        map.serialize_entry("__value", self.as_bytes().unwrap())?;
                    }
                    // Prefer plain text when the bytes happen to validate
                    // (the overwhelmingly common case, and much more
                    // readable) - fall back to a raw byte array otherwise,
                    // exactly like `Bytes`. `__encoding` above (not this
                    // shape) is what carries the actual declared encoding;
                    // deserialize accepts either shape for `__value`.
                    Kind::Str => match self.as_str() {
                        Some(text) => map.serialize_entry("__value", text)?,
                        None => map.serialize_entry("__value", self.as_bytes().unwrap())?,
                    },
                    Kind::Symbol => {
                        let text = String::from_utf8_lossy(self.as_symbol_bytes().unwrap());
                        map.serialize_entry("__value", &text)?;
                    }
                    Kind::Regexp => {
                        let (source, options) = self.as_regexp().unwrap();
                        map.serialize_entry("__value", &(source, options))?;
                    }
                    Kind::Array => {
                        map.serialize_entry("__value", &ArraySeq(*self))?;
                    }
                    Kind::Hash => {
                        map.serialize_entry("__value", &HashSeq(*self))?;
                        if let Some(default) = self.default_value() {
                            map.serialize_entry("__default", &default)?;
                        }
                    }
                    Kind::Struct | Kind::Object => {
                        map.serialize_entry("__members", &MembersSeq(*self))?;
                    }
                    Kind::Class | Kind::Module => {
                        let text = String::from_utf8_lossy(self.as_path().unwrap());
                        map.serialize_entry("__value", &text)?;
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

struct ArraySeq<'r, 'a>(ValueRef<'r, 'a>);
impl Serialize for ArraySeq<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for item in self.0.array() {
            seq.serialize_element(&item)?;
        }
        seq.end()
    }
}

struct HashSeq<'r, 'a>(ValueRef<'r, 'a>);
impl Serialize for HashSeq<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for (k, v) in self.0.entries() {
            seq.serialize_element(&(k, v))?;
        }
        seq.end()
    }
}

struct MembersSeq<'r, 'a>(ValueRef<'r, 'a>);
impl Serialize for MembersSeq<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for (name, value) in self.0.members() {
            seq.serialize_element(&(String::from_utf8_lossy(name), value))?;
        }
        seq.end()
    }
}

impl<'de> serde::Deserialize<'de> for Arena<'static> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut arena = Arena::builder();
        let root = ValueSeed { arena: &mut arena }.deserialize(deserializer)?;
        arena.set_root(root);
        Ok(arena)
    }
}

struct ValueSeed<'x> {
    arena: &'x mut Arena<'static>,
}

impl<'de> DeserializeSeed<'de> for ValueSeed<'_> {
    type Value = ValueId;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<ValueId, D::Error> {
        deserializer.deserialize_any(ValueVisitor { arena: self.arena })
    }
}

struct ValueVisitor<'x> {
    arena: &'x mut Arena<'static>,
}

enum Payload {
    None,
    Text(String),
    Bytes(Vec<u8>),
    Regexp(Vec<u8>, u8),
    Ids(Vec<ValueId>),
    Pairs(Vec<(ValueId, ValueId)>),
    Members(Vec<(String, ValueId)>),
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

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ValueId, A::Error> {
        let arena = self.arena;

        let Some(first_key) = map.next_key::<String>()? else {
            return Err(de::Error::custom("expected a non-empty envelope object"));
        };
        if first_key != "__type" {
            return Err(de::Error::custom(
                "envelope object must have \"__type\" as its first key",
            ));
        }
        let type_name: String = map.next_value()?;

        let mut class: Option<Vec<u8>> = None;
        let mut old_module = false;
        let mut user_class = false;
        let mut data = false;
        let mut user_marshal = false;
        let mut user_defined = false;
        let mut extensions: Vec<Vec<u8>> = Vec::new();
        let mut payload = Payload::None;
        let mut default: Option<ValueId> = None;
        let mut encoding: Option<Vec<u8>> = None;

        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "__class" => class = Some(map.next_value::<String>()?.into_bytes()),
                "__flags" => {
                    for name in map.next_value::<Vec<String>>()? {
                        match name.as_str() {
                            "old_module" => old_module = true,
                            "user_class" => user_class = true,
                            "data" => data = true,
                            "user_marshal" => user_marshal = true,
                            "user_defined" => user_defined = true,
                            _ => {}
                        }
                    }
                }
                "__extensions" => {
                    extensions = map
                        .next_value::<Vec<String>>()?
                        .into_iter()
                        .map(String::into_bytes)
                        .collect();
                }
                "__value" => payload = deserialize_value_payload(&type_name, arena, &mut map)?,
                "__members" => {
                    payload = Payload::Members(map.next_value_seed(MembersSeed { arena: &mut *arena })?);
                }
                "__default" => default = Some(map.next_value_seed(ValueSeed { arena: &mut *arena })?),
                "__encoding" => encoding = Some(map.next_value::<String>()?.into_bytes()),
                _ => {
                    let _ = map.next_value::<de::IgnoredAny>()?;
                }
            }
        }

        let id = build_from_payload(
            arena,
            &type_name,
            class.clone(),
            old_module,
            default,
            encoding.as_deref(),
            payload,
        )
        .map_err(de::Error::custom)?;

        if !matches!(type_name.as_str(), "struct" | "object") {
            if let Some(class_bytes) = class {
                let sym = arena.intern_symbol(Cow::Owned(class_bytes));
                patch(arena, id, |node| node.class = sym);
            }
        }
        if user_class {
            patch(arena, id, |node| node.flags = node.flags.with(Flags::USER_CLASS));
        }
        if data {
            patch(arena, id, |node| node.flags = node.flags.with(Flags::DATA));
        }
        if user_marshal {
            patch(arena, id, |node| node.flags = node.flags.with(Flags::USER_MARSHAL));
        }
        if user_defined {
            patch(arena, id, |node| node.flags = node.flags.with(Flags::USER_DEFINED));
        }
        for ext_bytes in extensions {
            let sym = arena.intern_symbol(Cow::Owned(ext_bytes));
            arena.add_extension(id, sym);
        }

        Ok(id)
    }
}

fn patch(arena: &mut Arena<'static>, id: ValueId, f: impl FnOnce(&mut crate::arena::Node)) {
    let mut node = *arena.node(id);
    f(&mut node);
    arena.set_node(id, node);
}

fn deserialize_value_payload<'de, A: MapAccess<'de>>(
    type_name: &str,
    arena: &mut Arena<'static>,
    map: &mut A,
) -> Result<Payload, A::Error> {
    Ok(match type_name {
        "bignum" | "float" | "symbol" | "class" | "module" => Payload::Text(map.next_value()?),
        // A `Str`'s `__value` is plain text when its bytes happened to
        // validate as UTF-8 at serialize time, or a raw byte array
        // otherwise (see the `Serialize` impl) - accept either shape.
        "str" => map.next_value_seed(TextOrBytesSeed)?,
        "bytes" => Payload::Bytes(map.next_value()?),
        "regexp" => {
            let (source, options): (Vec<u8>, u8) = map.next_value()?;
            Payload::Regexp(source, options)
        }
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
    class: Option<Vec<u8>>,
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
        ("bytes", Payload::Bytes(b)) => arena.push_bytes(b),
        ("str", Payload::Text(s)) => push_str_payload(arena, s.into_bytes(), encoding),
        ("str", Payload::Bytes(b)) => push_str_payload(arena, b, encoding),
        ("symbol", Payload::Text(s)) => arena.push_symbol(s.into_bytes()),
        ("class", Payload::Text(s)) => arena.push_class(s.into_bytes()),
        ("module", Payload::Text(s)) => arena.push_module(s.into_bytes(), old_module),
        ("regexp", Payload::Regexp(source, options)) => match encoding {
            Some(name) => arena.push_regexp_with_encoding_name(source, options, name),
            None => arena.push_regexp(source, options),
        },
        ("array", Payload::Ids(ids)) => arena.push_array(&ids),
        ("hash", Payload::Pairs(pairs)) => arena.push_hash(&pairs, default),
        ("struct", Payload::Members(members)) => {
            let members: Vec<(Vec<u8>, ValueId)> = members.into_iter().map(|(n, v)| (n.into_bytes(), v)).collect();
            arena.push_struct(class.unwrap_or_default(), &members)
        }
        ("object", Payload::Members(members)) => {
            let members: Vec<(Vec<u8>, ValueId)> = members.into_iter().map(|(n, v)| (n.into_bytes(), v)).collect();
            arena.push_object(class.unwrap_or_default(), &members)
        }
        _ => return Err("__type does not match its __value/__members payload"),
    })
}

/// A named encoding tags the bytes with it; otherwise they default to
/// UTF-8, matching [`Arena::push_string`]'s own convention.
fn push_str_payload(arena: &mut Arena<'static>, bytes: Vec<u8>, encoding: Option<&[u8]>) -> ValueId {
    match encoding {
        Some(name) => arena.push_str_with_encoding_name(bytes, name),
        None => arena.push_str_with_encoding(bytes, crate::encoding::ENCODING_UTF_8),
    }
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
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Payload, A::Error> {
        let mut out = Vec::new();
        while let Some(b) = seq.next_element::<u8>()? {
            out.push(b);
        }
        Ok(Payload::Bytes(out))
    }
}

struct SeqSeed<'x> {
    arena: &'x mut Arena<'static>,
}
impl<'de> DeserializeSeed<'de> for SeqSeed<'_> {
    type Value = Vec<ValueId>;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Vec<ValueId>, D::Error> {
        deserializer.deserialize_seq(SeqVisitor { arena: self.arena })
    }
}
struct SeqVisitor<'x> {
    arena: &'x mut Arena<'static>,
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
    arena: &'x mut Arena<'static>,
}
impl<'de> DeserializeSeed<'de> for HashPairsSeed<'_> {
    type Value = Vec<(ValueId, ValueId)>;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(HashPairsVisitor { arena: self.arena })
    }
}
struct HashPairsVisitor<'x> {
    arena: &'x mut Arena<'static>,
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
    arena: &'x mut Arena<'static>,
}
impl<'de> DeserializeSeed<'de> for PairSeed<'_> {
    type Value = (ValueId, ValueId);
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(PairVisitor { arena: self.arena })
    }
}
struct PairVisitor<'x> {
    arena: &'x mut Arena<'static>,
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
    arena: &'x mut Arena<'static>,
}
impl<'de> DeserializeSeed<'de> for MembersSeed<'_> {
    type Value = Vec<(String, ValueId)>;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(MembersVisitor { arena: self.arena })
    }
}
struct MembersVisitor<'x> {
    arena: &'x mut Arena<'static>,
}
impl<'de> Visitor<'de> for MembersVisitor<'_> {
    type Value = Vec<(String, ValueId)>;
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
    arena: &'x mut Arena<'static>,
}
impl<'de> DeserializeSeed<'de> for MemberPairSeed<'_> {
    type Value = (String, ValueId);
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(MemberPairVisitor { arena: self.arena })
    }
}
struct MemberPairVisitor<'x> {
    arena: &'x mut Arena<'static>,
}
impl<'de> Visitor<'de> for MemberPairVisitor<'_> {
    type Value = (String, ValueId);
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a [name, value] pair")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let arena = self.arena;
        let name: String = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &PairExpecting))?;
        let value = seq
            .next_element_seed(ValueSeed { arena: &mut *arena })?
            .ok_or_else(|| de::Error::invalid_length(1, &PairExpecting))?;
        Ok((name, value))
    }
}
