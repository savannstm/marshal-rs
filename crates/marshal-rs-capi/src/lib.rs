#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::missing_safety_doc)]

extern crate alloc;

use alloc::{boxed::Box, string::String, vec::Vec};
use core::slice;
use marshal_rs::{Arena, Kind, ReadError, ValueId, ValueRef};

#[cfg(not(feature = "std"))]
mod host_alloc {
    use core::{
        alloc::{GlobalAlloc, Layout},
        ptr,
        sync::atomic::{AtomicUsize, Ordering},
    };

    pub type MrsAllocFn = unsafe extern "C" fn(size: usize, align: usize) -> *mut u8;
    pub type MrsFreeFn = unsafe extern "C" fn(ptr: *mut u8, size: usize, align: usize);

    static ALLOC_FN: AtomicUsize = AtomicUsize::new(0);
    static FREE_FN: AtomicUsize = AtomicUsize::new(0);

    // SAFETY (caller contract, see README.md): `alloc`/`free` must be valid for the remaining
    // lifetime of the process, safe to call from any thread this library's functions are called
    // from, and `free` must accept exactly the `(ptr, size, align)` triples a prior call to
    // `alloc` handed out (same values, not just an equivalent layout).
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn mrs_set_allocator(alloc: MrsAllocFn, free: MrsFreeFn) {
        ALLOC_FN.store(alloc as usize, Ordering::Release);
        FREE_FN.store(free as usize, Ordering::Release);
    }

    struct HostAlloc;

    unsafe impl GlobalAlloc for HostAlloc {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let f = ALLOC_FN.load(Ordering::Acquire);
            if f == 0 {
                return ptr::null_mut();
            }
            // SAFETY: the only value ever stored is a `MrsAllocFn` cast to
            // `usize` by `mrs_set_allocator`.
            let alloc: MrsAllocFn = unsafe { core::mem::transmute(f) };
            unsafe { alloc(layout.size(), layout.align()) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            let f = FREE_FN.load(Ordering::Acquire);
            if f == 0 {
                return;
            }
            // SAFETY: see `alloc` above; `mrs_set_allocator` always sets both
            // atomics together.
            let free: MrsFreeFn = unsafe { core::mem::transmute(f) };
            unsafe { free(ptr, layout.size(), layout.align()) };
        }
    }

    #[global_allocator]
    static HOST_ALLOC: HostAlloc = HostAlloc;

    #[panic_handler]
    fn panic(_info: &core::panic::PanicInfo) -> ! {
        unsafe extern "C" {
            fn abort() -> !;
        }
        unsafe { abort() }
    }
}

#[cfg(not(feature = "std"))]
pub use host_alloc::mrs_set_allocator;

pub struct MrsArena(Arena<'static>);

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MrsError {
    Ok = 0,
    UnexpectedEof = 1,
    InvalidHeader = 2,
    InvalidTag = 3,
    SymbolTableFull = 4,
    UnknownSymbolLink = 5,
    UnknownObjectLink = 6,
    LengthOverflow = 7,
    Unsupported = 8,
}

impl From<ReadError> for MrsError {
    fn from(err: ReadError) -> Self {
        match err {
            ReadError::UnexpectedEof { .. } => MrsError::UnexpectedEof,
            ReadError::InvalidHeader => MrsError::InvalidHeader,
            ReadError::InvalidTag { .. } => MrsError::InvalidTag,
            ReadError::SymbolTableFull => MrsError::SymbolTableFull,
            ReadError::UnknownSymbolLink { .. } => MrsError::UnknownSymbolLink,
            ReadError::UnknownObjectLink { .. } => MrsError::UnknownObjectLink,
            ReadError::LengthOverflow => MrsError::LengthOverflow,
            ReadError::Unsupported(_) => MrsError::Unsupported,
        }
    }
}

// SAFETY: `buf` must point to at least `len` readable bytes. `out_error`, if non-null, must point
// to a writable `MrsError`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mrs_load(buf: *const u8, len: usize, out_error: *mut MrsError) -> *mut MrsArena {
    let bytes = unsafe { slice::from_raw_parts(buf, len) };
    match marshal_rs::load(bytes) {
        Ok(arena) => {
            if !out_error.is_null() {
                unsafe { *out_error = MrsError::Ok };
            }
            Box::into_raw(Box::new(MrsArena(arena.into_owned())))
        }
        Err(err) => {
            if !out_error.is_null() {
                unsafe { *out_error = err.into() };
            }
            core::ptr::null_mut()
        }
    }
}

// SAFETY: `arena` must be a handle previously returned by `mrs_load` (or built via this module)
// and not already freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mrs_arena_free(arena: *mut MrsArena) {
    if !arena.is_null() {
        drop(unsafe { Box::from_raw(arena) });
    }
}

// SAFETY: `arena` must be a valid handle. `out_len` must point to a writable `usize`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mrs_dump(arena: *const MrsArena, out_len: *mut usize) -> *mut u8 {
    let arena = unsafe { &(*arena).0 };
    let mut bytes = marshal_rs::dump(arena);
    bytes.shrink_to_fit();
    let len = bytes.len();
    let ptr = bytes.as_mut_ptr();
    core::mem::forget(bytes);
    unsafe { *out_len = len };
    ptr
}

