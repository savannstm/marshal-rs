//! Read cursors over an [`Arena`]. [`ValueRef`] is a `Copy` `(&Arena, ValueId)` pair. It does not implement
//! `Index` (there is no stored value to borrow from); use [`ValueRef::at`] and [`ValueRef::get`].

#[cfg(feature = "serde")]
use crate::arena::SymId;
use crate::{
    arena::{Arena, Flags, Kind, ValueId},
    bignum,
};
#[cfg(feature = "alloc")]
use alloc::{collections::BTreeMap, string::String};

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

    /// The node whose contents every accessor below reads: this one, or the body if it is a wrapper.
    #[inline]
    fn node(&self) -> &'r crate::arena::Node {
        self.arena.node(self.arena.resolve(self.id))
    }

    /// The value a `_dump_data`/`marshal_dump` wrapper returned, with its own class, `extend`s and instance
    /// variables. `None` for any other value. `kind`, `array`, `as_str` and the like already read through the
    /// wrapper to the body, while [`ValueRef::class_name`], [`ValueRef::extensions`] and [`ValueRef::extra_ivars`]
    /// describe the wrapper.
    #[must_use]
    pub fn body(&self) -> Option<Self> {
        self.arena.body_of(self.id).map(|body| Self::new(self.arena, body))
    }

    #[inline]
    #[must_use]
    pub fn kind(&self) -> Kind {
        self.node().kind
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
        self.node().flags.contains(Flags::USER_CLASS)
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
        let node = self.node();
        match node.kind {
            Kind::Fixnum => Some(i64::from(node.a as i32)),
            Kind::Bignum => {
                let bytes = trim_high_zeros(self.arena.blob(node.a));
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
        let node = self.node();
        (node.kind == Kind::Bignum).then(|| (node.flags.contains(Flags::NEGATIVE), self.arena.blob(node.a)))
    }

    /// The bignum as decimal, or `None` if not a bignum or over [`bignum::MAX_CONVERTIBLE_BYTES`].
    /// [`ValueRef::as_bignum_bytes`] has no limit.
    #[cfg(feature = "alloc")]
    #[must_use]
    pub fn as_bigint_decimal(&self) -> Option<String> {
        let (negative, magnitude) = self.as_bignum_bytes()?;
        // The conversion is quadratic, see `bignum::MAX_CONVERTIBLE_BYTES`.
        if magnitude.len() > bignum::MAX_CONVERTIBLE_BYTES {
            return None;
        }
        Some(bignum::le_bytes_to_decimal(negative, magnitude))
    }

    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        let node = self.node();
        (node.kind == Kind::Float)
            .then(|| self.arena.blob(node.a))
            .and_then(float_text)
    }

    /// The original ASCII text of a `Float` (e.g. `b"1.5"`, `b"inf"`),
    /// verbatim as Marshal stored it.
    #[must_use]
    pub fn as_float_bytes(&self) -> Option<&'r [u8]> {
        let node = self.node();
        (node.kind == Kind::Float).then(|| self.arena.blob(node.a))
    }

    /// Raw bytes of a `Str` or `Bytes` node.
    #[must_use]
    pub fn as_bytes(&self) -> Option<&'r [u8]> {
        let node = self.node();
        matches!(node.kind, Kind::Str | Kind::Bytes).then(|| self.arena.blob(node.a))
    }

    /// Text of a `Str` or `Symbol` whose bytes are valid UTF-8, regardless of the declared encoding. `Bytes` never
    /// yields `Some`; use [`ValueRef::as_bytes`] and transcode yourself.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // the encoding id is a byte
    pub fn as_str(&self) -> Option<&'r str> {
        let node = self.node();
        match node.kind {
            // Only a UTF-8 (or ASCII) declared encoding says the bytes are text in Rust's sense; a
            // `Shift_JIS` string whose bytes happen to be valid UTF-8 is not.
            Kind::Str
                if matches!(
                    node.b as u8,
                    crate::encoding::ENCODING_UTF_8 | crate::encoding::ENCODING_US_ASCII
                ) =>
            {
                core::str::from_utf8(self.arena.blob(node.a)).ok()
            }
            Kind::Symbol => self.arena.symbol_str(node.a),
            _ => None,
        }
    }

    /// The declared encoding id for a `Str`, `Bytes`, `Regexp` or `Symbol`; `None` for other kinds.
    /// Untagged bytes read as `ASCII-8BIT`; an untagged ASCII symbol reads as `US-ASCII`.
    #[allow(clippy::cast_possible_truncation)]
    #[must_use]
    pub fn encoding_id(&self) -> Option<u8> {
        let node = self.node();
        match node.kind {
            Kind::Bytes => Some(crate::encoding::ENCODING_ASCII_8BIT),
            Kind::Str => Some(node.b as u8),
            Kind::Regexp => Some((node.b >> 8) as u8),
            Kind::Symbol => Some(self.arena.symbol_effective_encoding(node.a)),
            _ => None,
        }
    }

    /// The name behind [`ValueRef::encoding_id`], from the fixed table or the custom side table.
    #[must_use]
    pub fn encoding_name(&self) -> Option<&'r [u8]> {
        match self.encoding_id()? {
            crate::encoding::ENCODING_CUSTOM if self.kind() == Kind::Symbol => {
                self.arena.symbol_custom_encoding(self.node().a)
            }
            crate::encoding::ENCODING_CUSTOM => self.arena.custom_encoding_of(self.id),
            id => crate::encoding::encoding_name(id),
        }
    }

    /// A symbol's encoding tag exactly as it is stored and written ([`crate::encoding::ENCODING_ASCII_8BIT`]:
    /// none), unlike [`ValueRef::encoding_id`], which answers what Ruby would say. `None` for any other kind.
    #[must_use]
    pub fn symbol_tag(&self) -> Option<u8> {
        let node = self.node();
        (node.kind == Kind::Symbol).then(|| self.arena.symbol_encoding(node.a))
    }

    #[must_use]
    pub fn as_symbol_bytes(&self) -> Option<&'r [u8]> {
        let node = self.node();
        (node.kind == Kind::Symbol).then(|| self.arena.symbol_bytes(node.a))
    }

    /// A regexp's source pattern and option bits. Its declared encoding (if
    /// any) is available separately via [`ValueRef::encoding_id`]/
    /// [`ValueRef::encoding_name`].
    #[allow(clippy::cast_possible_truncation)]
    #[must_use]
    pub fn as_regexp(&self) -> Option<(&'r [u8], u8)> {
        let node = self.node();
        (node.kind == Kind::Regexp).then(|| (self.arena.blob(node.a), node.b as u8))
    }

    /// The raw class/module path of a `Class`/`Module` value (Ruby stores
    /// these as plain strings, not symbols).
    #[must_use]
    pub fn as_path(&self) -> Option<&'r [u8]> {
        let node = self.node();
        matches!(node.kind, Kind::Class | Kind::Module).then(|| self.arena.blob(node.a))
    }

    #[must_use]
    pub fn is_old_module(&self) -> bool {
        self.node().flags.contains(Flags::OLD_MODULE)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        let node = self.node();
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
    pub fn array(&self) -> impl ExactSizeIterator<Item = ValueRef<'r, 'a>> + 'r {
        let arena = self.arena;
        let node = arena.node(arena.resolve(self.id));
        let (start, len) = if node.kind == Kind::Array {
            (node.a, node.b)
        } else {
            (0, 0)
        };
        (0..len).map(move |i| ValueRef::new(arena, arena.children[(start + i) as usize]))
    }

    #[must_use]
    pub fn at(&self, index: usize) -> Option<Self> {
        let node = self.node();
        (node.kind == Kind::Array && index < node.b as usize)
            .then(|| ValueRef::new(self.arena, self.arena.children[node.a as usize + index]))
    }

    /// Iterates a `Hash`'s key/value pairs (not including the default
    /// value, if any - see [`ValueRef::default_value`]).
    pub fn entries(&self) -> impl ExactSizeIterator<Item = (ValueRef<'r, 'a>, ValueRef<'r, 'a>)> + 'r {
        let arena = self.arena;
        let node = arena.node(arena.resolve(self.id));
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
        let node = self.node();
        (node.kind == Kind::Hash && node.flags.contains(Flags::HAS_DEFAULT)).then(|| {
            let idx = node.a as usize + node.b as usize * 2;
            ValueRef::new(self.arena, self.arena.children[idx])
        })
    }

    /// Looks up a `Hash` entry by structural key equality. Linear scan - for many lookups on one big
    /// hash use [`ValueRef::symbol_index`]. Only `nil`, booleans, numbers, strings, symbols and arrays of
    /// those compare equal by content; any other key (an object, a hash, ...) matches only itself.
    #[must_use]
    pub fn lookup(&self, key: ValueRef<'_, '_>) -> Option<Self> {
        self.entries().find(|(k, _)| value_eq(*k, key)).map(|(_, v)| v)
    }

    /// All of a `Hash`'s symbol-keyed entries by name, for many lookups on one big hash:
    /// [`ValueRef::lookup`] and [`ValueRef::lookup_symbol`] scan the entries, so n lookups cost n times
    /// the hash's size. If a name repeats, the first entry wins, as it does for those. Empty unless
    /// this is a `Hash`.
    #[must_use]
    pub fn symbol_index(&self) -> BTreeMap<&'r [u8], Self> {
        let mut index = BTreeMap::new();
        for (key, value) in self.entries() {
            if let Some(name) = key.as_symbol_bytes() {
                index.entry(name).or_insert(value);
            }
        }
        index
    }

    /// Looks up a `Hash` entry by symbol key, as in a keyword-style record (`{ name: "Alice", hp: 30 }`).
    /// Like [`ValueRef::lookup`], without building a symbol `ValueRef` to compare against.
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
    pub fn members(&self) -> impl ExactSizeIterator<Item = (&'r [u8], ValueRef<'r, 'a>)> + 'r {
        let arena = self.arena;
        let node = arena.node(arena.resolve(self.id));
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

    /// Instance variables this value carried besides its encoding, as `(name, value)` in stream
    /// order. Rare: a string with extra ivars, or an array, hash, ... that has any.
    pub fn extra_ivars(&self) -> impl Iterator<Item = (&'r [u8], ValueRef<'r, 'a>)> + 'r {
        let arena = self.arena;
        arena
            .extra_ivars_of(self.id)
            .iter()
            .map(move |&(_, name, value)| (arena.symbol_bytes(name), ValueRef::new(arena, value)))
    }

    /// A name symbol's bytes and the encoding tag it carries when that is not the one its bytes would be given
    /// anyway. For the serde envelope, which writes names as text and says a tag only when there is one to say.
    #[cfg(feature = "serde")]
    pub(crate) fn name_of(&self, sym: SymId) -> (&'r [u8], Option<&'r [u8]>) {
        (self.arena.symbol_bytes(sym), self.arena.symbol_unusual_tag(sym))
    }

    #[cfg(feature = "serde")]
    pub(crate) fn class_sym(&self) -> Option<SymId> {
        self.arena.class_of(self.id)
    }

    #[cfg(feature = "serde")]
    pub(crate) fn extension_syms(&self) -> impl Iterator<Item = SymId> + 'r {
        self.arena.extensions_of(self.id)
    }

    /// [`ValueRef::members`] with the names as symbols.
    #[cfg(feature = "serde")]
    pub(crate) fn member_syms(&self) -> impl ExactSizeIterator<Item = (SymId, ValueRef<'r, 'a>)> + 'r {
        let arena = self.arena;
        let node = arena.node(arena.resolve(self.id));
        let (start, len) = if matches!(node.kind, Kind::Object | Kind::Struct) {
            (node.a, node.b)
        } else {
            (0, 0)
        };
        (0..len).map(move |i| {
            let (sym, v) = arena.members[(start + i) as usize];
            (sym, ValueRef::new(arena, v))
        })
    }

    /// [`ValueRef::extra_ivars`] with the names as symbols.
    #[cfg(feature = "serde")]
    pub(crate) fn extra_ivar_syms(&self) -> impl Iterator<Item = (SymId, ValueRef<'r, 'a>)> + 'r {
        let arena = self.arena;
        arena
            .extra_ivars_of(self.id)
            .iter()
            .map(move |&(_, name, value)| (name, ValueRef::new(arena, value)))
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
        self.node().flags.contains(Flags::USER_DEFINED)
    }
}

