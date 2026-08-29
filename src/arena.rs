//! The `alloc`-tier DOM: a flat arena of `Copy` nodes addressed by `u32`
//! handles.
//!
//! An object link (Marshal's `@n` back-reference) is a plain index into
//! `Arena::links`, so cycles are representable without `Rc`/`RefCell`.
//! String/float/bignum payloads are `Cow<'a, [u8]>` slices borrowed directly
//! out of the input buffer wherever possible.

use alloc::{borrow::Cow, boxed::Box, collections::BTreeMap, vec::Vec};

/// A handle to a node in an [`Arena`]. Never dereferenced without the arena
/// that produced it.
pub type ValueId = u32;

/// A handle to an entry in an [`Arena`]'s symbol table.
pub type SymId = u32;

/// Sentinel meaning "no symbol" (e.g. a node with no declared class).
pub const NO_SYM: SymId = u32::MAX;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Kind {
    Nil,
    True,
    False,
    Fixnum,
    Bignum,
    Float,
    /// Raw bytes with no declared text encoding at all, which per Ruby's own
    /// Marshal semantics implicitly means `ASCII-8BIT` (binary).
    Bytes,
    /// Text that carried an `E`/`encoding` instance variable at load time -
    /// `Node::b`'s low byte is the declared encoding id (see
    /// [`crate::encoding`]). Use [`crate::value::ValueRef::as_str`] (safely returns
    /// `None` if the bytes don't validate) rather than assuming so, and
    /// [`crate::value::ValueRef::encoding_name`] to see what was declared.
    Str,
    Symbol,
    /// A regexp's source is always raw `Bytes`-like storage; `Node::b`
    /// packs the option bits in its low byte and (per the same tagging
    /// rules as `Str`) the declared encoding id in the next byte up -
    /// `0` (`ASCII-8BIT`) when no `E`/`encoding` ivar was present.
    Regexp,
    Array,
    Hash,
    Struct,
    Object,
    Class,
    Module,
}

/// Bit flags on a [`Node`]. Plain constants rather than an external
/// `bitflags` dependency - five bits don't need a macro.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Flags(pub u8);

impl Flags {
    pub const NONE: Self = Self(0);
    /// `Module` was written with the legacy `TYPE_MODULE_OLD` tag.
    pub const OLD_MODULE: Self = Self(1 << 0);
    /// `Hash` carries a trailing default-value entry.
    pub const HAS_DEFAULT: Self = Self(1 << 1);
    /// The value is an instance of a subclass of a built-in type
    /// (`TYPE_UCLASS`) - its `class` field is the subclass name, distinct
    /// from whatever the bare `Kind` would imply.
    pub const USER_CLASS: Self = Self(1 << 2);
    /// `Bignum` is negative.
    pub const NEGATIVE: Self = Self(1 << 3);
    /// At least one entry in `Arena::extensions` applies to this node.
    pub const EXTENDED: Self = Self(1 << 4);
    /// This value's bytes came from a `_dump`/`_load` (`TYPE_USERDEF`)
    /// round-trip. Always paired with `Kind::Bytes` - Ruby's own loader
    /// treats a user-defined value as its raw dumped bytes too, which is
    /// exactly what lets the same encoding-ivar handling that applies to
    /// plain strings also apply here for free.
    pub const USER_DEFINED: Self = Self(1 << 5);
    /// This value is the result of `_dump_data`/`_load_data`
    /// (`TYPE_DATA`). The node's `kind`/`a`/`b` describe whatever the
    /// wrapped value actually was (an `Object`, an `Array`, ...) - Data
    /// does not have a shape of its own, just an extra layer of identity
    /// and a class name.
    pub const DATA: Self = Self(1 << 6);
    /// This value is the result of `marshal_dump`/`marshal_load`
    /// (`TYPE_USRMARSHAL`); see [`Flags::DATA`] - same story.
    pub const USER_MARSHAL: Self = Self(1 << 7);

