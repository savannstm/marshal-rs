//! A flat arena of `Copy` nodes addressed by `u32` handles. Object links
//! (`@n`) are plain indices, so cycles need no `Rc`/`RefCell`.
//!
//! Slot setters (`set_array_*`, `set_hash_*`, `set_member_*`) repoint one
//! container slot. Content setters (`set_*_content`, ...) rewrite a node's
//! payload in place, so every shared reference sees the change. There is no
//! symbol content setter: symbols are interned, so it would rewrite every
//! identical symbol.

use alloc::{borrow::Cow, collections::BTreeMap, vec::Vec};

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
    /// Raw bytes with no declared encoding (implicitly `ASCII-8BIT`).
    Bytes,
    /// Text with a declared encoding (`E`/`encoding` ivar); `Node::b`'s low byte is its id.
    Str,
    Symbol,
    /// Regexp source; `Node::b` holds the option bits in its low byte and the encoding id in the next.
    Regexp,
    Array,
    Hash,
    Struct,
    Object,
    Class,
    Module,
}

/// Bit flags on a node.
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
    /// `_dump` result (`TYPE_USERDEF`). Always paired with `Kind::Bytes`.
    pub const USER_DEFINED: Self = Self(1 << 5);
    /// `_dump_data` wrapper (`TYPE_DATA`); the body is a node of its own, see [`crate::value::ValueRef::body`].
    pub const DATA: Self = Self(1 << 6);
    /// `marshal_dump` wrapper (`TYPE_USRMARSHAL`); see [`Flags::DATA`].
    pub const USER_MARSHAL: Self = Self(1 << 7);
    /// Either wrapper flag; see [`Arena::body_of`].
    pub(crate) const WRAPPER: Self = Self(1 << 6 | 1 << 7);

    #[inline]
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[inline]
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    #[inline]
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[inline]
    #[must_use]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

/// One arena entry. 16 bytes, `Copy`, no drop glue - a 100k-node graph is a
/// 1.6 MB flat `Vec`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Node {
    pub kind: Kind,
    pub flags: Flags,
    _pad: u16,
    /// Declared class/module name, or [`NO_SYM`]. Set for every kind, not only `Object`/`Struct`.
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
    /// `Array` elements and `Hash` pairs (`[k0, v0, k1, v1, ...]`, plus a trailing default if `Flags::HAS_DEFAULT`).
    pub(crate) children: Vec<ValueId>,
    /// Flat storage for `Object` instance variables and `Struct` members,
    /// both shaped as ordered (name, value) pairs.
    pub(crate) members: Vec<(SymId, ValueId)>,
    /// The interned symbol table; also doubles as Marshal's positional
    /// symlink table during loading.
    pub(crate) symbols: Vec<Cow<'a, [u8]>>,
    /// The encoding id of every symbol, parallel to `symbols`: what its `TYPE_IVAR` wrapper declared, with
    /// [`crate::encoding::ENCODING_ASCII_8BIT`] meaning no wrapper at all (see [`SymbolEncoding`](crate::SymbolEncoding)).
    pub(crate) sym_encodings: Vec<u8>,
    /// `(symbol, blob index)` for a symbol whose encoding name is not in [`crate::encoding::ENCODING_NAMES`]
    /// (tagged [`crate::encoding::ENCODING_CUSTOM`]) - rare, so a linear side table.
    pub(crate) sym_custom: Vec<(SymId, u32)>,
    /// Backing storage for string/bytes/float/bignum/class/module payloads.
    pub(crate) blobs: Vec<Cow<'a, [u8]>>,
    /// Marshal's positional object-link table.
    pub(crate) links: Vec<ValueId>,
    /// `(ValueId, module)` for `Module#extend`ed values, in stream order.
    pub(crate) extensions: Vec<(ValueId, SymId)>,
    /// `(ValueId, blob index)` for a `Str`/`Regexp` whose encoding name is not in the fixed table.
    pub(crate) custom_encodings: Vec<(ValueId, u32)>,
    /// String objects written for symbol encoding names; numbered like any object, so links can target them.
    pub(crate) name_nodes: Vec<ValueId>,
    /// `(target, name, value)` for instance variables besides the encoding, sorted by `target`.
    pub(crate) extra_ivars: Vec<(ValueId, SymId, ValueId)>,
    /// `(wrapper, body)` for `_dump_data`/`marshal_dump` wrappers, sorted by wrapper.
    pub(crate) wrappers: Vec<(ValueId, ValueId)>,
    pub(crate) root: ValueId,
    /// Content -> `SymId`, used while building so equal symbols intern to one id.
    pub(crate) sym_intern: BTreeMap<Cow<'a, [u8]>, SymId>,
    /// Symbols sharing bytes with an earlier one but not its encoding; almost always empty.
    sym_variants: Vec<SymId>,
    /// How many of `symbols` `sym_intern` and `sym_variants` cover.
    sym_indexed: usize,
}

