#![no_main]
//! The streaming reader over a fixed symbol table, the allocation-free entry point. Any bytes must read to an
//! error or the end of the stream, never a panic.
use libfuzzer_sys::fuzz_target;
use marshal_rs::{FixedSymbolTable, Reader, Span};

fuzz_target!(|data: &[u8]| {
    let mut slots = [Span { offset: 0, len: 0 }; 64];
    let mut symbols = FixedSymbolTable::new(&mut slots);
    let Ok(mut reader) = Reader::new(data, &mut symbols) else {
        return;
    };
    // Every token reads at least its tag byte, so the loop ends at the end of input or at the first error.
    while reader.remaining() > 0 {
        if reader.next().is_err() {
            break;
        }
    }
});
