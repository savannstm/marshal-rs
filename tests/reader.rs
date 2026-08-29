#![allow(clippy::cast_sign_loss)]

use marshal_rs::{FixedSymbolTable, ReadError, Reader, Span, Token};

// Tests use `FixedSymbolTable` (rather than the `alloc`-only `Vec<Span>`
// impl) so this module exercises correctly under `--no-default-features`
// too.
macro_rules! symtab {
    () => {
        [Span { offset: 0, len: 0 }; 16]
    };
}

#[test]
fn nil_true_false() {
    let mut slots = symtab!();
    let mut syms = FixedSymbolTable::new(&mut slots);
    let mut r = Reader::new(&[4, 8, b'0'], &mut syms).unwrap();
    assert_eq!(r.next().unwrap(), Token::Nil);

    let mut slots = symtab!();
    let mut syms = FixedSymbolTable::new(&mut slots);
    let mut r = Reader::new(&[4, 8, b'T'], &mut syms).unwrap();
    assert_eq!(r.next().unwrap(), Token::True);

    let mut slots = symtab!();
    let mut syms = FixedSymbolTable::new(&mut slots);
    let mut r = Reader::new(&[4, 8, b'F'], &mut syms).unwrap();
    assert_eq!(r.next().unwrap(), Token::False);
}

#[test]
fn fixnum() {
    let mut slots = symtab!();
    let mut syms = FixedSymbolTable::new(&mut slots);
    let mut r = Reader::new(&[4, 8, b'i', 0], &mut syms).unwrap();
    assert_eq!(r.next().unwrap(), Token::Fixnum(0));

    let mut slots = symtab!();
    let mut syms = FixedSymbolTable::new(&mut slots);
    let mut r = Reader::new(&[4, 8, b'i', 1 + 5], &mut syms).unwrap();
    assert_eq!(r.next().unwrap(), Token::Fixnum(1));

    let mut slots = symtab!();
    let mut syms = FixedSymbolTable::new(&mut slots);
    let mut r = Reader::new(&[4, 8, b'i', (-1i8 - 5) as u8], &mut syms).unwrap();
    assert_eq!(r.next().unwrap(), Token::Fixnum(-1));
}

#[test]
fn bad_header_is_rejected() {
    let mut slots = symtab!();
    let mut syms = FixedSymbolTable::new(&mut slots);
    assert!(Reader::new(&[4, 7, b'0'], &mut syms).is_err());
}

#[test]
fn symbol_then_symlink_resolve_to_same_bytes() {
    // Symbol/string chunk lengths are packed ints too: length 3 is
    // encoded as the byte `3 + 5`, not the literal `3`.
    let bytes = [4, 8, b'[', 2 + 5, b':', 3 + 5, b'a', b'b', b'c', b';', 0];
    let mut slots = symtab!();
    let mut syms = FixedSymbolTable::new(&mut slots);
    let mut reader = Reader::new(&bytes, &mut syms).unwrap();
    assert_eq!(reader.next().unwrap(), Token::BeginArray(2));
    assert_eq!(reader.next().unwrap(), Token::Symbol(b"abc"));
    assert_eq!(reader.next().unwrap(), Token::Symbol(b"abc"));
}

#[test]
fn fixed_symbol_table_reports_full() {
    let mut slots = [Span { offset: 0, len: 0 }; 1];
    let mut table = FixedSymbolTable::new(&mut slots);
    let bytes = [4, 8, b'[', 2 + 5, b':', 1 + 5, b'a', b':', 1 + 5, b'b'];
    let mut reader = Reader::new(&bytes, &mut table).unwrap();
    assert_eq!(reader.next().unwrap(), Token::BeginArray(2));
    assert_eq!(reader.next().unwrap(), Token::Symbol(b"a"));
    assert_eq!(reader.next().unwrap_err(), ReadError::SymbolTableFull);
}

#[test]
fn hostile_length_is_rejected_before_allocation() {
    let bytes = [4, 8, b'[', 4];
    let mut slots = symtab!();
    let mut syms = FixedSymbolTable::new(&mut slots);
    let mut reader = Reader::new(&bytes, &mut syms).unwrap();
    assert!(matches!(reader.next(), Err(ReadError::UnexpectedEof { .. })));
}