#[allow(clippy::cast_possible_truncation)]
impl<'a> Arena<'a> {
    pub(crate) const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            children: Vec::new(),
            members: Vec::new(),
            symbols: Vec::new(),
            sym_encodings: Vec::new(),
            sym_custom: Vec::new(),
            blobs: Vec::new(),
            links: Vec::new(),
            extensions: Vec::new(),
            custom_encodings: Vec::new(),
            name_nodes: Vec::new(),
            extra_ivars: Vec::new(),
            wrappers: Vec::new(),
            root: 0,
            sym_intern: BTreeMap::new(),
            sym_variants: Vec::new(),
            sym_indexed: 0,
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
    pub(crate) fn node(&self, id: ValueId) -> &Node {
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

    /// The encoding id a symbol was tagged with (see [`mod@crate::encoding`]); [`crate::encoding::ENCODING_ASCII_8BIT`]
    /// when it carried no tag. [`Arena::symbol_effective_encoding`] is what Ruby would call its encoding.
    #[inline]
    #[must_use]
    pub fn symbol_encoding(&self, id: SymId) -> u8 {
        self.sym_encodings[id as usize]
    }

    /// The name of a symbol's tag when it is not in the fixed table ([`crate::encoding::ENCODING_CUSTOM`]).
    #[must_use]
    pub fn symbol_custom_encoding(&self, id: SymId) -> Option<&[u8]> {
        let &(_, blob) = self.sym_custom.iter().find(|&&(sym, _)| sym == id)?;
        Some(self.blob(blob))
    }

    /// A symbol's encoding as Ruby sees it: its tag, or - with no tag - US-ASCII for an ASCII name and
    /// ASCII-8BIT for any other.
    #[must_use]
    pub fn symbol_effective_encoding(&self, id: SymId) -> u8 {
        match self.symbol_encoding(id) {
            crate::encoding::ENCODING_ASCII_8BIT if self.symbol_bytes(id).is_ascii() => {
                crate::encoding::ENCODING_US_ASCII
            }
            tag => tag,
        }
    }

    #[inline]
    #[must_use]
    pub fn blob(&self, idx: u32) -> &[u8] {
        &self.blobs[idx as usize]
    }

    #[inline]
    #[must_use]
    pub fn class_of(&self, id: ValueId) -> Option<SymId> {
        let sym = self.nodes.get(id as usize)?.class;
        (sym != NO_SYM).then_some(sym)
    }

    /// The modules `id` was `extend`ed with, in stream order.
    #[inline]
    pub fn extensions_of(&self, id: ValueId) -> impl Iterator<Item = SymId> + '_ {
        // Most arenas have none; skip the two searches then (this runs for every node of a dump).
        let run = if self.extensions.is_empty() {
            &[]
        } else {
            let lo = self.extensions.partition_point(|e| e.0 < id);
            let hi = self.extensions.partition_point(|e| e.0 <= id);
            &self.extensions[lo..hi]
        };
        run.iter().map(|&(_, module)| module)
    }

    /// The body of a `_dump_data`/`marshal_dump` wrapper node, `None` for any other node.
    #[inline]
    #[must_use]
    pub fn body_of(&self, id: ValueId) -> Option<ValueId> {
        if !self.nodes.get(id as usize)?.flags.intersects(Flags::WRAPPER) {
            return None;
        }
        let at = self.wrappers.binary_search_by_key(&id, |&(wrapper, _)| wrapper).ok()?;
        Some(self.wrappers[at].1)
    }

    /// `id`, or the body it stands for through nested wrappers. Bounded, so a cyclic stream
    /// (`U:A U:B @0`) stops on a wrapper that reads as `nil`.
    #[inline]
    pub(crate) fn resolve(&self, mut id: ValueId) -> ValueId {
        for _ in 0..=self.wrappers.len() {
            match self.body_of(id) {
                Some(body) => id = body,
                None => return id,
            }
        }
        id
    }

    /// Sorts the side tables looked up by node id, after a load.
    pub(crate) fn finish_load(&mut self) {
        self.extra_ivars.sort_by_key(|&(target, ..)| target);
        self.custom_encodings.sort_by_key(|&(target, _)| target);
        self.wrappers.sort_by_key(|&(wrapper, _)| wrapper);
        // Extensions complete innermost-first, so a value's run comes out in reverse stream order.
        self.extensions.sort_by_key(|&(target, _)| target);
        let mut start = 0;
        while start < self.extensions.len() {
            let target = self.extensions[start].0;
            let end = start + self.extensions[start..].partition_point(|e| e.0 == target);
            self.extensions[start..end].reverse();
            start = end;
        }
    }

    /// The instance variables `id` carried besides its encoding, in stream order.
    #[inline]
    pub(crate) fn extra_ivars_of(&self, id: ValueId) -> &[(ValueId, SymId, ValueId)] {
        if self.extra_ivars.is_empty() {
            return &[];
        }
        let lo = self.extra_ivars.partition_point(|e| e.0 < id);
        let hi = self.extra_ivars.partition_point(|e| e.0 <= id);
        &self.extra_ivars[lo..hi]
    }

    /// The raw name of an encoding outside the fixed table; `None` otherwise.
    #[inline]
    #[must_use]
    pub fn custom_encoding_of(&self, id: ValueId) -> Option<&[u8]> {
        let at = self
            .custom_encodings
            .binary_search_by_key(&id, |&(target, _)| target)
            .ok()?;
        Some(self.blob(self.custom_encodings[at].1))
    }

    pub(crate) fn push_node(&mut self, node: Node) -> ValueId {
        let id = self.nodes.len() as u32;
        self.nodes.push(node);
        id
    }

    pub(crate) fn set_node(&mut self, id: ValueId, node: Node) {
        self.nodes[id as usize] = node;
    }

