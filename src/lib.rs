#![cfg_attr(not(test), no_std)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![doc = include_str!("../README.md")]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub mod reader;
pub mod wire;
pub mod writer;

#[cfg(feature = "alloc")]
pub mod arena;
#[cfg(feature = "alloc")]
pub mod bignum;
#[cfg(feature = "alloc")]
pub mod dump;
#[cfg(feature = "alloc")]
pub mod encoding;
#[cfg(feature = "alloc")]
pub mod load;
#[cfg(feature = "alloc")]
pub mod value;

#[cfg(feature = "serde")]
pub mod ser;

pub use reader::{FixedSymbolTable, ReadError, Reader, Span, SymbolTable, Token};
pub use wire::Tag;
pub use writer::{Sink, WriteError, Writer};

#[cfg(feature = "alloc")]
pub use arena::{Arena, Flags, Kind, SymId, ValueId};
#[cfg(feature = "alloc")]
pub use dump::dump;
#[cfg(feature = "alloc")]
pub use encoding::{
    ENCODING_ASCII_8BIT, ENCODING_CUSTOM, ENCODING_US_ASCII, ENCODING_UTF_8, encoding_id, encoding_name,
};
#[cfg(feature = "alloc")]
pub use load::load;
#[cfg(feature = "alloc")]
pub use value::ValueRef;