    #[inline]
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[inline]
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// One arena entry. 16 bytes, `Copy`, no drop glue - a 100k-node graph is a
/// 1.6 MB flat `Vec`.
#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub kind: Kind,
    pub flags: Flags,
    _pad: u16,
    /// The node's declared class/module name, or [`NO_SYM`]. Meaningful for
    /// every kind (a bare `Array` can be a `MyArray < Array` instance via
    /// `TYPE_UCLASS`), not just `Object`/`Struct`.
    pub class: SymId,
    /// Kind-specific payload - see each [`Kind`] variant's doc comment.
    pub a: u32,
    pub b: u32,
}

impl Node {
    #[inline]
    const fn new(kind: Kind) -> Self {
        Self {
            kind,
            flags: Flags::NONE,
            _pad: 0,
            class: NO_SYM,
            a: 0,
            b: 0,
        }
    }
}

/// The DOM produced by [`crate::load::load`] and consumed by
/// [`crate::dump::dump`].
///
/// Borrows from the input buffer where possible (`'a`); call
/// [`Arena::into_owned`] to detach from it.
pub struct Arena<'a> {
    pub(crate) nodes: Vec<Node>,
    /// Flat storage for `Array` elements and `Hash` key/value pairs
    /// (`[k0, v0, k1, v1, ...]`, plus one trailing default-value id when
    /// `Flags::HAS_DEFAULT` is set).
    pub(crate) children: Vec<ValueId>,
    /// Flat storage for `Object` instance variables and `Struct` members,
    /// both shaped as ordered (name, value) pairs.
    pub(crate) members: Vec<(SymId, ValueId)>,
    /// The interned symbol table; also doubles as Marshal's positional
    /// symlink table during loading.
    pub(crate) symbols: Vec<Cow<'a, [u8]>>,
    /// Backing storage for string/bytes/float/bignum/class/module payloads.
    pub(crate) blobs: Vec<Cow<'a, [u8]>>,
    /// Marshal's positional object-link table.
    pub(crate) links: Vec<ValueId>,
    /// `(ValueId, module SymId)` pairs recording `Module#extend`ed values -
    /// rare, so a linear side table rather than a slot on every `Node`.
    pub(crate) extensions: Vec<(ValueId, SymId)>,
    /// `(ValueId, blob index)` pairs for a `Str`/`Regexp` value whose
    /// declared encoding name isn't in [`crate::encoding::ENCODING_NAMES`]
    /// (tagged [`crate::encoding::ENCODING_CUSTOM`] on the node) - rare, so
    /// a side table rather than a slot on every `Node`, exactly like
    /// `extensions` above.
    pub(crate) custom_encodings: Vec<(ValueId, u32)>,
    pub(crate) root: ValueId,
    /// Content -> `SymId`, used only while building the arena (loading or
    /// via a builder) so repeated symbol content interns to one id.
    pub(crate) sym_intern: BTreeMap<Box<[u8]>, SymId>,
}

