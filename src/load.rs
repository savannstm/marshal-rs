//! Builds an [`Arena`] from a Marshal byte stream.
//!
//! Builds the tree iteratively with an explicit frame stack, so deep input cannot overflow the call stack.
//! Object links (`@n`) resolve through `Arena::resolve_link`.

use crate::{
    arena::{Arena, Flags, Kind, SymId, ValueId},
    reader::{ReadError, Reader, Span, SymbolEncoding, Token},
};
use alloc::{borrow::Cow, vec::Vec};

/// Loads a Marshal byte stream.
///
/// # Errors
///
/// Returns [`ReadError`] if `bytes` isn't a well-formed Marshal 4.8 stream.
pub fn load(bytes: &[u8]) -> Result<Arena<'_>, ReadError> {
    let mut arena = Arena::new();
    let mut symbol_spans: Vec<Span> = Vec::new();
    let mut reader = Reader::new(bytes, &mut symbol_spans)?;
    let root = build(&mut reader, &mut arena)?;
    arena.root = root;
    arena.finish_load();
    if reader.remaining() != 0 {
        return Err(ReadError::TrailingData {
            offset: reader.position(),
        });
    }
    Ok(arena)
}

/// Loads every Marshal stream concatenated in `bytes`, such as an RPG Maker XP/VX/VX Ace save file
/// that is a series of `Marshal.dump` calls into one file.
///
/// # Errors
///
/// Returns [`ReadError`] if any of the streams isn't a well-formed Marshal 4.8 stream.
pub fn load_many(bytes: &[u8]) -> Result<Vec<Arena<'_>>, ReadError> {
    let mut arenas = Vec::new();
    let mut offset = 0;

    while offset < bytes.len() {
        let mut arena = Arena::new();
        let mut symbol_spans: Vec<Span> = Vec::new();
        let mut reader = Reader::new(&bytes[offset..], &mut symbol_spans)?;
        let root = build(&mut reader, &mut arena)?;
        arena.root = root;
        arena.finish_load();
        offset += reader.position();
        arenas.push(arena);
    }

    Ok(arenas)
}

/// A pending resume point in the iterative builder; see the module doc for the algorithm.
#[derive(Clone, Copy)]
enum Frame {
    /// A `TYPE_UCLASS`-wrapped value: apply the class name to whatever
    /// resolves next, then bubble it up unchanged.
    DecorateUClass { class: SymId },
    /// A `TYPE_EXTENDED`-wrapped value: record the extension, then bubble
    /// the same value up.
    DecorateExtended { module: SymId },
    /// A `TYPE_DATA`/`TYPE_USRMARSHAL` wrapper (`id`, already link-registered,
    /// carrying the wrapper's class and flag) awaiting its single body; once
    /// resolved, that node becomes the wrapper's body.
    DecorateWrap { id: ValueId },
    /// A `TYPE_IVAR`-wrapped value has fully resolved; read its ivar count
    /// next.
    IvarWrap,
    /// Awaiting one ivar pair's value; `name` is that pair's already-read
    /// key.
    IvarPairValue {
        target: ValueId,
        remaining: u32,
        name: SymId,
    },
    /// Awaiting the next `Array` element.
    ArrayElem { id: ValueId, slot: u32, remaining: u32 },
    /// Awaiting the next `Hash` pair's key.
    HashKey {
        id: ValueId,
        slot: u32,
        remaining: u32,
        has_default: bool,
    },
    /// Awaiting the value half of the pair whose key just resolved.
    HashValue {
        id: ValueId,
        slot: u32,
        remaining: u32,
        has_default: bool,
        key: ValueId,
    },
    /// Awaiting a `Hash`'s trailing default value.
    HashDefault { id: ValueId },
    /// Awaiting the value half of an `Object`/`Struct` member; `name` is
    /// that member's already-read key.
    MemberValue {
        id: ValueId,
        slot: u32,
        remaining: u32,
        name: SymId,
    },
}

enum Bubble {
    /// The frame needs another value read from the stream before it can
    /// resolve further.
    More,
    /// The frame is fully resolved; keep bubbling `ValueId` up the stack.
    Done(ValueId),
}