    pub(crate) fn push_blob(&mut self, bytes: Cow<'a, [u8]>) -> u32 {
        // Marshal lengths are packed `i32`s; a larger payload would dump with a wrong length.
        assert!(i32::try_from(bytes.len()).is_ok(), "payload longer than i32::MAX bytes");
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

    /// Interns `bytes` with the default tag for them; used for class, member and ivar names.
    pub(crate) fn intern_symbol(&mut self, bytes: Cow<'a, [u8]>) -> SymId {
        let encoding = crate::encoding::default_symbol_encoding(&bytes);
        self.intern_symbol_tagged(bytes, encoding, None)
    }

    /// [`Arena::intern_symbol`] for borrowed bytes: copies them only if the symbol is new.
    pub(crate) fn intern_bytes(&mut self, bytes: &[u8]) -> SymId {
        let encoding = crate::encoding::default_symbol_encoding(bytes);
        self.sync_intern();
        if let Some(id) = self.find_symbol(bytes, encoding, None) {
            return id;
        }
        self.intern_symbol_tagged(Cow::Owned(bytes.to_vec()), encoding, None)
    }

    /// Interns `bytes` with `encoding` (and `custom` for `ENCODING_CUSTOM`): one `SymId` per (content, tag).
    pub(crate) fn intern_symbol_tagged(&mut self, bytes: Cow<'a, [u8]>, encoding: u8, custom: Option<&[u8]>) -> SymId {
        self.sync_intern();
        if let Some(id) = self.find_symbol(&bytes, encoding, custom) {
            return id;
        }
        let id = self.symbols.len() as u32;
        // `sym_intern` keeps the first symbol per content; later ones with the same bytes are variants.
        if self.sym_intern.contains_key(bytes.as_ref()) {
            self.sym_variants.push(id);
        } else {
            self.sym_intern.insert(Self::intern_key(&bytes), id);
        }
        self.symbols.push(bytes);
        self.sym_encodings.push(encoding);
        if let Some(name) = custom {
            let blob = self.push_blob(Cow::Owned(name.to_vec()));
            self.sym_custom.push((id, blob));
        }
        self.sym_indexed = self.symbols.len();
        id
    }

    /// The symbol with this content and tag, if any. Needs `sync_intern` first.
    fn find_symbol(&self, bytes: &[u8], encoding: u8, custom: Option<&[u8]>) -> Option<SymId> {
        let matches = |id: SymId| {
            self.sym_encodings[id as usize] == encoding
                && (encoding != crate::encoding::ENCODING_CUSTOM || self.symbol_custom_encoding(id) == custom)
        };
        let first = *self.sym_intern.get(bytes)?;
        if matches(first) {
            return Some(first);
        }
        self.sym_variants
            .iter()
            .copied()
            .find(|&id| self.symbols[id as usize].as_ref() == bytes && matches(id))
    }

    /// Borrowed keys stay borrowed; owned ones are cloned.
    fn intern_key(bytes: &Cow<'a, [u8]>) -> Cow<'a, [u8]> {
        match bytes {
            Cow::Borrowed(slice) => Cow::Borrowed(slice),
            Cow::Owned(vec) => Cow::Owned(vec.clone()),
        }
    }

    /// Indexes symbols missing from the lookup table; `into_owned` leaves it empty, so read-only arenas never pay for it.
    fn sync_intern(&mut self) {
        for id in self.sym_indexed..self.symbols.len() {
            let key = Self::intern_key(&self.symbols[id]);
            if self.sym_intern.contains_key(key.as_ref()) {
                self.sym_variants.push(id as SymId);
            } else {
                self.sym_intern.insert(key, id as SymId);
            }
        }
        self.sym_indexed = self.symbols.len();
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

    /// The node a stream gave the encoding name `name`, if it has one.
    pub(crate) fn name_node(&self, name: &[u8]) -> Option<ValueId> {
        self.name_nodes
            .iter()
            .copied()
            .find(|&id| self.blob(self.node(id).a) == name)
    }

    pub(crate) fn add_custom_encoding(&mut self, id: ValueId, blob: u32) {
        self.custom_encodings.push((id, blob));
    }

    /// Copies every borrowed payload, detaching the arena from the input's lifetime.
    #[must_use]
    pub fn into_owned(self) -> Arena<'static> {
        Arena {
            nodes: self.nodes,
            children: self.children,
            members: self.members,
            symbols: self.symbols.into_iter().map(|c| Cow::Owned(c.into_owned())).collect(),
            sym_encodings: self.sym_encodings,
            sym_custom: self.sym_custom,
            blobs: self.blobs.into_iter().map(|c| Cow::Owned(c.into_owned())).collect(),
            links: self.links,
            extensions: self.extensions,
            custom_encodings: self.custom_encodings,
            name_nodes: self.name_nodes,
            extra_ivars: self.extra_ivars,
            wrappers: self.wrappers,
            root: self.root,
            sym_intern: BTreeMap::new(),
            sym_variants: Vec::new(),
            sym_indexed: 0,
        }
    }
}

impl Node {
    #[inline]
    pub(crate) const fn scalar(kind: Kind) -> Self {
        Self::new(kind)
    }
}

impl Arena<'static> {
    /// A new arena to build by hand, with `nil` as its root.
    #[must_use]
    pub fn builder() -> Self {
        // Node 0 is the root, so dumping an untouched builder writes `nil`.
        let mut arena = Self::new();
        arena.push_nil();
        arena
    }
}

/// Construction and mutation. Payloads are owned, so this works on fresh and loaded arenas alike.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]
impl Arena<'_> {
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

    /// `magnitude_le` is the absolute value, little-endian. High zero bytes are dropped, as Ruby drops them.
    pub fn push_bignum(&mut self, negative: bool, mut magnitude_le: Vec<u8>) -> ValueId {
        magnitude_le.truncate(crate::bignum::normalize_magnitude(&magnitude_le).len());
        let negative = negative && !magnitude_le.is_empty();
        let idx = self.push_blob(Cow::Owned(magnitude_le));
        let mut node = Node::new(Kind::Bignum);
        node.a = idx;
        if negative {
            node.flags = node.flags.with(Flags::NEGATIVE);
        }
        self.push_node(node)
    }

    /// `ascii` is the float's textual form, e.g. `b"1.5"`, `b"inf"`, stored verbatim and unchecked: Ruby
    /// before 1.9 follows the text with a NUL and mantissa bytes, which must round-trip exactly.
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

    /// A UTF-8 Ruby string (`text` is a Rust `String`, so always well-formed).
    /// Dumps with a plain `:E => true` ivar, as Ruby's own UTF-8 literals do.
    pub fn push_string(&mut self, text: alloc::string::String) -> ValueId {
        self.push_str_node(text.into_bytes(), crate::encoding::ENCODING_UTF_8)
    }

    /// A string tagged with a known encoding id. `None` if the id is not in the fixed table.
    pub fn push_str_with_encoding(&mut self, bytes: Vec<u8>, encoding_id: u8) -> Option<ValueId> {
        crate::encoding::encoding_name(encoding_id)?;
        Some(self.push_str_node(bytes, encoding_id))
    }

    fn push_str_node(&mut self, bytes: Vec<u8>, encoding_id: u8) -> ValueId {
        let idx = self.push_blob(Cow::Owned(bytes));
        let mut node = Node::new(Kind::Str);
        node.a = idx;
        node.b = u32::from(encoding_id);
        self.push_node(node)
    }

    /// A string tagged with an encoding name; names outside the fixed table are kept verbatim.
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

    /// A symbol with the default tag for its bytes (see [`crate::encoding::default_symbol_encoding`]).
    pub fn push_symbol(&mut self, bytes: Vec<u8>) -> ValueId {
        let sym = self.intern_symbol(Cow::Owned(bytes));
        self.push_symbol_node(sym)
    }

    fn push_symbol_node(&mut self, sym: SymId) -> ValueId {
        let mut node = Node::new(Kind::Symbol);
        node.a = sym;
        self.push_node(node)
    }

    /// A symbol with a known encoding id (`ENCODING_ASCII_8BIT` = untagged). `None` if the id is not in the fixed table.
    pub fn push_symbol_with_encoding(&mut self, bytes: Vec<u8>, encoding_id: u8) -> Option<ValueId> {
        crate::encoding::encoding_name(encoding_id)?;
        let sym = self.intern_symbol_tagged(Cow::Owned(bytes), encoding_id, None);
        Some(self.push_symbol_node(sym))
    }