#[allow(clippy::cast_possible_truncation)]
impl<'a> Arena<'a> {
    pub(crate) const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            children: Vec::new(),
            members: Vec::new(),
            symbols: Vec::new(),
            blobs: Vec::new(),
            links: Vec::new(),
            extensions: Vec::new(),
            custom_encodings: Vec::new(),
            root: 0,
            sym_intern: BTreeMap::new(),
        }
    }

    #[inline]
    #[must_use]
    pub const fn root(&self) -> ValueId {
        self.root
    }

    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    #[inline]
    #[must_use]
    pub fn node(&self, id: ValueId) -> &Node {
        &self.nodes[id as usize]
    }

    #[inline]
    #[must_use]
    pub fn symbol_bytes(&self, id: SymId) -> &[u8] {
        &self.symbols[id as usize]
    }

    #[inline]
    #[must_use]
    pub fn symbol_str(&self, id: SymId) -> Option<&str> {
        core::str::from_utf8(self.symbol_bytes(id)).ok()
    }

    #[inline]
    #[must_use]
    pub fn blob(&self, idx: u32) -> &[u8] {
        &self.blobs[idx as usize]
    }

    #[inline]
    #[must_use]
    pub fn class_of(&self, id: ValueId) -> Option<SymId> {
        let sym = self.node(id).class;
        (sym != NO_SYM).then_some(sym)
    }

    #[inline]
    pub fn extensions_of(&self, id: ValueId) -> impl Iterator<Item = SymId> + '_ {
        self.extensions.iter().filter(move |(v, _)| *v == id).map(|(_, m)| *m)
    }

    /// The raw encoding name for a value tagged
    /// [`crate::encoding::ENCODING_CUSTOM`], or `None` if it wasn't (an
    /// unrecognized name is the only reason a value ends up in this table).
    #[inline]
    #[must_use]
    pub fn custom_encoding_of(&self, id: ValueId) -> Option<&[u8]> {
        let &(_, blob) = self.custom_encodings.iter().find(|(v, _)| *v == id)?;
        Some(self.blob(blob))
    }

    // -- construction, used by `load` and the (future) builder API --

    pub(crate) fn push_node(&mut self, node: Node) -> ValueId {
        let id = self.nodes.len() as u32;
        self.nodes.push(node);
        id
    }

    pub(crate) fn set_node(&mut self, id: ValueId, node: Node) {
        self.nodes[id as usize] = node;
    }

    pub(crate) fn push_blob(&mut self, bytes: Cow<'a, [u8]>) -> u32 {
        let idx = self.blobs.len() as u32;
        self.blobs.push(bytes);
        idx
    }

    pub(crate) fn push_link_entry(&mut self, id: ValueId) {
        self.links.push(id);
    }

    pub(crate) fn resolve_link(&self, idx: u32) -> Option<ValueId> {
        self.links.get(idx as usize).copied()
    }

    /// Interns `bytes` as a symbol, reusing an existing `SymId` if the same
    /// content was already seen during this construction.
    pub(crate) fn intern_symbol(&mut self, bytes: Cow<'a, [u8]>) -> SymId {
        if let Some(&id) = self.sym_intern.get(bytes.as_ref()) {
            return id;
        }
        let id = self.symbols.len() as u32;
        self.sym_intern.insert(Box::from(bytes.as_ref()), id);
        self.symbols.push(bytes);
        id
    }

    pub(crate) fn reserve_children(&mut self, n: usize) -> u32 {
        let start = self.children.len() as u32;
        self.children.resize(self.children.len() + n, 0);
        start
    }

    pub(crate) fn reserve_members(&mut self, n: usize) -> u32 {
        let start = self.members.len() as u32;
        self.members.resize(self.members.len() + n, (NO_SYM, 0));
        start
    }

    pub(crate) fn add_extension(&mut self, id: ValueId, module: SymId) {
        self.extensions.push((id, module));
    }

    pub(crate) fn add_custom_encoding(&mut self, id: ValueId, blob: u32) {
        self.custom_encodings.push((id, blob));
    }

    /// Promotes every borrowed payload to an owned allocation, detaching the
    /// arena from the input buffer's lifetime.
    #[must_use]
    pub fn into_owned(self) -> Arena<'static> {
        Arena {
            nodes: self.nodes,
            children: self.children,
            members: self.members,
            symbols: self.symbols.into_iter().map(|c| Cow::Owned(c.into_owned())).collect(),
            blobs: self.blobs.into_iter().map(|c| Cow::Owned(c.into_owned())).collect(),
            links: self.links,
            extensions: self.extensions,
            custom_encodings: self.custom_encodings,
            root: self.root,
            sym_intern: BTreeMap::new(),
        }
    }
}

impl Node {
    #[inline]
    pub(crate) const fn scalar(kind: Kind) -> Self {
        Self::new(kind)
    }
}

