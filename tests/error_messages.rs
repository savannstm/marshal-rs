//! The error messages are what a user sees when a file fails to load or write, so each variant's wording and numbers are
//! pinned here. A change to one of them should be deliberate.

use marshal_rs::{ReadError, WriteError};

#[test]
fn every_read_error_names_what_failed_and_where() {
    let cases = [
        (
            ReadError::UnexpectedEof { offset: 7 },
            "unexpected end of input at byte 7",
        ),
        (
            ReadError::InvalidHeader,
            "not a Marshal 4.8 byte stream (bad 2-byte header)",
        ),
        (
            ReadError::InvalidTag { byte: 0x2a, offset: 3 },
            "unexpected type tag 0x2a at byte 3",
        ),
        (ReadError::SymbolTableFull, "symbol table capacity exceeded"),
        (
            ReadError::UnknownSymbolLink { idx: 4 },
            "symbol link 4 has no matching symbol",
        ),
        (
            ReadError::UnknownObjectLink { idx: 5 },
            "object link 5 has no matching object",
        ),
        (ReadError::LengthOverflow, "declared length exceeds remaining input"),
        (
            ReadError::NegativeLength { value: -1, offset: 9 },
            "negative length -1 at byte 9",
        ),
        (
            ReadError::TrailingData { offset: 11 },
            "unexpected data after the value, at byte 11",
        ),
        (
            ReadError::Unsupported("symbol ivar"),
            "unsupported construct: symbol ivar",
        ),
        (
            ReadError::EmptyName { offset: 2 },
            "empty class or module name at byte 2",
        ),
    ];

    for (error, message) in cases {
        assert_eq!(error.to_string(), message);
    }
}

#[test]
fn a_full_output_buffer_says_so() {
    assert_eq!(WriteError::BufferFull.to_string(), "output buffer capacity exceeded");
}