    /// A symbol tagged with an encoding name; resolved as in [`Arena::push_str_with_encoding_name`].
    pub fn push_symbol_with_encoding_name(&mut self, bytes: Vec<u8>, encoding_name: &[u8]) -> ValueId {
        let (id, custom) = Self::resolve_encoding_name(encoding_name);
        let sym = self.intern_symbol_tagged(Cow::Owned(bytes), id, custom);
        self.push_symbol_node(sym)
    }

    /// The id of an encoding name, and the name itself if it is not in the fixed table.
    fn resolve_encoding_name(encoding_name: &[u8]) -> (u8, Option<&[u8]>) {
        crate::encoding::encoding_id(encoding_name)
            .map_or((crate::encoding::ENCODING_CUSTOM, Some(encoding_name)), |id| (id, None))
    }

    /// An untagged (`ASCII-8BIT`) regexp; see [`Arena::push_regexp_with_encoding_name`] for a tagged one.
    pub fn push_regexp(&mut self, source: Vec<u8>, options: u8) -> ValueId {
        let idx = self.push_blob(Cow::Owned(source));
        let mut node = Node::new(Kind::Regexp);
        node.a = idx;
        node.b = u32::from(options);
        self.push_node(node)
    }

    /// A regexp tagged with an encoding name; resolved as in [`Arena::push_str_with_encoding_name`].
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

    /// Resolves an encoding name to its id. Does not record custom names; callers do that once the `ValueId` exists.
    fn tag_encoding_name(encoding_name: &[u8]) -> u8 {
        crate::encoding::encoding_id(encoding_name).unwrap_or(crate::encoding::ENCODING_CUSTOM)
    }

    /// Whether `id` names a node of this arena.
    #[inline]
    #[must_use]
    pub fn contains(&self, id: ValueId) -> bool {
        (id as usize) < self.nodes.len()
    }

    /// `None` if any element is not a node of this arena.
    pub fn push_array(&mut self, elements: &[ValueId]) -> Option<ValueId> {
        if !elements.iter().all(|&id| self.contains(id)) {
            return None;
        }
        let start = self.reserve_children(elements.len());
        self.children[start as usize..start as usize + elements.len()].copy_from_slice(elements);
        let mut node = Node::new(Kind::Array);
        node.a = start;
        node.b = elements.len() as u32;
        Some(self.push_node(node))
    }

    /// `None` if any id is not a node of this arena.
    pub fn push_hash(&mut self, pairs: &[(ValueId, ValueId)], default: Option<ValueId>) -> Option<ValueId> {
        let valid = pairs.iter().all(|&(k, v)| self.contains(k) && self.contains(v))
            && default.map_or(true, |id| self.contains(id));
        if !valid {
            return None;
        }
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
        Some(self.push_node(node))
    }

    /// `None` if `class` is empty or any value is not a node of this arena.
    pub fn push_struct(&mut self, class: Vec<u8>, members: &[(Vec<u8>, ValueId)]) -> Option<ValueId> {
        if class.is_empty() || !members.iter().all(|&(_, id)| self.contains(id)) {
            return None;
        }
        let class_sym = self.intern_symbol(Cow::Owned(class));
        let start = self.reserve_members(members.len());
        for (i, (name, value)) in members.iter().enumerate() {
            let sym = self.intern_bytes(name);
            self.members[start as usize + i] = (sym, *value);
        }
        Some(self.push_members_node(Kind::Struct, class_sym, start, members.len()))
    }

    fn push_members_node(&mut self, kind: Kind, class: SymId, start: u32, len: usize) -> ValueId {
        let mut node = Node::new(kind);
        node.class = class;
        node.a = start;
        node.b = len as u32;
        self.push_node(node)
    }

    /// [`Arena::push_struct`] / [`Arena::push_object`] (by `kind`) with names that are already symbols, tags
    /// included.
    #[cfg(feature = "serde")]
    pub(crate) fn push_members_sym(
        &mut self,
        kind: Kind,
        class: SymId,
        members: &[(SymId, ValueId)],
    ) -> Option<ValueId> {
        if self.symbol_bytes(class).is_empty() || !members.iter().all(|&(_, id)| self.contains(id)) {
            return None;
        }
        let start = self.reserve_members(members.len());
        self.members[start as usize..start as usize + members.len()].copy_from_slice(members);
        Some(self.push_members_node(kind, class, start, members.len()))
    }

    /// The symbol for `bytes` carrying the encoding named `encoding`, or the one the builder gives a name
    /// ([`crate::encoding::default_symbol_encoding`]) when there is none.
    #[cfg(feature = "serde")]
    pub(crate) fn name_symbol(&mut self, bytes: Vec<u8>, encoding: Option<&[u8]>) -> SymId {
        match encoding {
            None => self.intern_symbol(Cow::Owned(bytes)),
            Some(name) => {
                let (id, custom) = Self::resolve_encoding_name(name);
                self.intern_symbol_tagged(Cow::Owned(bytes), id, custom)
            }
        }
    }

    /// The name of the encoding `sym` is tagged with, when that is not the tag its bytes would be given anyway
    /// (so, `None` for nearly every name); for the envelope, which says a tag only when it has to.
    #[must_use]
    pub fn symbol_unusual_tag(&self, sym: SymId) -> Option<&[u8]> {
        let tag = self.symbol_encoding(sym);
        if tag == crate::encoding::default_symbol_encoding(self.symbol_bytes(sym)) {
            return None;
        }
        if tag == crate::encoding::ENCODING_CUSTOM {
            self.symbol_custom_encoding(sym)
        } else {
            crate::encoding::encoding_name(tag)
        }
    }

    /// `None` if `class` is empty or any ivar value is not a node of this arena.
    pub fn push_object(&mut self, class: Vec<u8>, ivars: &[(Vec<u8>, ValueId)]) -> Option<ValueId> {
        if class.is_empty() || !ivars.iter().all(|&(_, id)| self.contains(id)) {
            return None;
        }
        let class_sym = self.intern_symbol(Cow::Owned(class));
        let start = self.reserve_members(ivars.len());
        for (i, (name, value)) in ivars.iter().enumerate() {
            let sym = self.intern_bytes(name);
            self.members[start as usize + i] = (sym, *value);
        }
        Some(self.push_members_node(Kind::Object, class_sym, start, ivars.len()))
    }

