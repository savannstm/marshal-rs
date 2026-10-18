//! Writes an [`Arena`] back to a Marshal byte stream.
//!
//! Traversal uses an explicit work stack: a hostile input can nest as deep as its length, so recursion would overflow.

use crate::{
    arena::{Arena, Flags, Kind, SymId, ValueId},
    writer::Writer,
};
use alloc::vec::Vec;

/// Serializes `arena` back to Marshal bytes. Infallible: the builder rejects ids that are not nodes of the arena.
#[must_use]
pub fn dump(arena: &Arena<'_>) -> Vec<u8> {
    let mut dumper = Dumper {
        writer: Writer::new(Vec::with_capacity(arena.nodes.len() * 8 + 64)),
        arena,
        sym_link: alloc::vec![u32::MAX; arena.symbols.len()],
        obj_link: alloc::vec![u32::MAX; arena.nodes.len()],
        next_sym: 0,
        next_obj: 0,
        fixed_symbol_links: [u32::MAX; 2],
        encoding_names: Vec::new(),
        work: Vec::new(),
    };
    let _: Result<(), core::convert::Infallible> = dumper.writer.write_header();
    dumper.write_value(arena.root());
    dumper.writer.into_inner()
}

struct Dumper<'r, 'a> {
    writer: Writer<Vec<u8>>,
    arena: &'r Arena<'a>,
    sym_link: Vec<u32>,
    obj_link: Vec<u32>,
    next_sym: u32,
    next_obj: u32,
    /// Table index of the `E` and `encoding` symbols once written; see [`Dumper::write_fixed_symbol`].
    fixed_symbol_links: [u32; 2],
    /// The encoding names written so far and the object index of their string, so a repeat is a link.
    encoding_names: Vec<(&'r [u8], u32)>,
    work: Vec<Work>,
}

/// One pending write. Items are popped in the order the wire format wants them, so a container
/// pushes its parts in reverse.
enum Work {
    Value(ValueId),
    /// An object/struct member name, which precedes that member's value.
    MemberName(SymId),
    /// The ivar pair that follows a string's or regexp's whole body.
    TrailingIvars {
        encoding_id: Option<u8>,
        target: ValueId,
    },
    /// Gives a `_dump` value its place in the object numbering, once its instance variables are written.
    Number(ValueId),
}

impl<'r> Dumper<'r, '_> {
    /// The `obj_link` of a value being written that has no number yet.
    const UNFINISHED: u32 = u32::MAX - 1;

    fn write_value(&mut self, root: ValueId) {
        self.work.push(Work::Value(root));
        while let Some(item) = self.work.pop() {
            match item {
                Work::Value(id) => self.write_node(id),
                Work::MemberName(sym) => self.write_symbol_ref(sym),
                Work::TrailingIvars { encoding_id, target } => {
                    self.write_trailing_ivars(encoding_id, target);
                }
                Work::Number(id) => self.number(id),
            }
        }
    }

    /// Gives `id` the next object number.
    fn number(&mut self, id: ValueId) {
        self.obj_link[id as usize] = self.next_obj;
        self.next_obj += 1;
    }

    fn write_symbol_ref(&mut self, sym: SymId) {
        let slot = &mut self.sym_link[sym as usize];
        if *slot != u32::MAX {
            let _ = self.writer.write_symbol_link(*slot);
            return;
        }
        let idx = self.next_sym;
        self.next_sym += 1;
        *slot = idx;
        let arena = self.arena;
        let bytes = arena.symbol_bytes(sym);
        // A symbol's encoding is written as a `TYPE_IVAR` wrapper, or bare when it has none.
        let encoding = arena.symbol_encoding(sym);
        let tagged = encoding != crate::encoding::ENCODING_ASCII_8BIT;
        if tagged {
            let _ = self.writer.write_ivar_wrap_tag();
        }
        let _ = self.writer.write_symbol_new(bytes);
        if tagged {
            let _ = self.writer.write_len(1);
            self.write_encoding_ivar(encoding, arena.symbol_custom_encoding(sym));
        }
    }

    // One exhaustive match is clearer than splitting it. The `as _` casts on `node.a`/`node.b` reinterpret
    // packed payload bits (fixnum bits, encoding and option bytes); they do not narrow ranges.
    #[allow(clippy::too_many_lines, clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    fn write_node(&mut self, id: ValueId) {
        let node = *self.arena.node(id);

        // A wrapper node is `Kind::Nil` with a flag; its real content is its body node.
        let wrapped = node.flags.intersects(Flags::WRAPPER);
        if !wrapped
            && matches!(
                node.kind,
                Kind::Nil | Kind::True | Kind::False | Kind::Fixnum | Kind::Symbol
            )
        {
            self.write_scalar(node.kind, node.a);
            return;
        }

        let slot = &mut self.obj_link[id as usize];
        if *slot == Self::UNFINISHED {
            // A `_dump` value inside its own instance variables: Ruby has no stream for that either (it numbers the
            // value only after them, so there is nothing to link to yet).
            let _ = self.writer.write_nil();
            return;
        }
        if *slot != u32::MAX {
            let _ = self.writer.write_object_link(*slot);
            return;
        }
        // Ruby numbers a value before its contents, except `_dump` values (after class name and ivars)
        // and `marshal_dump`/`_dump_data` wrappers (after class name).
        if wrapped || node.flags.contains(Flags::USER_DEFINED) {
            *slot = Self::UNFINISHED;
        }