/// Programmatic construction - building an [`Arena`] from scratch (e.g. from
/// the `serde` `Deserialize` impl, or by hand before [`crate::dump::dump`]).
///
/// Everything here takes owned data: a builder has no input buffer to borrow
/// from, so payloads are always `Cow::Owned`.
// `value as u32` (`push_fixnum`) is a bit-preserving reinterpret, not a
// range-narrowing cast - `Node::a` stores a fixnum's bits verbatim and
// `ValueRef::as_i64` casts back with `as i32` to recover them. The
// `.len() as u32` calls are the same "`ValueId` is `u32`" reasoning as the
// main `Arena` impl above.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
impl Arena<'static> {
    #[must_use]
    pub const fn builder() -> Self {
        Self::new()
    }

    pub fn push_nil(&mut self) -> ValueId {
        self.push_node(Node::scalar(Kind::Nil))
    }

    pub fn push_bool(&mut self, value: bool) -> ValueId {
        self.push_node(Node::scalar(if value { Kind::True } else { Kind::False }))
    }

    pub fn push_fixnum(&mut self, value: i32) -> ValueId {
        let mut node = Node::new(Kind::Fixnum);
        node.a = value as u32;
        self.push_node(node)
    }

    /// `magnitude_le` is the absolute value, little-endian.
    pub fn push_bignum(&mut self, negative: bool, magnitude_le: Vec<u8>) -> ValueId {
        let idx = self.push_blob(Cow::Owned(magnitude_le));
        let mut node = Node::new(Kind::Bignum);
        node.a = idx;
        if negative {
            node.flags = node.flags.with(Flags::NEGATIVE);
        }
        self.push_node(node)
    }

    /// `ascii` is the float's textual form, e.g. `b"1.5"`, `b"inf"`.
    pub fn push_float(&mut self, ascii: Vec<u8>) -> ValueId {
        let idx = self.push_blob(Cow::Owned(ascii));
        let mut node = Node::new(Kind::Float);
        node.a = idx;
        self.push_node(node)
    }

    pub fn push_bytes(&mut self, bytes: Vec<u8>) -> ValueId {
        let idx = self.push_blob(Cow::Owned(bytes));
        let mut node = Node::new(Kind::Bytes);
        node.a = idx;
        self.push_node(node)
    }

    /// A UTF-8 Ruby string (`text` is a Rust `String`, so this is always
    /// well-formed) - dumps with a plain `:E => true` ivar, matching Ruby's
    /// own UTF-8 string literals.
    pub fn push_string(&mut self, text: alloc::string::String) -> ValueId {
        self.push_str_with_encoding(text.into_bytes(), crate::encoding::ENCODING_UTF_8)
    }

    /// A Ruby string tagged with a known [`crate::encoding`] id. Use
    /// [`Arena::push_str_with_encoding_name`] if you only have the
    /// encoding's name.
    pub fn push_str_with_encoding(&mut self, bytes: Vec<u8>, encoding_id: u8) -> ValueId {
        let idx = self.push_blob(Cow::Owned(bytes));
        let mut node = Node::new(Kind::Str);
        node.a = idx;
        node.b = u32::from(encoding_id);
        self.push_node(node)
    }

    /// A Ruby string tagged with an encoding named `encoding_name` (e.g.
    /// `b"Shift_JIS"`) - resolved against the fixed table if known, or
    /// recorded verbatim in the custom-encoding side table otherwise.
    pub fn push_str_with_encoding_name(&mut self, bytes: Vec<u8>, encoding_name: &[u8]) -> ValueId {
        let idx = self.push_blob(Cow::Owned(bytes));
        let mut node = Node::new(Kind::Str);
        node.a = idx;
        let id = Self::tag_encoding_name(encoding_name);
        node.b = u32::from(id);
        let value = self.push_node(node);
        if id == crate::encoding::ENCODING_CUSTOM {
            let blob = self.push_blob(Cow::Owned(encoding_name.to_vec()));
            self.add_custom_encoding(value, blob);
        }
        value
    }

    pub fn push_symbol(&mut self, bytes: Vec<u8>) -> ValueId {
        let sym = self.intern_symbol(Cow::Owned(bytes));
        let mut node = Node::new(Kind::Symbol);
        node.a = sym;
        self.push_node(node)
    }

    /// An untagged regexp (no `E`/`encoding` ivar - encoding implicitly
    /// `ASCII-8BIT`). Use [`Arena::push_regexp_with_encoding_name`] for one
    /// with a declared encoding.
    pub fn push_regexp(&mut self, source: Vec<u8>, options: u8) -> ValueId {
        let idx = self.push_blob(Cow::Owned(source));
        let mut node = Node::new(Kind::Regexp);
        node.a = idx;
        node.b = u32::from(options);
        self.push_node(node)
    }

    /// A regexp tagged with an encoding named `encoding_name`, same
    /// resolution rule as [`Arena::push_str_with_encoding_name`].
    pub fn push_regexp_with_encoding_name(&mut self, source: Vec<u8>, options: u8, encoding_name: &[u8]) -> ValueId {
        let idx = self.push_blob(Cow::Owned(source));
        let mut node = Node::new(Kind::Regexp);
        node.a = idx;
        let id = Self::tag_encoding_name(encoding_name);
        node.b = u32::from(options) | (u32::from(id) << 8);
        let value = self.push_node(node);
        if id == crate::encoding::ENCODING_CUSTOM {
            let blob = self.push_blob(Cow::Owned(encoding_name.to_vec()));
            self.add_custom_encoding(value, blob);
        }
        value
    }

    /// Resolves an encoding name to its id, without recording it in the
    /// custom-encoding side table (callers needing that do so themselves,
    /// since they must do it after the value's own `ValueId` exists).
    fn tag_encoding_name(encoding_name: &[u8]) -> u8 {
        crate::encoding::encoding_id(encoding_name).unwrap_or(crate::encoding::ENCODING_CUSTOM)
    }

    pub fn push_array(&mut self, elements: &[ValueId]) -> ValueId {
        let start = self.reserve_children(elements.len());
        self.children[start as usize..start as usize + elements.len()].copy_from_slice(elements);
        let mut node = Node::new(Kind::Array);
        node.a = start;
        node.b = elements.len() as u32;
        self.push_node(node)
    }

    /// `pairs` is `[key, value, key, value, ...]`; `default` is the hash's
    /// default value, if any.
    pub fn push_hash(&mut self, pairs: &[(ValueId, ValueId)], default: Option<ValueId>) -> ValueId {
        let extra = usize::from(default.is_some());
        let start = self.reserve_children(pairs.len() * 2 + extra);
        for (i, &(k, v)) in pairs.iter().enumerate() {
            self.children[start as usize + i * 2] = k;
            self.children[start as usize + i * 2 + 1] = v;
        }
        let mut node = Node::new(Kind::Hash);
        node.a = start;
        node.b = pairs.len() as u32;
        if let Some(default) = default {
            self.children[start as usize + pairs.len() * 2] = default;
            node.flags = node.flags.with(Flags::HAS_DEFAULT);
        }
        self.push_node(node)
    }

    pub fn push_struct(&mut self, class: Vec<u8>, members: &[(Vec<u8>, ValueId)]) -> ValueId {
        let class_sym = self.intern_symbol(Cow::Owned(class));
        let start = self.reserve_members(members.len());
        for (i, (name, value)) in members.iter().enumerate() {
            let sym = self.intern_symbol(Cow::Owned(name.clone()));
            self.members[start as usize + i] = (sym, *value);
        }
        let mut node = Node::new(Kind::Struct);
        node.class = class_sym;
        node.a = start;
        node.b = members.len() as u32;
        self.push_node(node)
    }

    pub fn push_object(&mut self, class: Vec<u8>, ivars: &[(Vec<u8>, ValueId)]) -> ValueId {
        let class_sym = self.intern_symbol(Cow::Owned(class));
        let start = self.reserve_members(ivars.len());
        for (i, (name, value)) in ivars.iter().enumerate() {
            let sym = self.intern_symbol(Cow::Owned(name.clone()));
            self.members[start as usize + i] = (sym, *value);
        }
        let mut node = Node::new(Kind::Object);
        node.class = class_sym;
        node.a = start;
        node.b = ivars.len() as u32;
        self.push_node(node)
    }

    pub fn push_class(&mut self, path: Vec<u8>) -> ValueId {
        let idx = self.push_blob(Cow::Owned(path));
        let mut node = Node::new(Kind::Class);
        node.a = idx;
        self.push_node(node)
    }

    pub fn push_module(&mut self, path: Vec<u8>, old: bool) -> ValueId {
        let idx = self.push_blob(Cow::Owned(path));
        let mut node = Node::new(Kind::Module);
        node.a = idx;
        if old {
            node.flags = node.flags.with(Flags::OLD_MODULE);
        }
        self.push_node(node)
    }

    pub fn set_root(&mut self, id: ValueId) {
        self.root = id;
    }
}