    /// `None` if `path` is empty.
    pub fn push_class(&mut self, path: Vec<u8>) -> Option<ValueId> {
        let utf8 = Self::is_non_ascii_utf8(&path);
        let id = self.push_path(Kind::Class, path, false)?;
        self.tag_path(id, utf8);
        Some(id)
    }

    /// `None` if `path` is empty.
    pub fn push_module(&mut self, path: Vec<u8>, old: bool) -> Option<ValueId> {
        let utf8 = Self::is_non_ascii_utf8(&path);
        let id = self.push_path(Kind::Module, path, old)?;
        self.tag_path(id, utf8);
        Some(id)
    }

    /// A class or module with no UTF-8 tag, whatever its bytes; the caller adds one. `None` if `path` is empty.
    pub(crate) fn push_path(&mut self, kind: Kind, path: Vec<u8>, old: bool) -> Option<ValueId> {
        if path.is_empty() {
            return None;
        }
        let idx = self.push_blob(Cow::Owned(path));
        let mut node = Node::new(kind);
        node.a = idx;
        if old {
            node.flags = node.flags.with(Flags::OLD_MODULE);
        }
        Some(self.push_node(node))
    }

    const fn is_non_ascii_utf8(path: &[u8]) -> bool {
        !path.is_ascii() && core::str::from_utf8(path).is_ok()
    }

    /// A non-ASCII path is UTF-8 tagged, as Ruby writes it, so Ruby finds the constant.
    fn tag_path(&mut self, id: ValueId, utf8: bool) {
        if utf8 {
            let yes = self.push_bool(true);
            self.push_extra_ivar(id, b"E", yes);
        }
    }

    /// Makes `id` the value [`crate::dump::dump`] writes. `false` if `id` is not a node of this arena.
    pub fn set_root(&mut self, id: ValueId) -> bool {
        let valid = self.contains(id);
        if valid {
            self.root = id;
        }
        valid
    }

    /// A `marshal_dump` wrapper (`TYPE_USRMARSHAL`) of `class` around `body`. `None` if `class` is empty or `body` is not a node.
    pub fn push_user_marshal(&mut self, class: Vec<u8>, body: ValueId) -> Option<ValueId> {
        self.push_wrapper(Flags::USER_MARSHAL, class, body)
    }

    /// A `_dump_data` wrapper (`TYPE_DATA`); see [`Arena::push_user_marshal`].
    pub fn push_data(&mut self, class: Vec<u8>, body: ValueId) -> Option<ValueId> {
        self.push_wrapper(Flags::DATA, class, body)
    }

    fn push_wrapper(&mut self, flag: Flags, class: Vec<u8>, body: ValueId) -> Option<ValueId> {
        if class.is_empty() {
            return None;
        }
        let class = self.intern_symbol(Cow::Owned(class));
        self.push_wrapper_sym(flag, class, body)
    }

    pub(crate) fn push_wrapper_sym(&mut self, flag: Flags, class: SymId, body: ValueId) -> Option<ValueId> {
        if self.symbol_bytes(class).is_empty() || !self.contains(body) {
            return None;
        }
        // The wrapper is the newest node, so no body can lead back to it.
        let mut node = Node::new(Kind::Nil);
        node.flags = flag;
        node.class = class;
        let id = self.push_node(node);
        self.wrappers.push((id, body));
        Some(id)
    }

    /// Makes `id` an instance of subclass `class` of its built-in type (`TYPE_UCLASS`). `false` unless `id` is a
    /// string, regexp, array or hash that is not a `_dump` value, or `class` is empty.
    pub fn set_user_class(&mut self, id: ValueId, class: Vec<u8>) -> bool {
        let sym = self.intern_symbol(Cow::Owned(class));
        self.set_user_class_sym(id, sym)
    }

    pub(crate) fn set_user_class_sym(&mut self, id: ValueId, sym: SymId) -> bool {
        let Some(&node) = self.nodes.get(id as usize) else {
            return false;
        };
        let builtin = matches!(
            node.kind,
            Kind::Bytes | Kind::Str | Kind::Regexp | Kind::Array | Kind::Hash
        );
        if self.symbol_bytes(sym).is_empty()
            || !builtin
            || node.flags.intersects(Flags::USER_DEFINED.with(Flags::WRAPPER))
        {
            return false;
        }
        let node = &mut self.nodes[id as usize];
        node.class = sym;
        node.flags = node.flags.with(Flags::USER_CLASS);
        true
    }

    /// Undoes [`Arena::set_user_class`]; `false` if `id` has no user class.
    pub fn clear_user_class(&mut self, id: ValueId) -> bool {
        let Some(node) = self.nodes.get_mut(id as usize) else {
            return false;
        };
        if !node.flags.contains(Flags::USER_CLASS) {
            return false;
        }
        node.class = NO_SYM;
        node.flags = node.flags.without(Flags::USER_CLASS);
        true
    }

    /// Makes bytes or a string the `_dump` result of `class` (`TYPE_USERDEF`). `false` unless `id` is bytes or
    /// a string without a user class, or `class` is empty.
    pub fn set_user_defined(&mut self, id: ValueId, class: Vec<u8>) -> bool {
        let sym = self.intern_symbol(Cow::Owned(class));
        self.set_user_defined_sym(id, sym)
    }

    pub(crate) fn set_user_defined_sym(&mut self, id: ValueId, sym: SymId) -> bool {
        let Some(&node) = self.nodes.get(id as usize) else {
            return false;
        };
        let text = matches!(node.kind, Kind::Bytes | Kind::Str);
        if self.symbol_bytes(sym).is_empty() || !text || node.flags.intersects(Flags::USER_CLASS.with(Flags::WRAPPER)) {
            return false;
        }
        let node = &mut self.nodes[id as usize];
        node.class = sym;
        node.flags = node.flags.with(Flags::USER_DEFINED);
        true
    }