        // `TYPE_IVAR` is the outermost wrapper, so its tag and trailing pairs are handled here. An
        // `ASCII-8BIT` string needs no ivar (Ruby only writes one for a non-default encoding).
        let encoding_id = match node.kind {
            Kind::Str => Some(node.b as u8),
            Kind::Regexp => Some((node.b >> 8) as u8),
            _ => None,
        }
        .filter(|&id| id != crate::encoding::ENCODING_ASCII_8BIT);
        let needs_ivar_wrap = encoding_id.is_some() || !self.arena.extra_ivars_of(id).is_empty();
        if needs_ivar_wrap {
            let _ = self.writer.write_ivar_wrap_tag();
        }

        for module in self.arena.extensions_of(id) {
            let _ = self.writer.write_extended_tag();
            self.write_symbol_ref(module);
        }

        if wrapped {
            let tag = if node.flags.contains(Flags::DATA) {
                crate::wire::Tag::Data
            } else {
                crate::wire::Tag::UserMarshal
            };
            self.write_class_name_tag(tag, node.class);
            self.number(id);
            if needs_ivar_wrap {
                self.work.push(Work::TrailingIvars {
                    encoding_id,
                    target: id,
                });
            }
            // SAFETY of the unwrap: every node carrying a wrapper flag was created together with its
            // `wrappers` entry, by `push_wrapper` or by the loader's `DecorateWrap`.
            let body = self.arena.body_of(id).expect("a wrapper node has a body");
            self.work.push(Work::Value(body));
            return;
        }

        let mut class_written = false;
        if node.flags.contains(Flags::USER_CLASS) {
            let _ = self.writer.write_uclass_tag();
            self.write_symbol_ref(node.class);
            class_written = true;
        } else if node.flags.contains(Flags::USER_DEFINED) {
            self.write_class_name_tag(crate::wire::Tag::UserDef, node.class);
            let _ = self.writer.write_chunk(self.arena.blob(node.a));
            if needs_ivar_wrap {
                self.work.push(Work::Number(id));
                self.work.push(Work::TrailingIvars {
                    encoding_id,
                    target: id,
                });
            } else {
                self.number(id);
            }
            return;
        }

        // Everything else is numbered after the markers above (the names of an `extend`ed module and of a subclass
        // are written before it, and an encoding name there is an object too) and before its own class name.
        self.number(id);

        // Pushed before the children so it's popped after them: it trails the value's whole body.
        if needs_ivar_wrap {
            self.work.push(Work::TrailingIvars {
                encoding_id,
                target: id,
            });
        }