fn build<'a>(reader: &mut Reader<'a, '_>, arena: &mut Arena<'a>) -> Result<ValueId, ReadError> {
    let mut scratch = Scratch::default();
    let mut stack: Vec<Frame> = Vec::new();
    let mut resolved = read_value(reader, arena, &mut stack, &mut scratch)?;

    loop {
        let Some(frame) = stack.pop() else {
            return Ok(resolved);
        };
        match apply_frame(frame, resolved, reader, arena, &mut stack, &mut scratch)? {
            Bubble::Done(id) => resolved = id,
            Bubble::More => {
                resolved = read_value(reader, arena, &mut stack, &mut scratch)?;
            }
        }
    }
}

/// Reads a symbol expected to name an `Object`/`Struct` member or an ivar.
fn read_member_name<'a>(
    reader: &mut Reader<'a, '_>,
    arena: &mut Arena<'a>,
    scratch: &mut Scratch,
) -> Result<SymId, ReadError> {
    let bytes = reader.next_symbol()?;
    let sym = scratch.symbol(reader, arena, bytes)?;
    scratch.flush(arena);
    Ok(sym)
}

/// Reads tokens, pushing a [`Frame`] for each container/decorator opener
/// encountered, until an actual leaf value (or an already-empty container)
/// resolves.
#[allow(clippy::too_many_lines, clippy::cast_sign_loss, clippy::cast_possible_truncation)]
fn read_value<'a>(
    reader: &mut Reader<'a, '_>,
    arena: &mut Arena<'a>,
    stack: &mut Vec<Frame>,
    scratch: &mut Scratch,
) -> Result<ValueId, ReadError> {
    loop {
        let token = reader.next()?;
        match token {
            Token::Nil => return Ok(arena.push_node(scalar(Kind::Nil))),
            Token::True => return Ok(arena.push_node(scalar(Kind::True))),
            Token::False => return Ok(arena.push_node(scalar(Kind::False))),

            Token::Fixnum(value) => {
                let mut node = scalar(Kind::Fixnum);
                node.a = value as u32;
                return Ok(arena.push_node(node));
            }

            Token::Link(idx) => {
                return arena.resolve_link(idx).ok_or(ReadError::UnknownObjectLink { idx });
            }

            Token::Symbol(bytes) => {
                let sym = scratch.symbol(reader, arena, bytes)? as usize;
                scratch.flush(arena);
                if sym >= scratch.symbol_values.len() {
                    scratch.symbol_values.resize(sym + 1, ValueId::MAX);
                }
                if scratch.symbol_values[sym] != ValueId::MAX {
                    return Ok(scratch.symbol_values[sym]);
                }
                let mut node = scalar(Kind::Symbol);
                node.a = sym as u32;
                let id = arena.push_node(node);
                scratch.symbol_values[sym] = id;
                return Ok(id);
            }

            Token::Bignum { negative, magnitude_le } => {
                let magnitude = crate::bignum::normalize_magnitude(magnitude_le);
                let blob = arena.push_blob(Cow::Borrowed(magnitude));
                let mut node = scalar(Kind::Bignum);
                node.a = blob;
                // Zero is never negative.
                if negative && !magnitude.is_empty() {
                    node.flags = node.flags.with(Flags::NEGATIVE);
                }
                let id = arena.push_node(node);
                arena.push_link_entry(id);
                return Ok(id);
            }

            Token::Float(ascii) => {
                let blob = arena.push_blob(Cow::Borrowed(ascii));
                let mut node = scalar(Kind::Float);
                node.a = blob;
                let id = arena.push_node(node);
                arena.push_link_entry(id);
                return Ok(id);
            }

            Token::Str(bytes) => {
                let blob = arena.push_blob(Cow::Borrowed(bytes));
                let mut node = scalar(Kind::Bytes);
                node.a = blob;
                let id = arena.push_node(node);
                arena.push_link_entry(id);
                return Ok(id);
            }

            Token::Regexp { source, options } => {
                let blob = arena.push_blob(Cow::Borrowed(source));
                let mut node = scalar(Kind::Regexp);
                node.a = blob;
                node.b = u32::from(options);
                let id = arena.push_node(node);
                arena.push_link_entry(id);
                return Ok(id);
            }

            Token::Class(bytes) => {
                let blob = arena.push_blob(Cow::Borrowed(bytes));
                let mut node = scalar(Kind::Class);
                node.a = blob;
                let id = arena.push_node(node);
                arena.push_link_entry(id);
                return Ok(id);
            }

            Token::Module(bytes) | Token::OldModule(bytes) => {
                let old = matches!(token, Token::OldModule(_));
                let blob = arena.push_blob(Cow::Borrowed(bytes));
                let mut node = scalar(Kind::Module);
                node.a = blob;
                if old {
                    node.flags = node.flags.with(Flags::OLD_MODULE);
                }
                let id = arena.push_node(node);
                arena.push_link_entry(id);
                return Ok(id);
            }

            Token::UserDefined { class, data } => {
                let sym = scratch.symbol(reader, arena, class)?;
                let blob = arena.push_blob(Cow::Borrowed(data));
                let mut node = scalar(Kind::Bytes);
                node.class = sym;
                node.a = blob;
                node.flags = node.flags.with(Flags::USER_DEFINED);
                let id = arena.push_node(node);
                // Ruby numbers a `_dump` value after its class name (an encoding name there is an object too) and,
                // when it is wrapped in `TYPE_IVAR`, after its instance variables: see `Frame::IvarWrap`.
                scratch.flush(arena);
                // Extended modules sit between the wrap and the value, and don't change when the value is numbered.
                let wrapped = stack
                    .iter()
                    .rev()
                    .find(|frame| !matches!(frame, Frame::DecorateExtended { .. }))
                    .is_some_and(|frame| matches!(frame, Frame::IvarWrap));
                if wrapped {
                    scratch.unnumbered.push(id);
                } else {
                    arena.push_link_entry(id);
                }
                return Ok(id);
            }

            Token::BeginArray(len) => {
                owe(&mut scratch.owed, (len as usize).saturating_sub(1), reader)?;
                let start = arena.reserve_children(len as usize);
                let mut node = scalar(Kind::Array);
                node.a = start;
                node.b = len;
                let id = arena.push_node(node);
                arena.push_link_entry(id);
                if len == 0 {
                    return Ok(id);
                }
                stack.push(Frame::ArrayElem {
                    id,
                    slot: 0,
                    remaining: len,
                });
            }

            Token::BeginHash { len, has_default } => {
                let extra = usize::from(has_default);
                owe(&mut scratch.owed, (len as usize * 2 + extra).saturating_sub(1), reader)?;
                let start = arena.reserve_children(len as usize * 2 + extra);
                let mut node = scalar(Kind::Hash);
                node.a = start;
                node.b = len;
                if has_default {
                    node.flags = node.flags.with(Flags::HAS_DEFAULT);
                }
                let id = arena.push_node(node);
                arena.push_link_entry(id);
                if len > 0 {
                    stack.push(Frame::HashKey {
                        id,
                        slot: 0,
                        remaining: len,
                        has_default,
                    });
                } else if has_default {
                    stack.push(Frame::HashDefault { id });
                } else {
                    return Ok(id);
                }
            }

            Token::BeginStruct { class, len } => {
                let sym = scratch.symbol(reader, arena, class)?;
                owe(&mut scratch.owed, (len as usize).saturating_sub(1), reader)?;
                let start = arena.reserve_members(len as usize);
                let mut node = scalar(Kind::Struct);
                node.class = sym;
                node.a = start;
                node.b = len;
                let id = arena.push_node(node);
                arena.push_link_entry(id);
                scratch.flush(arena);
                if len == 0 {
                    return Ok(id);
                }
                let name = read_member_name(reader, arena, scratch)?;
                stack.push(Frame::MemberValue {
                    id,
                    slot: 0,
                    remaining: len,
                    name,
                });
            }

            Token::BeginObject { class, len } => {
                let sym = scratch.symbol(reader, arena, class)?;
                owe(&mut scratch.owed, (len as usize).saturating_sub(1), reader)?;
                let start = arena.reserve_members(len as usize);
                let mut node = scalar(Kind::Object);
                node.class = sym;
                node.a = start;
                node.b = len;
                let id = arena.push_node(node);
                arena.push_link_entry(id);
                scratch.flush(arena);
                if len == 0 {
                    return Ok(id);
                }
                let name = read_member_name(reader, arena, scratch)?;
                stack.push(Frame::MemberValue {
                    id,
                    slot: 0,
                    remaining: len,
                    name,
                });
            }

            Token::BeginUserMarshal { class } | Token::BeginData { class } => {
                let sym = scratch.symbol(reader, arena, class)?;
                let flag = if matches!(token, Token::BeginUserMarshal { .. }) {
                    Flags::USER_MARSHAL
                } else {
                    Flags::DATA
                };
                let mut node = scalar(Kind::Nil);
                node.flags = flag;
                node.class = sym;
                let id = arena.push_node(node);
                // A `marshal_dump`/`_dump_data` wrapper is numbered after its class name, unlike an object.
                scratch.flush(arena);
                arena.push_link_entry(id);
                stack.push(Frame::DecorateWrap { id });
            }

            Token::BeginUClass { class } => {
                let sym = scratch.symbol(reader, arena, class)?;
                // The marker's class name comes before the object it marks, and so does its encoding name.
                scratch.flush(arena);
                stack.push(Frame::DecorateUClass { class: sym });
            }

            Token::BeginExtended { module } => {
                let sym = scratch.symbol(reader, arena, module)?;
                scratch.flush(arena);
                stack.push(Frame::DecorateExtended { module: sym });
            }

            Token::BeginIvar => {
                stack.push(Frame::IvarWrap);
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
fn apply_frame<'a>(
    frame: Frame,
    resolved: ValueId,
    reader: &mut Reader<'a, '_>,
    arena: &mut Arena<'a>,
    stack: &mut Vec<Frame>,
    scratch: &mut Scratch,
) -> Result<Bubble, ReadError> {
    Ok(match frame {
        Frame::DecorateUClass { class } => {
            let mut node = *arena.node(resolved);
            // Ruby only ever writes `C` for a string, regexp, array or hash (and never twice).
            let builtin = matches!(node.kind, Kind::Bytes | Kind::Regexp | Kind::Array | Kind::Hash);
            if !builtin
                || node
                    .flags
                    .intersects(Flags::USER_DEFINED.with(Flags::USER_CLASS).with(Flags::WRAPPER))
            {
                return Err(ReadError::Unsupported(
                    "subclass marker on a value that is not a plain built-in",
                ));
            }
            node.class = class;
            node.flags = node.flags.with(Flags::USER_CLASS);
            arena.set_node(resolved, node);
            Bubble::Done(resolved)
        }

        Frame::DecorateExtended { module } => {
            // An immediate (or the node every use of a symbol shares) cannot be extended.
            if !arena.can_carry_extras(resolved) {
                return Err(ReadError::Unsupported(
                    "extend marker on nil, a boolean, a fixnum or a symbol",
                ));
            }
            arena.add_extension(resolved, module);
            let mut node = *arena.node(resolved);
            node.flags = node.flags.with(Flags::EXTENDED);
            arena.set_node(resolved, node);
            Bubble::Done(resolved)
        }

        Frame::DecorateWrap { id } => {
            arena.wrappers.push((id, resolved));
            Bubble::Done(id)
        }

        Frame::IvarWrap => {
            let count = reader.next_ivar_count()?;
            if count == 0 {
                scratch.number_unnumbered(arena, resolved);
                Bubble::Done(resolved)
            } else {
                let name = read_member_name(reader, arena, scratch)?;
                stack.push(Frame::IvarPairValue {
                    target: resolved,
                    remaining: count,
                    name,
                });
                Bubble::More
            }
        }

        Frame::IvarPairValue {
            target,
            remaining,
            name,
        } => {
            apply_ivar_pair(arena, target, name, resolved);
            let remaining = remaining - 1;
            if remaining == 0 {
                scratch.number_unnumbered(arena, target);
                Bubble::Done(target)
            } else {
                let name = read_member_name(reader, arena, scratch)?;
                stack.push(Frame::IvarPairValue {
                    target,
                    remaining,
                    name,
                });
                Bubble::More
            }
        }

        Frame::ArrayElem { id, slot, remaining } => {
            let start = arena.node(id).a;
            arena.children[start as usize + slot as usize] = resolved;
            let remaining = remaining - 1;
            if remaining == 0 {
                Bubble::Done(id)
            } else {
                scratch.owed = scratch.owed.saturating_sub(1);
                stack.push(Frame::ArrayElem {
                    id,
                    slot: slot + 1,
                    remaining,
                });
                Bubble::More
            }
        }

        Frame::HashKey {
            id,
            slot,
            remaining,
            has_default,
        } => {
            scratch.owed = scratch.owed.saturating_sub(1);
            stack.push(Frame::HashValue {
                id,
                slot,
                remaining,
                has_default,
                key: resolved,
            });
            Bubble::More
        }

        Frame::HashValue {
            id,
            slot,
            remaining,
            has_default,
            key,
        } => {
            let start = arena.node(id).a;
            arena.children[start as usize + (slot * 2) as usize] = key;
            arena.children[start as usize + (slot * 2 + 1) as usize] = resolved;
            let remaining = remaining - 1;
            if remaining == 0 {
                if has_default {
                    scratch.owed = scratch.owed.saturating_sub(1);
                    stack.push(Frame::HashDefault { id });
                    Bubble::More
                } else {
                    Bubble::Done(id)
                }
            } else {
                scratch.owed = scratch.owed.saturating_sub(1);
                stack.push(Frame::HashKey {
                    id,
                    slot: slot + 1,
                    remaining,
                    has_default,
                });
                Bubble::More
            }
        }

        Frame::HashDefault { id } => {
            let node = *arena.node(id);
            let idx = node.a as usize + node.b as usize * 2;
            arena.children[idx] = resolved;
            Bubble::Done(id)
        }

        Frame::MemberValue {
            id,
            slot,
            remaining,
            name,
        } => {
            let start = arena.node(id).a;
            arena.members[start as usize + slot as usize] = (name, resolved);
            let remaining = remaining - 1;
            if remaining == 0 {
                Bubble::Done(id)
            } else {
                scratch.owed = scratch.owed.saturating_sub(1);
                let next_name = read_member_name(reader, arena, scratch)?;
                stack.push(Frame::MemberValue {
                    id,
                    slot: slot + 1,
                    remaining,
                    name: next_name,
                });
                Bubble::More
            }
        }
    })
}

/// Applies one already-read ivar to `target`. Only `E`/`encoding` on a bytes or regexp target have effect;
/// other ivars are read (to keep the cursor right) and discarded. The bytes are never checked.
fn apply_ivar_pair(arena: &mut Arena<'_>, target: ValueId, name: SymId, value: ValueId) {
    let node = *arena.node(target);
    // A symbol's own ivar is its encoding, which the symbol table does not keep; and the node of a
    // symbol, like those of `nil` and the booleans, is shared between every use of it.
    if !arena.can_carry_extras(target) {
        return;
    }
    let taggable = matches!(node.kind, Kind::Bytes | Kind::Regexp);
    match arena.symbol_bytes(name) {
        // `:E => true` means UTF-8, `:E => false` means US-ASCII.
        b"E" if taggable => {
            let id = if arena.node(value).kind == Kind::True {
                crate::encoding::ENCODING_UTF_8
            } else {
                crate::encoding::ENCODING_US_ASCII
            };
            tag_encoding(arena, target, node, id);
        }
        b"encoding" if taggable => {
            let value_node = *arena.node(value);
            if matches!(value_node.kind, Kind::Bytes | Kind::Str) {
                let encoding_name = arena.blob(value_node.a).to_vec();
                if let Some(id) = crate::encoding::encoding_id(&encoding_name) {
                    tag_encoding(arena, target, node, id);
                } else {
                    let blob = arena.push_blob(Cow::Owned(encoding_name));
                    tag_encoding(arena, target, node, crate::encoding::ENCODING_CUSTOM);
                    arena.add_custom_encoding(target, blob);
                }
            }
        }
        // The encoding of an already tagged string: nothing more to record.
        b"E" | b"encoding" if node.kind == Kind::Str => {}
        _ => arena.extra_ivars.push((target, name, value)),
    }
}

/// Stamps `id` as `target`'s declared encoding. For a `Bytes` value this also promotes it to `Kind::Str`;
/// for `Regexp` only the encoding byte (packed above the option bits) changes.
fn tag_encoding(arena: &mut Arena<'_>, target: ValueId, node: crate::arena::Node, id: u8) {
    let mut updated = node;
    match node.kind {
        Kind::Bytes => {
            updated.kind = Kind::Str;
            updated.b = u32::from(id);
        }
        Kind::Regexp => {
            updated.b = (node.b & 0xFF) | (u32::from(id) << 8);
        }
        _ => unreachable!("apply_ivar_pair only calls this for Bytes/Regexp targets"),
    }
    arena.set_node(target, updated);
}

#[inline]
const fn scalar(kind: Kind) -> crate::arena::Node {
    crate::arena::Node::scalar(kind)
}

/// Per-stream state of the builder.
#[derive(Default)]
struct Scratch {
    /// Elements the open containers still owe. Each takes at least one byte, so this is capped by the input size.
    owed: usize,
    /// The one `Symbol` value per [`SymId`], so a repeated symbol is a single node.
    symbol_values: Vec<ValueId>,
    /// The arena's [`SymId`] for each entry of the reader's symbol table, in table order (`UNSEEN` for those not
    /// met yet).
    symbol_ids: Vec<SymId>,
    /// The string node of an encoding name a symbol carried inline. Ruby numbers it right after the object the
    /// symbol belongs to, so it takes its slot in the object-link table only once that object has: see
    /// [`Scratch::flush`].
    pending_name: Option<ValueId>,
    /// `_dump` values read inside a `TYPE_IVAR` wrapper, which take their slot in the object-link table after
    /// their instance variables: see [`Scratch::number_unnumbered`].
    unnumbered: Vec<ValueId>,
}

impl Scratch {
    /// Marks an entry of `symbol_ids` whose symbol the loader has not met.
    const UNSEEN: SymId = SymId::MAX;

    /// The [`SymId`] of the symbol `reader` just resolved to `bytes`. A back-reference is found by its
    /// table index; only a symbol's first appearance is looked up by content, and it keeps the encoding it
    /// declared.
    fn symbol<'a>(
        &mut self,
        reader: &Reader<'a, '_>,
        arena: &mut Arena<'a>,
        bytes: &'a [u8],
    ) -> Result<SymId, ReadError> {
        let index = reader.last_symbol() as usize;
        if let Some(&id) = self.symbol_ids.get(index) {
            if id != Self::UNSEEN {
                return Ok(id);
            }
        }
        let (encoding, name) = match reader.last_symbol_encoding() {
            SymbolEncoding::Bare => (crate::encoding::ENCODING_ASCII_8BIT, None),
            SymbolEncoding::Utf8 => (crate::encoding::ENCODING_UTF_8, None),
            SymbolEncoding::UsAscii => (crate::encoding::ENCODING_US_ASCII, None),
            SymbolEncoding::Named(name) => {
                let mut node = scalar(Kind::Bytes);
                node.a = arena.push_blob(Cow::Borrowed(name));
                let name_node = arena.push_node(node);
                arena.name_nodes.push(name_node);
                self.pending_name = Some(name_node);
                (
                    crate::encoding::encoding_id(name).unwrap_or(crate::encoding::ENCODING_CUSTOM),
                    Some(name.to_vec()),
                )
            }
            SymbolEncoding::NamedLink(idx) => {
                let linked = arena.resolve_link(idx).ok_or(ReadError::UnknownObjectLink { idx })?;
                let node = *arena.node(linked);
                if !matches!(node.kind, Kind::Bytes | Kind::Str) {
                    return Err(ReadError::Unsupported("symbol encoding that is not a string"));
                }
                let name = arena.blob(node.a).to_vec();
                (
                    crate::encoding::encoding_id(&name).unwrap_or(crate::encoding::ENCODING_CUSTOM),
                    Some(name),
                )
            }
        };
        let custom = name.as_deref().filter(|_| encoding == crate::encoding::ENCODING_CUSTOM);
        let id = arena.intern_symbol_tagged(Cow::Borrowed(bytes), encoding, custom);
        // The reader numbers every symbol it reads, including the `E`/`encoding` inside a symbol's own wrapper,
        // which this never sees: those indexes stay unseen.
        if index >= self.symbol_ids.len() {
            self.symbol_ids.resize(index + 1, Self::UNSEEN);
        }
        self.symbol_ids[index] = id;
        Ok(id)
    }

    /// Gives `id` its slot in the object-link table if it is the `_dump` value waiting for one, now that its
    /// instance variables are read.
    fn number_unnumbered(&mut self, arena: &mut Arena<'_>, id: ValueId) {
        if self.unnumbered.last() == Some(&id) {
            self.unnumbered.pop();
            arena.push_link_entry(id);
        }
    }

    /// Gives the encoding name the last symbol carried inline its slot in the object-link table. Ruby numbers
    /// an `Object` or `Struct` before its class name's encoding (so this runs once the object has its slot), and
    /// every other value after it (so this runs before).
    fn flush(&mut self, arena: &mut Arena<'_>) {
        if let Some(id) = self.pending_name.take() {
            arena.push_link_entry(id);
        }
    }
}

/// Records that a new container owes `count` more elements; errors if the open containers together
/// owe more elements than there are bytes left.
fn owe(owed: &mut usize, count: usize, reader: &Reader<'_, '_>) -> Result<(), ReadError> {
    *owed = owed.saturating_add(count);
    if *owed > reader.remaining() {
        return Err(ReadError::LengthOverflow);
    }
    Ok(())
}