    /// Whether `id` can carry an `extend` or instance variables. Not `nil`, booleans, fixnums or symbols,
    /// which Ruby cannot extend and the loader shares.
    pub(crate) fn can_carry_extras(&self, id: ValueId) -> bool {
        self.nodes.get(id as usize).is_some_and(|node| {
            !matches!(
                node.kind,
                Kind::Nil | Kind::True | Kind::False | Kind::Fixnum | Kind::Symbol
            ) || node.flags.intersects(Flags::WRAPPER)
        })
    }

    /// Records `Module#extend`ed `module` on `id` (`TYPE_EXTENDED`), in stream order. Stream order is the reverse
    /// of call order: `obj.extend(A).extend(B)` is `push_extension(B)` then `push_extension(A)`.
    /// `false` if `id` cannot be extended or `module` is empty.
    pub fn push_extension(&mut self, id: ValueId, module: Vec<u8>) -> bool {
        let sym = self.intern_symbol(Cow::Owned(module));
        self.push_extension_sym(id, sym)
    }

    pub(crate) fn push_extension_sym(&mut self, id: ValueId, sym: SymId) -> bool {
        if self.symbol_bytes(sym).is_empty() || !self.can_carry_extras(id) {
            return false;
        }
        let at = self.extensions.partition_point(|e| e.0 <= id);
        self.extensions.insert(at, (id, sym));
        true
    }

    /// Gives `target` an instance variable `name` (with its `@`) holding `value`, after any it already has.
    /// On an `Object` it becomes one more member. `false` if either id is not a node, `target` cannot carry
    /// ivars, or `name` is empty.
    pub fn push_extra_ivar(&mut self, target: ValueId, name: &[u8], value: ValueId) -> bool {
        let sym = self.intern_bytes(name);
        self.push_extra_ivar_sym(target, sym, value)
    }

    pub(crate) fn push_extra_ivar_sym(&mut self, target: ValueId, sym: SymId, value: ValueId) -> bool {
        if self.symbol_bytes(sym).is_empty() || !self.can_carry_extras(target) || !self.contains(value) {
            return false;
        }
        let node = self.nodes[target as usize];
        if node.kind == Kind::Object && !node.flags.intersects(Flags::WRAPPER) {
            // Members are one run of the table; the object gets a longer copy of it.
            let len = node.b as usize;
            let start = self.reserve_members(len + 1);
            self.members
                .copy_within(node.a as usize..node.a as usize + len, start as usize);
            self.members[start as usize + len] = (sym, value);
            let node = &mut self.nodes[target as usize];
            node.a = start;
            node.b += 1;
            return true;
        }
        // `push_class`/`push_module` already tag a non-ASCII path UTF-8; an `E` given later replaces that tag.
        if matches!(node.kind, Kind::Class | Kind::Module) && self.symbol_bytes(sym) == b"E" {
            let lo = self.extra_ivars.partition_point(|e| e.0 < target);
            let hi = self.extra_ivars.partition_point(|e| e.0 <= target);
            if let Some(existing) = self.extra_ivars[lo..hi].iter_mut().find(|e| e.1 == sym) {
                existing.2 = value;
                return true;
            }
        }
        let at = self.extra_ivars.partition_point(|e| e.0 <= target);
        self.extra_ivars.insert(at, (target, sym, value));
        true
    }

    /// The `children` index of element `index` of an `Array` or of a `Hash` entry's key; `None` if out of range.
    fn child_slot(&self, id: ValueId, kind: Kind, stride: usize, index: usize) -> Option<usize> {
        let node = self.nodes.get(self.resolve(id) as usize)?;
        (node.kind == kind && index < node.b as usize).then(|| node.a as usize + index * stride)
    }

    /// Repoints `id`'s array element `index` at `new_value`, returning the id it replaced. `None` if not an
    /// `Array` or out of bounds.
    pub fn set_array_value(&mut self, id: ValueId, index: usize, new_value: ValueId) -> Option<ValueId> {
        let slot = self.child_slot(id, Kind::Array, 1, index)?;
        if !self.contains(new_value) {
            return None;
        }
        Some(core::mem::replace(&mut self.children[slot], new_value))
    }

    /// Pushes `text` as a fresh UTF-8 string and overwrites array element
    /// `index` of `id` to reference it. See [`Arena::set_array_value`].
    pub fn set_array_string(&mut self, id: ValueId, index: usize, text: alloc::string::String) -> Option<ValueId> {
        let new_value = self.push_string(text);
        self.set_array_value(id, index, new_value)
    }

    /// Pushes `bytes` as fresh untagged bytes and overwrites array element
    /// `index` of `id` to reference it. See [`Arena::set_array_value`].
    pub fn set_array_bytes(&mut self, id: ValueId, index: usize, bytes: Vec<u8>) -> Option<ValueId> {
        let new_value = self.push_bytes(bytes);
        self.set_array_value(id, index, new_value)
    }

    /// Pushes `bytes` as a fresh symbol and overwrites array element `index`
    /// of `id` to reference it. See [`Arena::set_array_value`].
    pub fn set_array_symbol(&mut self, id: ValueId, index: usize, bytes: Vec<u8>) -> Option<ValueId> {
        let new_value = self.push_symbol(bytes);
        self.set_array_value(id, index, new_value)
    }

    /// Pushes `value` as a fresh fixnum and overwrites array element `index`
    /// of `id` to reference it. See [`Arena::set_array_value`].
    pub fn set_array_fixnum(&mut self, id: ValueId, index: usize, value: i32) -> Option<ValueId> {
        let new_value = self.push_fixnum(value);
        self.set_array_value(id, index, new_value)
    }

    /// Pushes `value` as a fresh bool and overwrites array element `index`
    /// of `id` to reference it. See [`Arena::set_array_value`].
    pub fn set_array_bool(&mut self, id: ValueId, index: usize, value: bool) -> Option<ValueId> {
        let new_value = self.push_bool(value);
        self.set_array_value(id, index, new_value)
    }

    /// Repoints `id`'s hash entry `index` key at `new_key`, returning the id it replaced. `None` if not a `Hash`
    /// or out of bounds.
    pub fn set_hash_key(&mut self, id: ValueId, index: usize, new_key: ValueId) -> Option<ValueId> {
        let slot = self.child_slot(id, Kind::Hash, 2, index)?;
        if !self.contains(new_key) {
            return None;
        }
        Some(core::mem::replace(&mut self.children[slot], new_key))
    }

