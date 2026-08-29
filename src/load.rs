//! Builds an [`Arena`] from a Marshal byte stream.
//!
//! The tree is built *iteratively*, not via recursive descent, so arbitrarily
//! deep or wide input is a normal allocation, never a stack overflow. Object
//! links (`@n`) resolve to an existing [`ValueId`] via `Arena::resolve_link`.

use crate::{
    arena::{Arena, Flags, Kind, SymId, ValueId},
    reader::{ReadError, Reader, Span, Token},
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
    Ok(arena)
}

/// One pending "resume point" in the iterative builder - see the module
/// doc comment for the overall shape of the algorithm.
#[derive(Clone, Copy)]
enum Frame {
    /// A `TYPE_UCLASS`-wrapped value: apply the class name to whatever
    /// resolves next, then bubble it up unchanged.
    DecorateUClass { class: SymId },
    /// A `TYPE_EXTENDED`-wrapped value: record the extension, then bubble
    /// the same value up.
    DecorateExtended { module: SymId },
    /// A `TYPE_DATA`/`TYPE_USRMARSHAL` placeholder (`id`, already
    /// link-registered) awaiting its single wrapped child; once resolved,
    /// the child's entire node content is copied onto `id` and the class +
    /// provenance flag are applied.
    DecorateWrap { id: ValueId, flag: Flags, class: SymId },
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
    let mut stack: Vec<Frame> = Vec::new();
    let mut symbol_values: Vec<ValueId> = Vec::new();
    let mut resolved = read_value(reader, arena, &mut stack, &mut symbol_values)?;

    loop {
        let Some(frame) = stack.pop() else {
            return Ok(resolved);
        };
        match apply_frame(frame, resolved, reader, arena, &mut stack)? {
            Bubble::Done(id) => resolved = id,
            Bubble::More => {
                resolved = read_value(reader, arena, &mut stack, &mut symbol_values)?;
            }
        }
    }
}

/// Reads a symbol expected to name an `Object`/`Struct` member or an ivar.
fn read_member_name<'a>(reader: &mut Reader<'a, '_>, arena: &mut Arena<'a>) -> Result<SymId, ReadError> {
    match reader.next()? {
        Token::Symbol(bytes) => Ok(arena.intern_symbol(Cow::Borrowed(bytes))),
        _ => Err(ReadError::Unsupported("expected a symbol member/ivar name")),
    }
}

