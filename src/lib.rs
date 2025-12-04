#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::needless_doctest_main)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::deref_addrof)]
#![doc = include_str!("../README.md")]

mod constants;
pub mod dump;
pub mod load;
pub mod types;

pub use dump::{Dumper, dump};
pub use load::{LoadError, Loader, StringMode, load, load_binary, load_utf8};
pub use types::{Get, HashMap, Object, Value, ValueType};

thread_local! {
    pub(crate) static VALUE_INSTANCE_COUNTER: types::SafeCell<usize> = const { types::SafeCell::new(0) };
}
