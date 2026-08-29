//! Ergonomic read cursors over an [`Arena`].
//!
//! [`ValueRef`] is a `Copy` `(&Arena, ValueId)` pair - cheap to pass around,
//! borrowing everything it returns from the arena. It intentionally does
//! **not** implement `core::ops::Index`: that trait must return `&Output`,
//! and a cursor has no stored `ValueRef` anywhere to hand out a reference
//! to (it is a fresh value constructed on every call). Use [`ValueRef::at`]
//! and [`ValueRef::get`] instead of `v[i]`/`v["key"]`.

use crate::{
    arena::{Arena, Flags, Kind, ValueId},
    bignum,
};
#[cfg(feature = "alloc")]
use alloc::string::String;

/// A read cursor into one node of an [`Arena`].
#[derive(Clone, Copy)]
pub struct ValueRef<'r, 'a> {
    arena: &'r Arena<'a>,
    id: ValueId,
}

impl<'r, 'a> ValueRef<'r, 'a> {
    #[must_use]
    pub const fn new(arena: &'r Arena<'a>, id: ValueId) -> Self {
        Self { arena, id }
    }

    #[must_use]
    pub const fn root(arena: &'r Arena<'a>) -> Self {
        Self {
            arena,
            id: arena.root(),
        }
    }

    #[inline]
    #[must_use]
    pub const fn id(&self) -> ValueId {
        self.id
    }

    #[inline]
    #[must_use]
    pub const fn arena(&self) -> &'r Arena<'a> {
        self.arena
    }

    #[inline]
    #[must_use]
    pub fn kind(&self) -> Kind {
        self.arena.node(self.id).kind
    }

    #[inline]
    #[must_use]
    pub fn is_nil(&self) -> bool {
        self.kind() == Kind::Nil
    }