    /// Repoints `id`'s hash entry `index` value at `new_value`, returning the id it replaced. `None` if not a
    /// `Hash` or out of bounds.
    pub fn set_hash_value(&mut self, id: ValueId, index: usize, new_value: ValueId) -> Option<ValueId> {
        let slot = self.child_slot(id, Kind::Hash, 2, index)? + 1;
        if !self.contains(new_value) {
            return None;
        }
        Some(core::mem::replace(&mut self.children[slot], new_value))
    }

    /// Repoints `id`'s default value, returning the id it replaced. `None` if there is no default to replace;
    /// never adds one.
    pub fn set_hash_default(&mut self, id: ValueId, new_default: ValueId) -> Option<ValueId> {
        let node = *self.nodes.get(self.resolve(id) as usize)?;
        if node.kind != Kind::Hash || !node.flags.contains(Flags::HAS_DEFAULT) {
            return None;
        }
        if !self.contains(new_default) {
            return None;
        }
        let idx = node.a as usize + node.b as usize * 2;
        Some(core::mem::replace(&mut self.children[idx], new_default))
    }

    /// Index into `members` of `id`'s `Object`/`Struct` member `name`; the `@` is optional, as in `ValueRef::get`.
    fn member_index(&self, id: ValueId, name: &[u8]) -> Option<usize> {
        let node = *self.nodes.get(self.resolve(id) as usize)?;
        if !matches!(node.kind, Kind::Object | Kind::Struct) {
            return None;
        }
        (0..node.b).map(|i| (node.a + i) as usize).find(|&idx| {
            let sym = self.symbol_bytes(self.members[idx].0);
            sym == name || sym.strip_prefix(b"@") == Some(name)
        })
    }

    /// Repoints `id`'s member `name` at `new_value`, returning the id it replaced. Never adds a member.
    pub fn set_member_value(&mut self, id: ValueId, name: &[u8], new_value: ValueId) -> Option<ValueId> {
        let idx = self.member_index(id, name)?;
        if !self.contains(new_value) {
            return None;
        }
        Some(core::mem::replace(&mut self.members[idx].1, new_value))
    }

    /// Pushes `text` as a fresh UTF-8 string and overwrites `id`'s `name`
    /// member to reference it. See [`Arena::set_member_value`].
    pub fn set_member_string(&mut self, id: ValueId, name: &[u8], text: alloc::string::String) -> Option<ValueId> {
        let new_value = self.push_string(text);
        self.set_member_value(id, name, new_value)
    }

    /// Pushes `bytes` as fresh untagged bytes and overwrites `id`'s `name`
    /// member to reference it. See [`Arena::set_member_value`].
    pub fn set_member_bytes(&mut self, id: ValueId, name: &[u8], bytes: Vec<u8>) -> Option<ValueId> {
        let new_value = self.push_bytes(bytes);
        self.set_member_value(id, name, new_value)
    }

    /// Pushes `bytes` as a fresh symbol and overwrites `id`'s `name` member
    /// to reference it. See [`Arena::set_member_value`].
    pub fn set_member_symbol(&mut self, id: ValueId, name: &[u8], bytes: Vec<u8>) -> Option<ValueId> {
        let new_value = self.push_symbol(bytes);
        self.set_member_value(id, name, new_value)
    }

    /// Pushes `value` as a fresh fixnum and overwrites `id`'s `name` member
    /// to reference it. See [`Arena::set_member_value`].
    pub fn set_member_fixnum(&mut self, id: ValueId, name: &[u8], value: i32) -> Option<ValueId> {
        let new_value = self.push_fixnum(value);
        self.set_member_value(id, name, new_value)
    }

    /// Pushes `value` as a fresh bool and overwrites `id`'s `name` member to
    /// reference it. See [`Arena::set_member_value`].
    pub fn set_member_bool(&mut self, id: ValueId, name: &[u8], value: bool) -> Option<ValueId> {
        let new_value = self.push_bool(value);
        self.set_member_value(id, name, new_value)
    }

    /// Overwrites `id`'s bytes in place, so every reference to it sees the new content. `None` unless `id` is `Kind::Bytes`.
    pub fn set_bytes_content(&mut self, id: ValueId, bytes: Vec<u8>) -> Option<u32> {
        self.replace_blob(id, Kind::Bytes, bytes)
    }

    /// Overwrites `id`'s text in place and tags it UTF-8. Use [`Arena::set_str_content`] to keep the tag.
    /// `None` unless `id` is `Kind::Str` or `Kind::Bytes`.
    pub fn set_string_text(&mut self, id: ValueId, text: alloc::string::String) -> Option<u32> {
        self.set_str_with_encoding(id, text.into_bytes(), crate::encoding::ENCODING_UTF_8)
    }

    /// Overwrites `id`'s bytes in place, keeping its encoding tag. `None` unless `id` is `Kind::Str`.
    pub fn set_str_content(&mut self, id: ValueId, bytes: Vec<u8>) -> Option<u32> {
        self.replace_blob(id, Kind::Str, bytes)
    }

    /// Overwrites `id`'s bytes and encoding tag in place. Bytes become a string unless the tag is ASCII-8BIT, and
    /// vice versa. `None` unless `id` is a string or bytes, and `encoding_id` is in the fixed table.
    pub fn set_str_with_encoding(&mut self, id: ValueId, bytes: Vec<u8>, encoding_id: u8) -> Option<u32> {
        let id = self.resolve(id);
        if !matches!(self.nodes.get(id as usize)?.kind, Kind::Str | Kind::Bytes) {
            return None;
        }
        crate::encoding::encoding_name(encoding_id)?;
        let idx = self.push_blob(Cow::Owned(bytes));
        let previous = core::mem::replace(&mut self.nodes[id as usize].a, idx);
        self.set_encoding(id, encoding_id);
        Some(previous)
    }

    /// Re-tags a string, regexp or symbol without touching its bytes (like Ruby's `force_encoding`; never checked).
    /// `false` for other kinds or an id outside the fixed table; see [`Arena::set_encoding_name`] for names.
    pub fn set_encoding(&mut self, id: ValueId, encoding_id: u8) -> bool {
        crate::encoding::encoding_name(encoding_id).is_some() && self.retag(id, encoding_id, None)
    }