/// Strips the most significant zero bytes of a little-endian magnitude.
fn trim_high_zeros(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |last| last + 1);
    &bytes[..end]
}

/// A float's text as a number: Rust's float syntax (which includes `inf`, `-inf` and `nan`), up to the
/// NUL after which Ruby before 1.9 appended mantissa bytes.
pub(crate) fn float_text(bytes: &[u8]) -> Option<f64> {
    core::str::from_utf8(bytes.split(|&b| b == 0).next()?)
        .ok()?
        .parse()
        .ok()
}

/// Structural equality for hash-key lookup, ignoring identity (content-equal values match, as in Ruby's `Hash#[]`).
/// Bounded, so cyclic arrays compare unequal past the limit.
fn value_eq(a: ValueRef<'_, '_>, b: ValueRef<'_, '_>) -> bool {
    const MAX_EQ_DEPTH: usize = 256;
    const MAX_EQ_STEPS: usize = 1 << 20;

    value_eq_within(a, b, MAX_EQ_DEPTH, &mut { MAX_EQ_STEPS })
}

fn value_eq_within(a: ValueRef<'_, '_>, b: ValueRef<'_, '_>, depth: usize, steps: &mut usize) -> bool {
    if a.id == b.id && core::ptr::eq(a.arena, b.arena) {
        return true;
    }

    match (a.kind(), b.kind()) {
        (Kind::Nil, Kind::Nil) | (Kind::True, Kind::True) | (Kind::False, Kind::False) => true,
        // The same number is equal whichever way it was stored; past `i64` only bignums are left.
        (Kind::Fixnum | Kind::Bignum, Kind::Fixnum | Kind::Bignum) => match (a.as_i64(), b.as_i64()) {
            (Some(x), Some(y)) => x == y,
            (None, None) => match (a.as_bignum_bytes(), b.as_bignum_bytes()) {
                (Some((x_negative, x)), Some((y_negative, y))) => {
                    x_negative == y_negative && trim_high_zeros(x) == trim_high_zeros(y)
                }
                _ => false,
            },
            _ => false,
        },
        (Kind::Float, Kind::Float) => a.as_float_bytes() == b.as_float_bytes(),
        (Kind::Str, Kind::Str) | (Kind::Bytes, Kind::Bytes) => a.as_bytes() == b.as_bytes(),
        (Kind::Symbol, Kind::Symbol) => a.as_symbol_bytes() == b.as_symbol_bytes(),
        (Kind::Array, Kind::Array) => {
            if depth == 0 || *steps == 0 {
                return false;
            }
            *steps -= 1;
            a.len() == b.len()
                && a.array()
                    .zip(b.array())
                    .all(|(x, y)| value_eq_within(x, y, depth - 1, steps))
        }
        _ => false,
    }
}