    /// The value's declared class/module name (works for any kind - a bare
    /// `Array` can carry a class via `TYPE_UCLASS`, not just `Object`).
    #[must_use]
    pub fn class_name(&self) -> Option<&'r [u8]> {
        self.arena.class_of(self.id).map(|sym| self.arena.symbol_bytes(sym))
    }

    #[must_use]
    pub fn is_user_class(&self) -> bool {
        self.arena.node(self.id).flags.contains(Flags::USER_CLASS)
    }

    /// Module names this value was `extend`ed with (`TYPE_EXTENDED`), in
    /// declaration order.
    pub fn extensions(&self) -> impl Iterator<Item = &'r [u8]> + 'r {
        let arena = self.arena;
        arena.extensions_of(self.id).map(move |sym| arena.symbol_bytes(sym))
    }

    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self.kind() {
            Kind::True => Some(true),
            Kind::False => Some(false),
            _ => None,
        }
    }

    #[allow(clippy::cast_possible_wrap)]
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        let node = self.arena.node(self.id);
        match node.kind {
            Kind::Fixnum => Some(i64::from(node.a as i32)),
            Kind::Bignum => {
                let bytes = self.arena.blob(node.a);
                if bytes.len() > 8 {
                    return None;
                }
                let mut buf = [0u8; 8];
                buf[..bytes.len()].copy_from_slice(bytes);
                let magnitude = u64::from_le_bytes(buf);
                if node.flags.contains(Flags::NEGATIVE) {
                    #[allow(clippy::cast_possible_wrap)]
                    return (magnitude <= 1u64 << 63).then(|| (magnitude as i64).wrapping_neg());
                }
                i64::try_from(magnitude).ok()
            }
            _ => None,
        }
    }

    /// The bignum's sign and little-endian magnitude bytes, regardless of
    /// whether it fits in an `i64`.
    #[must_use]
    pub fn as_bignum_bytes(&self) -> Option<(bool, &'r [u8])> {
        let node = self.arena.node(self.id);
        (node.kind == Kind::Bignum).then(|| (node.flags.contains(Flags::NEGATIVE), self.arena.blob(node.a)))
    }

    #[cfg(feature = "alloc")]
    #[must_use]
    pub fn as_bigint_decimal(&self) -> Option<String> {
        let (negative, magnitude) = self.as_bignum_bytes()?;
        Some(bignum::le_bytes_to_decimal(negative, magnitude))
    }

    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        let node = self.arena.node(self.id);
        (node.kind == Kind::Float)
            .then(|| core::str::from_utf8(self.arena.blob(node.a)).ok())
            .flatten()
            .and_then(|s| s.parse().ok())
    }

    /// The original ASCII text of a `Float` (e.g. `b"1.5"`, `b"inf"`),
    /// verbatim as Marshal stored it.
    #[must_use]
    pub fn as_float_bytes(&self) -> Option<&'r [u8]> {
        let node = self.arena.node(self.id);
        (node.kind == Kind::Float).then(|| self.arena.blob(node.a))
    }

    /// Raw bytes of a `Str` or `Bytes` node.
    #[must_use]
    pub fn as_bytes(&self) -> Option<&'r [u8]> {
        let node = self.arena.node(self.id);
        matches!(node.kind, Kind::Str | Kind::Bytes).then(|| self.arena.blob(node.a))
    }

    /// Text of a `Str` or `Symbol` node, if its bytes happen to validate as
    /// UTF-8 - independent of what encoding was actually declared (see
    /// [`ValueRef::encoding_name`]), since a `Str`'s tag is never checked
    /// against its content. `Bytes` never yields `Some` here - call
    /// [`ValueRef::as_bytes`] and validate/transcode yourself if that's
    /// what you want.
    #[must_use]
    pub fn as_str(&self) -> Option<&'r str> {
        let node = self.arena.node(self.id);
        match node.kind {
            Kind::Str => core::str::from_utf8(self.arena.blob(node.a)).ok(),
            Kind::Symbol => self.arena.symbol_str(node.a),
            _ => None,
        }
    }

    /// This value's declared text-encoding id (see [`mod@crate::encoding`]),
    /// for a `Str`, `Bytes`, or `Regexp` value. Every Ruby `String`/`Regexp`
    /// has an encoding - an untagged `Bytes` value (no `E`/`encoding` ivar
    /// was present at load time) implicitly means
    /// [`crate::encoding::ENCODING_ASCII_8BIT`], matching Ruby's own
    /// default. `None` for any other kind.
    #[allow(clippy::cast_possible_truncation)]
    #[must_use]
    pub fn encoding_id(&self) -> Option<u8> {
        let node = self.arena.node(self.id);
        match node.kind {
            Kind::Bytes => Some(crate::encoding::ENCODING_ASCII_8BIT),
            Kind::Str => Some(node.b as u8),
            Kind::Regexp => Some((node.b >> 8) as u8),
            _ => None,
        }
    }

    /// The name behind [`ValueRef::encoding_id`] - resolved from the fixed
    /// table for a known id, or from the arena's custom-encoding side table
    /// for [`crate::encoding::ENCODING_CUSTOM`]. `None` only when
    /// `encoding_id` itself is `None`; an untagged `Bytes` value still
    /// yields `Some(b"ASCII-8BIT")` even though no ivar was ever written for
    /// it.
    #[must_use]
    pub fn encoding_name(&self) -> Option<&'r [u8]> {
        match self.encoding_id()? {
            crate::encoding::ENCODING_CUSTOM => self.arena.custom_encoding_of(self.id),
            id => crate::encoding::encoding_name(id),
        }
    }

    #[must_use]
    pub fn as_symbol_bytes(&self) -> Option<&'r [u8]> {
        let node = self.arena.node(self.id);
        (node.kind == Kind::Symbol).then(|| self.arena.symbol_bytes(node.a))
    }

    /// A regexp's source pattern and option bits. Its declared encoding (if
    /// any) is available separately via [`ValueRef::encoding_id`]/
    /// [`ValueRef::encoding_name`].
    #[allow(clippy::cast_possible_truncation)]
    #[must_use]
    pub fn as_regexp(&self) -> Option<(&'r [u8], u8)> {
        let node = self.arena.node(self.id);
        (node.kind == Kind::Regexp).then(|| (self.arena.blob(node.a), node.b as u8))
    }

    /// The raw class/module path of a `Class`/`Module` value (Ruby stores
    /// these as plain strings, not symbols).
    #[must_use]
    pub fn as_path(&self) -> Option<&'r [u8]> {
        let node = self.arena.node(self.id);
        matches!(node.kind, Kind::Class | Kind::Module).then(|| self.arena.blob(node.a))
    }

    #[must_use]
    pub fn is_old_module(&self) -> bool {
        self.arena.node(self.id).flags.contains(Flags::OLD_MODULE)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        let node = self.arena.node(self.id);
        match node.kind {
            Kind::Array | Kind::Hash | Kind::Object | Kind::Struct => node.b as usize,
            _ => 0,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterates an `Array`'s elements.
    #[must_use]
    pub fn array(&self) -> impl ExactSizeIterator<Item = ValueRef<'r, 'a>> + 'r {
        let arena = self.arena;
        let node = arena.node(self.id);
        let (start, len) = if node.kind == Kind::Array {
            (node.a, node.b)
        } else {
            (0, 0)
        };
        (0..len).map(move |i| ValueRef::new(arena, arena.children[(start + i) as usize]))
    }

    #[must_use]
    pub fn at(&self, index: usize) -> Option<Self> {
        let node = self.arena.node(self.id);
        (node.kind == Kind::Array && index < node.b as usize)
            .then(|| ValueRef::new(self.arena, self.arena.children[node.a as usize + index]))
    }

    /// Iterates a `Hash`'s key/value pairs (not including the default
    /// value, if any - see [`ValueRef::default_value`]).
    #[must_use]
    pub fn entries(&self) -> impl ExactSizeIterator<Item = (ValueRef<'r, 'a>, ValueRef<'r, 'a>)> + 'r {
        let arena = self.arena;
        let node = arena.node(self.id);
        let (start, len) = if node.kind == Kind::Hash {
            (node.a, node.b)
        } else {
            (0, 0)
        };
        (0..len).map(move |i| {
            let k = arena.children[(start + i * 2) as usize];
            let v = arena.children[(start + i * 2 + 1) as usize];
            (ValueRef::new(arena, k), ValueRef::new(arena, v))
        })
    }

    #[must_use]
    pub fn default_value(&self) -> Option<Self> {
        let node = self.arena.node(self.id);
        (node.kind == Kind::Hash && node.flags.contains(Flags::HAS_DEFAULT)).then(|| {
            let idx = node.a as usize + node.b as usize * 2;
            ValueRef::new(self.arena, self.arena.children[idx])
        })
    }

    /// Looks up a `Hash` entry by structural key equality. Linear scan -
    /// fine for the small hashes real Marshal payloads carry; callers
    /// hammering one large hash repeatedly should build their own index
    /// from [`ValueRef::entries`] instead.
    #[must_use]
    pub fn lookup(&self, key: ValueRef<'_, '_>) -> Option<Self> {
        self.entries().find(|(k, _)| value_eq(*k, key)).map(|(_, v)| v)
    }

    /// Looks up a `Hash` entry whose key is the symbol `name` - the common
    /// case of a Ruby `Hash` used as a keyword-style record
    /// (`{ name: "Alice", hp: 30 }`). Convenience over [`ValueRef::lookup`]
    /// that doesn't require building a symbol `ValueRef` to compare against.
    #[must_use]
    pub fn lookup_symbol(&self, name: &str) -> Option<Self> {
        let wanted = name.as_bytes();
        self.entries()
            .find(|(k, _)| k.as_symbol_bytes() == Some(wanted))
            .map(|(_, v)| v)
    }

    /// Iterates an `Object`'s instance variables or a `Struct`'s members as
    /// `(name, value)` pairs. Names are the raw ivar symbol bytes, e.g.
    /// `b"@hp"` for an object, or the bare member name for a struct.
    #[must_use]
    pub fn members(&self) -> impl ExactSizeIterator<Item = (&'r [u8], ValueRef<'r, 'a>)> + 'r {
        let arena = self.arena;
        let node = arena.node(self.id);
        let (start, len) = if matches!(node.kind, Kind::Object | Kind::Struct) {
            (node.a, node.b)
        } else {
            (0, 0)
        };
        (0..len).map(move |i| {
            let (sym, v) = arena.members[(start + i) as usize];
            (arena.symbol_bytes(sym), ValueRef::new(arena, v))
        })
    }

    /// Looks up an `Object` instance variable by name (with or without the
    /// leading `@`).
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Self> {
        let wanted = name.as_bytes();
        self.members()
            .find(|(n, _)| *n == wanted || n.strip_prefix(b"@") == Some(wanted))
            .map(|(_, v)| v)
    }

    /// True if this value came from `_dump_data`/`_load_data`
    /// (`TYPE_DATA`). Its `kind` describes whatever the wrapped value
    /// actually was - `Data` has no shape of its own.
    #[must_use]
    pub fn is_data(&self) -> bool {
        self.arena.node(self.id).flags.contains(Flags::DATA)
    }

    /// True if this value came from `marshal_dump`/`marshal_load`
    /// (`TYPE_USRMARSHAL`).
    #[must_use]
    pub fn is_user_marshal(&self) -> bool {
        self.arena.node(self.id).flags.contains(Flags::USER_MARSHAL)
    }

    /// True if this value is `_dump`-produced raw bytes (`TYPE_USERDEF`) -
    /// always paired with `Kind::Bytes`.
    #[must_use]
    pub fn is_user_defined(&self) -> bool {
        self.arena.node(self.id).flags.contains(Flags::USER_DEFINED)
    }
}