    /// [`Arena::set_encoding`] for an encoding known by its name (e.g. `b"Shift_JIS"`); a name outside the fixed
    /// table is kept as it is.
    pub fn set_encoding_name(&mut self, id: ValueId, encoding_name: &[u8]) -> bool {
        let (encoding_id, custom) = Self::resolve_encoding_name(encoding_name);
        self.retag(id, encoding_id, custom)
    }

    fn retag(&mut self, id: ValueId, encoding_id: u8, custom: Option<&[u8]>) -> bool {
        let id = self.resolve(id);
        let Some(&node) = self.nodes.get(id as usize) else {
            return false;
        };
        let mut updated = node;
        match node.kind {
            Kind::Str | Kind::Bytes => {
                if encoding_id == crate::encoding::ENCODING_ASCII_8BIT {
                    updated.kind = Kind::Bytes;
                    updated.b = 0;
                } else {
                    updated.kind = Kind::Str;
                    updated.b = u32::from(encoding_id);
                }
            }
            Kind::Regexp => updated.b = (node.b & 0xFF) | (u32::from(encoding_id) << 8),
            Kind::Symbol => {
                let bytes = Cow::Owned(self.symbol_bytes(node.a).to_vec());
                updated.a = self.intern_symbol_tagged(bytes, encoding_id, custom);
                self.nodes[id as usize] = updated;
                return true;
            }
            _ => return false,
        }
        self.nodes[id as usize] = updated;
        if let Ok(at) = self.custom_encodings.binary_search_by_key(&id, |&(target, _)| target) {
            self.custom_encodings.remove(at);
        }
        if let Some(name) = custom {
            let blob = self.push_blob(Cow::Owned(name.to_vec()));
            let at = self.custom_encodings.partition_point(|&(target, _)| target < id);
            self.custom_encodings.insert(at, (id, blob));
        }
        true
    }

    fn replace_blob(&mut self, id: ValueId, kind: Kind, bytes: Vec<u8>) -> Option<u32> {
        let id = self.resolve(id);
        if self.nodes.get(id as usize)?.kind != kind {
            return None;
        }
        let idx = self.push_blob(Cow::Owned(bytes));
        Some(core::mem::replace(&mut self.nodes[id as usize].a, idx))
    }

    /// Overwrites `id`'s fixnum in place, returning the old value. `None` unless `id` is `Kind::Fixnum`.
    #[allow(clippy::cast_possible_wrap)]
    pub fn set_fixnum_value(&mut self, id: ValueId, value: i32) -> Option<i32> {
        let id = self.resolve(id);
        let node = self.nodes.get_mut(id as usize)?;
        (node.kind == Kind::Fixnum).then(|| core::mem::replace(&mut node.a, value as u32) as i32)
    }

    /// Moves out the blob of `id` if its kind is in `kinds`, leaving `empty` in its place. No copy for owned arenas.
    fn take_blob(&mut self, id: ValueId, kinds: &[Kind], empty: &'static [u8]) -> Option<Vec<u8>> {
        let node = self.nodes.get(self.resolve(id) as usize)?;
        let idx = kinds.contains(&node.kind).then_some(node.a)?;
        Some(core::mem::replace(&mut self.blobs[idx as usize], Cow::Borrowed(empty)).into_owned())
    }

    /// A class or module name, taken: the node becomes `nil`, since neither can be dumped without a name. Its
    /// extensions and instance variables go with it, as `nil` cannot carry them.
    fn take_named(&mut self, id: ValueId, kind: Kind) -> Option<Vec<u8>> {
        let id = self.resolve(id);
        let path = self.take_blob(id, &[kind], &[])?;
        self.nodes[id as usize] = Node::scalar(Kind::Nil);
        self.extensions.retain(|e| e.0 != id);
        self.extra_ivars.retain(|e| e.0 != id);
        Some(path)
    }

    /// Takes ownership of `id`'s own byte payload, leaving an empty blob in
    /// its place. `None` unless `id` is `Kind::Str` or `Kind::Bytes`.
    pub fn take_bytes_content(&mut self, id: ValueId) -> Option<Vec<u8>> {
        self.take_blob(id, &[Kind::Str, Kind::Bytes], &[])
    }

    /// Takes ownership of `id`'s bignum magnitude (little-endian, sign in
    /// [`Flags::NEGATIVE`]). See [`Arena::take_bytes_content`]. `None` unless `id` is `Kind::Bignum`.
    pub fn take_bignum_magnitude(&mut self, id: ValueId) -> Option<Vec<u8>> {
        let magnitude = self.take_blob(id, &[Kind::Bignum], &[])?;
        // The node now holds zero, which is never negative. Left set, dump writes `l-` with no words, and that
        // loads as `l+`, so the dump would change on reload.
        let idx = self.resolve(id) as usize;
        self.nodes[idx].flags = self.nodes[idx].flags.without(Flags::NEGATIVE);
        Some(magnitude)
    }

    /// Takes ownership of `id`'s textual float form (e.g. `b"1.5"`). See
    /// [`Arena::take_bytes_content`]. `None` unless `id` is `Kind::Float`.
    ///
    /// Afterwards the float's text is `0`, so the value is `0.0`.
    pub fn take_float_ascii(&mut self, id: ValueId) -> Option<Vec<u8>> {
        self.take_blob(id, &[Kind::Float], b"0")
    }

    /// Takes ownership of `id`'s regexp source. See [`Arena::take_bytes_content`]. `None` unless `id`
    /// is `Kind::Regexp`.
    ///
    /// Afterwards the source is empty, so the regexp is `//`.
    pub fn take_regexp_source(&mut self, id: ValueId) -> Option<Vec<u8>> {
        self.take_blob(id, &[Kind::Regexp], &[])
    }

    /// Takes ownership of `id`'s class path. See [`Arena::take_bytes_content`]. `None` unless `id` is
    /// `Kind::Class`.
    ///
    /// Afterwards the value is `nil`, with no extensions or instance variables.
    pub fn take_class_path(&mut self, id: ValueId) -> Option<Vec<u8>> {
        self.take_named(id, Kind::Class)
    }

    /// Takes ownership of `id`'s module path. See [`Arena::take_bytes_content`]. `None` unless `id` is
    /// `Kind::Module`.
    ///
    /// Afterwards the value is `nil`, with no extensions or instance variables.
    pub fn take_module_path(&mut self, id: ValueId) -> Option<Vec<u8>> {
        self.take_named(id, Kind::Module)
    }
}