        match node.kind {
            Kind::Nil | Kind::True | Kind::False | Kind::Fixnum | Kind::Symbol => unreachable!("written above"),

            Kind::Bignum => {
                let magnitude = self.arena.blob(node.a);
                let _ = self
                    .writer
                    .write_bignum(node.flags.contains(Flags::NEGATIVE), magnitude);
            }

            Kind::Float => {
                let _ = self.writer.write_float(self.arena.blob(node.a));
            }

            // Same wire shape as `Bytes`; only the outer ivar wrap and trailing pair differ.
            Kind::Bytes | Kind::Str => {
                let _ = self.writer.write_string_bytes(self.arena.blob(node.a));
            }

            Kind::Regexp => {
                let _ = self.writer.write_regexp(self.arena.blob(node.a), node.b as u8);
            }

            Kind::Array => {
                let _ = self.writer.write_array_header(node.b);
                for i in (0..node.b).rev() {
                    let child = self.arena.children[(node.a + i) as usize];
                    self.work.push(Work::Value(child));
                }
            }

            Kind::Hash => {
                let _ = self
                    .writer
                    .write_hash_header(node.b, node.flags.contains(Flags::HAS_DEFAULT));
                if node.flags.contains(Flags::HAS_DEFAULT) {
                    let default = self.arena.children[(node.a + node.b * 2) as usize];
                    self.work.push(Work::Value(default));
                }
                for i in (0..node.b).rev() {
                    let key = self.arena.children[(node.a + i * 2) as usize];
                    let value = self.arena.children[(node.a + i * 2 + 1) as usize];
                    self.work.push(Work::Value(value));
                    self.work.push(Work::Value(key));
                }
            }

            Kind::Struct => {
                if !class_written {
                    let _ = self.writer.write_tag(crate::wire::Tag::Struct);
                    self.write_symbol_ref(node.class);
                }
                let _ = self.writer.write_len(node.b);
                self.push_members(node.a, node.b);
            }

            Kind::Object => {
                if !class_written {
                    let _ = self.writer.write_tag(crate::wire::Tag::Object);
                    self.write_symbol_ref(node.class);
                }
                let _ = self.writer.write_len(node.b);
                self.push_members(node.a, node.b);
            }

            Kind::Class => {
                if !class_written {
                    let _ = self.writer.write_class_name(self.arena.blob(node.a));
                }
            }

            Kind::Module => {
                if !class_written {
                    let old = node.flags.contains(Flags::OLD_MODULE);
                    let _ = self.writer.write_module_name(self.arena.blob(node.a), old);
                }
            }
        }
    }

    fn write_scalar(&mut self, kind: Kind, payload: u32) {
        match kind {
            Kind::Nil => drop(self.writer.write_nil()),
            Kind::True => drop(self.writer.write_bool(true)),
            Kind::False => drop(self.writer.write_bool(false)),
            #[allow(clippy::cast_possible_wrap)]
            Kind::Fixnum => drop(self.writer.write_fixnum(payload as i32)),
            Kind::Symbol => self.write_symbol_ref(payload),
            _ => unreachable!("not a scalar kind"),
        }
    }

    /// Queues `len` members starting at `start`, each as its name and then its value.
    fn push_members(&mut self, start: u32, len: u32) {
        for i in (0..len).rev() {
            let (name, value) = self.arena.members[(start + i) as usize];
            self.work.push(Work::Value(value));
            self.work.push(Work::MemberName(name));
        }
    }

    fn write_class_name_tag(&mut self, tag: crate::wire::Tag, class: SymId) {
        let _ = self.writer.write_tag(tag);
        self.write_symbol_ref(class);
    }

    /// Writes the ivar count and pairs that follow an outer `TYPE_IVAR` wrap: the encoding pair, if
    /// any, then the value's other instance variables (their names and values are queued).
    #[allow(clippy::cast_possible_truncation)] // the count is at most the number of nodes
    fn write_trailing_ivars(&mut self, encoding_id: Option<u8>, target: ValueId) {
        let extras = self.arena.extra_ivars_of(target);
        let _ = self
            .writer
            .write_len(usize::from(encoding_id.is_some()) as u32 + extras.len() as u32);
        if let Some(encoding_id) = encoding_id {
            self.write_trailing_encoding_ivar(encoding_id, target);
        }
        for &(_, name, value) in extras.iter().rev() {
            self.work.push(Work::Value(value));
            self.work.push(Work::MemberName(name));
        }
    }

    /// Writes the single trailing ivar pair that follows an outer `TYPE_IVAR` wrap for a
    /// `Kind::Str`/encoding-tagged `Kind::Regexp` value. `target` is the wrapped value's own id, needed to look
    /// up a [`crate::encoding::ENCODING_CUSTOM`] name.
    fn write_trailing_encoding_ivar(&mut self, encoding_id: u8, target: ValueId) {
        let arena = self.arena;
        self.write_encoding_ivar(encoding_id, arena.custom_encoding_of(target));
    }

    /// The encoding pair of a string, regexp or symbol: `:E => true/false` for UTF-8/US-ASCII, otherwise
    /// `:encoding => "<name>"`. The name is written once per stream and linked afterwards.
    fn write_encoding_ivar(&mut self, encoding_id: u8, custom: Option<&'r [u8]>) {
        match encoding_id {
            crate::encoding::ENCODING_UTF_8 => {
                self.write_symbol_of_e();
                let _ = self.writer.write_bool(true);
            }
            crate::encoding::ENCODING_US_ASCII => {
                self.write_symbol_of_e();
                let _ = self.writer.write_bool(false);
            }
            id => {
                self.write_symbol_of_encoding();
                let name = if id == crate::encoding::ENCODING_CUSTOM {
                    custom.unwrap_or(b"")
                } else {
                    crate::encoding::encoding_name(id).unwrap_or(b"")
                };
                if let Some(&(_, index)) = self.encoding_names.iter().find(|&&(known, _)| known == name) {
                    let _ = self.writer.write_object_link(index);
                } else {
                    self.encoding_names.push((name, self.next_obj));
                    // A stream that links to this name string as an object (nothing Ruby writes does) gets the link back.
                    if let Some(node) = self
                        .arena
                        .name_node(name)
                        .filter(|&node| self.obj_link[node as usize] == u32::MAX)
                    {
                        self.obj_link[node as usize] = self.next_obj;
                    }
                    self.next_obj += 1;
                    let _ = self.writer.write_string_bytes(name);
                }
            }
        }
    }

    /// Writes a fixed ASCII symbol (`E`, `encoding`) that has no `SymId` in the arena; it is synthesized
    /// here, not read from the source. The first use writes it fresh, later ones link to it via a small local cache.
    fn write_fixed_symbol(&mut self, slot: usize, name: &[u8]) {
        if self.fixed_symbol_links[slot] == u32::MAX {
            self.fixed_symbol_links[slot] = self.next_sym;
            self.next_sym += 1;
            let _ = self.writer.write_symbol_new(name);
        } else {
            let _ = self.writer.write_symbol_link(self.fixed_symbol_links[slot]);
        }
    }

    fn write_symbol_of_e(&mut self) {
        self.write_fixed_symbol(0, b"E");
    }

    fn write_symbol_of_encoding(&mut self) {
        self.write_fixed_symbol(1, b"encoding");
    }
}