/// Structural equality between two values (recursing into compound kinds),
/// used for hash-key lookup. Object identity (`ValueId`) is deliberately
/// not part of this - two separately-built but content-equal strings are
/// equal keys, matching Ruby's `Hash#[]` semantics for value types.
fn value_eq(a: ValueRef<'_, '_>, b: ValueRef<'_, '_>) -> bool {
    match (a.kind(), b.kind()) {
        (Kind::Nil, Kind::Nil) | (Kind::True, Kind::True) | (Kind::False, Kind::False) => true,
        (Kind::Fixnum, Kind::Fixnum) => a.as_i64() == b.as_i64(),
        (Kind::Bignum, Kind::Bignum) => a.as_bignum_bytes() == b.as_bignum_bytes(),
        (Kind::Float, Kind::Float) => a.as_float_bytes() == b.as_float_bytes(),
        (Kind::Str, Kind::Str) | (Kind::Bytes, Kind::Bytes) => a.as_bytes() == b.as_bytes(),
        (Kind::Symbol, Kind::Symbol) => a.as_symbol_bytes() == b.as_symbol_bytes(),
        (Kind::Array, Kind::Array) => a.len() == b.len() && a.array().zip(b.array()).all(|(x, y)| value_eq(x, y)),
        _ => false,
    }
}