/// Reads tokens, pushing a [`Frame`] for each container/decorator opener
/// encountered, until an actual leaf value (or an already-empty container)
/// resolves.
#[allow(clippy::too_many_lines, clippy::cast_sign_loss, clippy::cast_possible_truncation)]
fn read_value<'a>(
    reader: &mut Reader<'a, '_>,
    arena: &mut Arena<'a>,
    stack: &mut Vec<Frame>,
    symbol_values: &mut Vec<ValueId>,
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
                let sym = arena.intern_symbol(Cow::Borrowed(bytes)) as usize;
                if sym >= symbol_values.len() {
                    symbol_values.resize(sym + 1, ValueId::MAX);
                }
                if symbol_values[sym] != ValueId::MAX {
                    return Ok(symbol_values[sym]);
                }
                let mut node = scalar(Kind::Symbol);
                node.a = sym as u32;
                let id = arena.push_node(node);
                symbol_values[sym] = id;
                return Ok(id);
            }

            Token::Bignum { negative, magnitude_le } => {
                let blob = arena.push_blob(Cow::Borrowed(magnitude_le));
                let mut node = scalar(Kind::Bignum);
                node.a = blob;
                if negative {
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
                let sym = arena.intern_symbol(Cow::Borrowed(class));
                let blob = arena.push_blob(Cow::Borrowed(data));
                let mut node = scalar(Kind::Bytes);
                node.class = sym;
                node.a = blob;
                node.flags = node.flags.with(Flags::USER_DEFINED);
                let id = arena.push_node(node);
                arena.push_link_entry(id);
                return Ok(id);
            }

            Token::BeginArray(len) => {
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
                let sym = arena.intern_symbol(Cow::Borrowed(class));
                let start = arena.reserve_members(len as usize);
                let mut node = scalar(Kind::Struct);
                node.class = sym;
                node.a = start;
                node.b = len;
                let id = arena.push_node(node);
                arena.push_link_entry(id);
                if len == 0 {
                    return Ok(id);
                }
                let name = read_member_name(reader, arena)?;
                stack.push(Frame::MemberValue {
                    id,
                    slot: 0,
                    remaining: len,
                    name,
                });
            }

            Token::BeginObject { class, len } => {
                let sym = arena.intern_symbol(Cow::Borrowed(class));
                let start = arena.reserve_members(len as usize);
                let mut node = scalar(Kind::Object);
                node.class = sym;
                node.a = start;
                node.b = len;
                let id = arena.push_node(node);
                arena.push_link_entry(id);
                if len == 0 {
                    return Ok(id);
                }
                let name = read_member_name(reader, arena)?;
                stack.push(Frame::MemberValue {
                    id,
                    slot: 0,
                    remaining: len,
                    name,
                });
            }

            Token::BeginUserMarshal { class } | Token::BeginData { class } => {
                let sym = arena.intern_symbol(Cow::Borrowed(class));
                let flag = if matches!(token, Token::BeginUserMarshal { .. }) {
                    Flags::USER_MARSHAL
                } else {
                    Flags::DATA
                };
                let id = arena.push_node(scalar(Kind::Nil));
                arena.push_link_entry(id);
                stack.push(Frame::DecorateWrap { id, flag, class: sym });
            }

            Token::BeginUClass { class } => {
                let sym = arena.intern_symbol(Cow::Borrowed(class));
                stack.push(Frame::DecorateUClass { class: sym });
            }

            Token::BeginExtended { module } => {
                let sym = arena.intern_symbol(Cow::Borrowed(module));
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
) -> Result<Bubble, ReadError> {
    Ok(match frame {
        Frame::DecorateUClass { class } => {
            let mut node = *arena.node(resolved);
            node.class = class;
            node.flags = node.flags.with(Flags::USER_CLASS);
            arena.set_node(resolved, node);
            Bubble::Done(resolved)
        }

        Frame::DecorateExtended { module } => {
            arena.add_extension(resolved, module);
            let mut node = *arena.node(resolved);
            node.flags = node.flags.with(Flags::EXTENDED);
            arena.set_node(resolved, node);
            Bubble::Done(resolved)
        }

        Frame::DecorateWrap { id, flag, class } => {
            let mut node = *arena.node(resolved);
            node.flags = node.flags.with(flag);
            node.class = class;
            arena.set_node(id, node);
            Bubble::Done(id)
        }

        Frame::IvarWrap => {
            let count = reader.next_ivar_count()?;
            if count == 0 {
                Bubble::Done(resolved)
            } else {
                let name = read_member_name(reader, arena)?;
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
                Bubble::Done(target)
            } else {
                let name = read_member_name(reader, arena)?;
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
                    stack.push(Frame::HashDefault { id });
                    Bubble::More
                } else {
                    Bubble::Done(id)
                }
            } else {
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
                let next_name = read_member_name(reader, arena)?;
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

/// Applies one already-read ivar (name, value) pair to `target`: only a
/// `Bytes`- or `Regexp`-kind target is affected (`Bytes` also covers
/// `Flags::USER_DEFINED` values - see that flag's doc comment) and only the
/// two encoding-related names have any effect; every other ivar is read (to
/// keep the stream cursor correct) and otherwise discarded. The declared
/// encoding is recorded as an id (see [`mod@crate::encoding`]) - the bytes
/// themselves are never touched, matching Ruby's own semantics: `E`/
/// `encoding` is a tag, not a promise the content actually validates.
fn apply_ivar_pair(arena: &mut Arena<'_>, target: ValueId, name: SymId, value: ValueId) {
    let node = *arena.node(target);
    if !matches!(node.kind, Kind::Bytes | Kind::Regexp) {
        return;
    }
    match arena.symbol_bytes(name) {
        // `:E => true` means UTF-8, `:E => false` means US-ASCII.
        b"E" => {
            let id = if arena.node(value).kind == Kind::True {
                crate::encoding::ENCODING_UTF_8
            } else {
                crate::encoding::ENCODING_US_ASCII
            };
            tag_encoding(arena, target, node, id);
        }
        b"encoding" => {
            let value_node = *arena.node(value);
            if matches!(value_node.kind, Kind::Bytes | Kind::Str) {
                let encoding_name = arena.blob(value_node.a).to_vec();
                if let Some(id) = crate::encoding::encoding_id(&encoding_name) {
                    tag_encoding(arena, target, node, id)
                } else {
                    let blob = arena.push_blob(Cow::Owned(encoding_name));
                    tag_encoding(arena, target, node, crate::encoding::ENCODING_CUSTOM);
                    arena.add_custom_encoding(target, blob);
                }
            }
        }
        _ => {}
    }
}

/// Stamps `id` as `target`'s declared encoding: for a `Bytes` value this
/// also promotes it to `Kind::Str` (Ruby's own "string with a declared
/// encoding" vs. plain binary distinction); for `Regexp`, only the encoding
/// byte (packed above the option bits) changes.
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