// SAFETY: `buf`/`len` must be exactly the pointer/length pair returned together by a single
// `mrs_dump` call, not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mrs_buffer_free(buf: *mut u8, len: usize) {
    if !buf.is_null() {
        drop(unsafe { Vec::from_raw_parts(buf, len, len) });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_root(arena: &MrsArena) -> ValueId {
    arena.0.root()
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_kind(arena: &MrsArena, id: ValueId) -> Kind {
    arena.0.node(id).kind
}

fn value<'r>(arena: &'r MrsArena, id: ValueId) -> ValueRef<'r, 'static> {
    ValueRef::new(&arena.0, id)
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_as_bool(arena: &MrsArena, id: ValueId, out: &mut bool) -> bool {
    match value(arena, id).as_bool() {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_as_i64(arena: &MrsArena, id: ValueId, out: &mut i64) -> bool {
    match value(arena, id).as_i64() {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_as_f64(arena: &MrsArena, id: ValueId, out: &mut f64) -> bool {
    match value(arena, id).as_f64() {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_as_bytes(arena: &MrsArena, id: ValueId, out_ptr: &mut *const u8, out_len: &mut usize) -> bool {
    let v = value(arena, id);
    let bytes = v
        .as_bytes()
        .or_else(|| v.as_symbol_bytes())
        .or_else(|| v.as_str().map(str::as_bytes));
    match bytes {
        Some(b) => {
            *out_ptr = b.as_ptr();
            *out_len = b.len();
            true
        }
        None => false,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_class_name(arena: &MrsArena, id: ValueId, out_ptr: &mut *const u8, out_len: &mut usize) -> bool {
    match value(arena, id).class_name() {
        Some(b) => {
            *out_ptr = b.as_ptr();
            *out_len = b.len();
            true
        }
        None => false,
    }
}

// Ruby stores a Class/Module's own path as a plain string, not a symbol - distinct from
// `mrs_class_name`, which is the declared class *of* a value, not a Class/Module value's own
// name.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_as_path(arena: &MrsArena, id: ValueId, out_ptr: &mut *const u8, out_len: &mut usize) -> bool {
    match value(arena, id).as_path() {
        Some(b) => {
            *out_ptr = b.as_ptr();
            *out_len = b.len();
            true
        }
        None => false,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_as_regexp(
    arena: &MrsArena,
    id: ValueId,
    out_ptr: &mut *const u8,
    out_len: &mut usize,
    out_options: &mut u8,
) -> bool {
    match value(arena, id).as_regexp() {
        Some((source, options)) => {
            *out_ptr = source.as_ptr();
            *out_len = source.len();
            *out_options = options;
            true
        }
        None => false,
    }
}

// `0` (MRS_ENCODING_ASCII_8BIT) is Ruby's own default for an untagged Bytes value. `255`
// (MRS_ENCODING_CUSTOM) means the name isn't in the fixed table; resolve it with
// `mrs_encoding_name` instead of hardcoding against the id.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_encoding_id(arena: &MrsArena, id: ValueId, out: &mut u8) -> bool {
    match value(arena, id).encoding_id() {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_encoding_name(
    arena: &MrsArena,
    id: ValueId,
    out_ptr: &mut *const u8,
    out_len: &mut usize,
) -> bool {
    match value(arena, id).encoding_name() {
        Some(b) => {
            *out_ptr = b.as_ptr();
            *out_len = b.len();
            true
        }
        None => false,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_as_bignum_decimal(
    arena: &MrsArena,
    id: ValueId,
    out_ptr: &mut *mut u8,
    out_len: &mut usize,
) -> bool {
    match value(arena, id).as_bigint_decimal() {
        Some(text) => {
            let mut bytes: Vec<u8> = String::into_bytes(text);
            bytes.shrink_to_fit();
            *out_len = bytes.len();
            *out_ptr = bytes.as_mut_ptr();
            core::mem::forget(bytes);
            true
        }
        None => false,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_array_len(arena: &MrsArena, id: ValueId) -> u32 {
    value(arena, id).len() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_array_get(arena: &MrsArena, id: ValueId, index: u32) -> ValueId {
    value(arena, id).at(index as usize).map_or(ValueId::MAX, |v| v.id())
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_hash_len(arena: &MrsArena, id: ValueId) -> u32 {
    value(arena, id).len() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_hash_key_at(arena: &MrsArena, id: ValueId, index: u32) -> ValueId {
    value(arena, id)
        .entries()
        .nth(index as usize)
        .map_or(ValueId::MAX, |(k, _)| k.id())
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_hash_value_at(arena: &MrsArena, id: ValueId, index: u32) -> ValueId {
    value(arena, id)
        .entries()
        .nth(index as usize)
        .map_or(ValueId::MAX, |(_, v)| v.id())
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_members_len(arena: &MrsArena, id: ValueId) -> u32 {
    value(arena, id).len() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_member_name_at(
    arena: &MrsArena,
    id: ValueId,
    index: u32,
    out_ptr: &mut *const u8,
    out_len: &mut usize,
) -> bool {
    match value(arena, id).members().nth(index as usize) {
        Some((name, _)) => {
            *out_ptr = name.as_ptr();
            *out_len = name.len();
            true
        }
        None => false,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mrs_member_value_at(arena: &MrsArena, id: ValueId, index: u32) -> ValueId {
    value(arena, id)
        .members()
        .nth(index as usize)
        .map_or(ValueId::MAX, |(_, v)| v.id())
}
