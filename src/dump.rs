//! Writes an [`Arena`] back to a Marshal byte stream.
//!
//! Traversal is iterative, over an explicit work stack: an [`Arena`] loaded from
//! hostile bytes can be as deep as the input is long (two bytes an array level),
//! so recursing here would let a 200 KB file overflow the call stack.

use crate::{
    arena::{Arena, Flags, Kind, SymId, ValueId},
    writer::Writer,
};
use alloc::vec::Vec;

/// Serializes `arena` back to Marshal bytes.
///
/// Infallible: every value in an [`Arena`] was constructed through
/// [`crate::load::load`] or the [`Arena`] builder API, both of which only
/// ever produce internally consistent nodes, and the `Vec<u8>` sink cannot
/// fail to grow.
#[must_use]
pub fn dump(arena: &Arena<'_>) -> Vec<u8> {
    let mut dumper = Dumper {
        writer: Writer::new(Vec::with_capacity(1024)),
        arena,
        sym_link: alloc::vec![u32::MAX; arena.symbols.len()],
        obj_link: alloc::vec![u32::MAX; arena.nodes.len()],
        next_sym: 0,
        next_obj: 0,
        e_symbol_link: u32::MAX,
        encoding_symbol_link: u32::MAX,
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
    e_symbol_link: u32,
    encoding_symbol_link: u32,
    work: Vec<Work>,
}

/// One pending write. Items are popped in the order the wire format wants them, so a container
/// pushes its parts in reverse.
enum Work {
    Value(ValueId),
    /// An object/struct member name, which precedes that member's value.
    MemberName(SymId),
    /// The ivar pair that follows a string's or regexp's whole body.
    TrailingEncoding { encoding_id: u8, target: ValueId },
}

impl Dumper<'_, '_> {
    fn write_value(&mut self, root: ValueId) {
        self.work.push(Work::Value(root));
        while let Some(item) = self.work.pop() {
            match item {
                Work::Value(id) => self.write_node(id),
                Work::MemberName(sym) => self.write_member_name(sym),
                Work::TrailingEncoding { encoding_id, target } => {
                    self.write_trailing_encoding_ivar(encoding_id, target);
                }
            }
        }
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
        let _ = self.writer.write_symbol_new(self.arena.symbol_bytes(sym));
    }

    /// Writes one member/ivar-name symbol without the "already written?"
    /// dedup fast path short-circuiting into a link for the *first* use -
    /// identical to `write_symbol_ref`, kept as a separate name at call
    /// sites for readability (member names are always fresh-or-linked
    /// symbols, never a general value).
    fn write_member_name(&mut self, sym: SymId) {
        self.write_symbol_ref(sym);
    }

    // One long exhaustive match over every `Kind`/wrapper-flag combination -
    // splitting it up would just scatter one linear dispatch across several
    // functions, not make it clearer. The `node.a`/`node.b as _` casts are
    // bit-preserving reinterprets of a `Node`'s packed payload (a fixnum's
    // bits, a `Str`/`Regexp`'s one-byte encoding id, a `Regexp`'s option
    // byte) - not range-narrowing.
    #[allow(
        clippy::too_many_lines,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap
    )]
    fn write_node(&mut self, id: ValueId) {
        let node = *self.arena.node(id);

        match node.kind {
            Kind::Nil => {
                let _ = self.writer.write_nil();
                return;
            }
            Kind::True => {
                let _ = self.writer.write_bool(true);
                return;
            }
            Kind::False => {
                let _ = self.writer.write_bool(false);
                return;
            }
            Kind::Fixnum => {
                let _ = self.writer.write_fixnum(node.a as i32);
                return;
            }
            Kind::Symbol => {
                self.write_symbol_ref(node.a);
                return;
            }
            _ => {}
        }

        let slot = &mut self.obj_link[id as usize];
        if *slot != u32::MAX {
            let _ = self.writer.write_object_link(*slot);
            return;
        }
        let idx = self.next_obj;
        self.next_obj += 1;
        *slot = idx;

        // `TYPE_IVAR` is the OUTERMOST wrapper when present - it wraps the
        // entire extended/uclass/data/... construct, not just the bare
        // string - and its ivar pairs trail after everything else, so both
        // the tag and the trailing pair are handled here rather than inside
        // the `Kind::Str`/`Kind::Regexp` arms below. A declared encoding of
        // `ASCII-8BIT` needs no ivar at all - matching Ruby, which only
        // ever writes `E`/`encoding` for a *non-default* encoding - so a
        // `Kind::Str` node built with that id (only reachable through the
        // builder API; the loader never produces it, since Ruby never
        // writes the ivar for it) dumps identically to plain `Bytes`.
        let encoding_id = match node.kind {
            Kind::Str => Some(node.b as u8),
            Kind::Regexp => Some((node.b >> 8) as u8),
            _ => None,
        }
        .filter(|&id| id != crate::encoding::ENCODING_ASCII_8BIT);
        let needs_ivar_wrap = encoding_id.is_some();
        if needs_ivar_wrap {
            let _ = self.writer.write_ivar_wrap_tag();
        }

        for module in self.arena.extensions_of(id) {
            let _ = self.writer.write_extended_tag();
            self.write_symbol_ref(module);
        }

        let mut class_written = false;
        if node.flags.contains(Flags::DATA) {
            let _ = self.writer.write_tag(crate::wire::Tag::Data);
            self.write_symbol_ref(node.class);
            class_written = true;
        } else if node.flags.contains(Flags::USER_CLASS) {
            let _ = self.writer.write_uclass_tag();
            self.write_symbol_ref(node.class);
            class_written = true;
        } else if node.flags.contains(Flags::USER_DEFINED) {
            self.write_class_name_tag(crate::wire::Tag::UserDef, node.class);
            let _ = self.writer.write_chunk(self.arena.blob(node.a));
            if let Some(encoding_id) = encoding_id {
                self.write_trailing_encoding_ivar(encoding_id, id);
            }
            return;
        } else if node.flags.contains(Flags::USER_MARSHAL) {
            self.write_class_name_tag(crate::wire::Tag::UserMarshal, node.class);
            class_written = true;
        }

        // Pushed before the children so it's popped after them: it trails the value's whole body.
        if let Some(encoding_id) = encoding_id {
            self.work.push(Work::TrailingEncoding { encoding_id, target: id });
        }

        match node.kind {
            Kind::Nil | Kind::True | Kind::False | Kind::Fixnum | Kind::Symbol => unreachable!(),

            Kind::Bignum => {
                let magnitude = self.arena.blob(node.a);
                let _ = self
                    .writer
                    .write_bignum(node.flags.contains(Flags::NEGATIVE), magnitude);
            }

            Kind::Float => {
                let _ = self.writer.write_float(self.arena.blob(node.a));
            }

            // `Str`'s bytes are exactly what was declared, untouched (see
            // `Kind::Str`'s doc comment); the wire shape is identical to
            // `Bytes` (a bare `"` + chunk) - only the outer ivar-wrap above
            // (and the trailing pair below) differ.
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

    /// Writes the single trailing ivar pair that follows an outer
    /// `TYPE_IVAR` wrap for a `Kind::Str`/encoding-tagged `Kind::Regexp`
    /// value - `:E => true`/`:E => false` for UTF-8/US-ASCII (Ruby's own
    /// shorthand for the two most common encodings), `:encoding => "<name>"`
    /// for everything else. `target` is the wrapped value's own id, needed
    /// to look up a [`crate::encoding::ENCODING_CUSTOM`] name.
    fn write_trailing_encoding_ivar(&mut self, encoding_id: u8, target: ValueId) {
        let _ = self.writer.write_len(1);
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
                    self.arena.custom_encoding_of(target).unwrap_or(b"")
                } else {
                    crate::encoding::encoding_name(id).unwrap_or(b"")
                };
                let _ = self.writer.write_string_bytes(name);
            }
        }
    }

    /// Writes a fixed ASCII symbol (`"E"`) that isn't backed by a `SymId` in
    /// the arena - it's synthesized here, not read from the source stream -
    /// so it always goes through the fresh-symbol path (and joins the
    /// regular dedup table for the rest of this dump).
    fn write_symbol_of_e(&mut self) {
        // Not a real `SymId` lookup: this literal is short-lived enough
        // that deduping it against the arena's own table isn't worth a
        // dynamic intern call mid-dump. The first dumped `:E` writes it
        // fresh and links to that occurrence afterward via a tiny local
        // cache.
        if self.e_symbol_link == u32::MAX {
            self.e_symbol_link = self.next_sym;
            self.next_sym += 1;
            let _ = self.writer.write_symbol_new(b"E");
        } else {
            let _ = self.writer.write_symbol_link(self.e_symbol_link);
        }
    }

    /// Same idea as [`Dumper::write_symbol_of_e`], for the `"encoding"`
    /// symbol.
    fn write_symbol_of_encoding(&mut self) {
        if self.encoding_symbol_link == u32::MAX {
            self.encoding_symbol_link = self.next_sym;
            self.next_sym += 1;
            let _ = self.writer.write_symbol_new(b"encoding");
        } else {
            let _ = self.writer.write_symbol_link(self.encoding_symbol_link);
        }
    }
}
