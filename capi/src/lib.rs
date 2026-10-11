#![cfg_attr(not(feature = "std"), no_std)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]

extern crate alloc;

use alloc::{boxed::Box, string::String, vec::Vec};
use core::slice;
use marshal_rs::{Arena, Kind, ReadError, ValueId, ValueRef};

/// A `MrsValueId` is an index into an `MrsArena`. `MRS_VALUE_ID_NONE` marks "no value", for
/// example an out-of-range array, hash or member lookup.
pub type MrsValueId = u32;

/// See [`MrsValueId`].
pub const MRS_VALUE_ID_NONE: MrsValueId = u32::MAX;

// `marshal_rs::ValueId` is a plain `u32`, so the C type can be spelled as one.
const _: fn(ValueId) -> MrsValueId = |id| id;

/// `mrs_encoding_id` of `ASCII-8BIT`, Ruby's default encoding.
pub const MRS_ENCODING_ASCII_8BIT: u8 = 0;

/// `mrs_encoding_id` of an encoding whose name isn't in the fixed table; resolve it with
/// `mrs_encoding_name` instead of hardcoding against the id.
pub const MRS_ENCODING_CUSTOM: u8 = 255;

// The C constants are spelled as literals for cbindgen, which can't see into the core crate.
const _: () = {
    assert!(MRS_ENCODING_ASCII_8BIT == marshal_rs::ENCODING_ASCII_8BIT);
    assert!(MRS_ENCODING_CUSTOM == marshal_rs::ENCODING_CUSTOM);
};

/// Host allocation hook: returns a pointer to at least `size` bytes aligned to `align` (a power of
/// two), or `NULL` on failure.
///
/// Not assumed to be `malloc` - back it with whatever allocator the target actually has. See
/// `mrs_set_allocator`.
pub type MrsAllocFn = unsafe extern "C" fn(size: usize, align: usize) -> *mut u8;

/// Host deallocation hook: gets back exactly the `(ptr, size, align)` triple a prior `MrsAllocFn`
/// call produced. See `mrs_set_allocator`.
pub type MrsFreeFn = unsafe extern "C" fn(ptr: *mut u8, size: usize, align: usize);

/// Registers the host's allocator. Must be called exactly once, before any other `mrs_*` function,
/// in a `no_std` build of this library (see the `no_std` section of this crate's README).
///
/// It has no effect, and need not be called, in the default `std`-linked build, which uses Rust's
/// own allocator.
///
/// # Safety
///
/// `alloc`/`free` must be valid for the remaining lifetime of the process, safe to call from any
/// thread this library's functions are called from, and `free` must accept exactly the
/// `(ptr, size, align)` triples a prior call to `alloc` handed out (same values, not just an
/// equivalent layout).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mrs_set_allocator(alloc: MrsAllocFn, free: MrsFreeFn) {
    #[cfg(not(feature = "std"))]
    host_alloc::set(alloc, free);
    #[cfg(feature = "std")]
    let _ = (alloc, free);
}

#[cfg(not(feature = "std"))]
mod host_alloc {
    use super::{MrsAllocFn, MrsFreeFn};
    use core::{
        alloc::{GlobalAlloc, Layout},
        ptr,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static ALLOC_FN: AtomicUsize = AtomicUsize::new(0);
    static FREE_FN: AtomicUsize = AtomicUsize::new(0);

    pub fn set(alloc: MrsAllocFn, free: MrsFreeFn) {
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
            // `usize` by `set`.
            let alloc: MrsAllocFn = unsafe { core::mem::transmute(f) };
            unsafe { alloc(layout.size(), layout.align()) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            let f = FREE_FN.load(Ordering::Acquire);
            if f == 0 {
                return;
            }
            // SAFETY: see `alloc` above; `set` always sets both
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

/// Opaque owning handle to a loaded or built `Arena`. Must be released exactly once via
/// `mrs_arena_free`.
pub struct MrsArena(Arena<'static>);

/// Mirrors (a subset of) `ReadError` as a stable, C-representable code.
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

/// Mirrors `marshal_rs::Kind`, in declaration order.
///
/// Spelled out here (rather than exporting `Kind` itself) so the C ABI doesn't silently change when
/// the core crate grows a variant: the exhaustive `From<Kind>` below stops compiling instead.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MrsKind {
    Nil = 0,
    True = 1,
    False = 2,
    Fixnum = 3,
    Bignum = 4,
    Float = 5,
    /// Raw bytes with no declared text encoding, implicitly `ASCII-8BIT`.
    Bytes = 6,
    /// Text that carried a declared encoding; see `mrs_encoding_id`.
    Str = 7,
    Symbol = 8,
    Regexp = 9,
    Array = 10,
    Hash = 11,
    Struct = 12,
    Object = 13,
    Class = 14,
    Module = 15,
}

impl From<Kind> for MrsKind {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::Nil => Self::Nil,
            Kind::True => Self::True,
            Kind::False => Self::False,
            Kind::Fixnum => Self::Fixnum,
            Kind::Bignum => Self::Bignum,
            Kind::Float => Self::Float,
            Kind::Bytes => Self::Bytes,
            Kind::Str => Self::Str,
            Kind::Symbol => Self::Symbol,
            Kind::Regexp => Self::Regexp,
            Kind::Array => Self::Array,
            Kind::Hash => Self::Hash,
            Kind::Struct => Self::Struct,
            Kind::Object => Self::Object,
            Kind::Class => Self::Class,
            Kind::Module => Self::Module,
        }
    }
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

/// Loads a Marshal byte buffer. Returns `NULL` and sets `*out_error` (if non-null) on failure. Free
/// the result with `mrs_arena_free`.
///
/// # Safety
///
/// `buf` must point to at least `len` readable bytes. `out_error`, if non-null, must point
/// to a writable `MrsError`.
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

/// Releases an `MrsArena` handle. `NULL` is a no-op.
///
/// # Safety
///
/// `arena` must be a handle previously returned by `mrs_load` (or built via this module)
/// and not already freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mrs_arena_free(arena: *mut MrsArena) {
    if !arena.is_null() {
        drop(unsafe { Box::from_raw(arena) });
    }
}

/// Dumps `arena` back to Marshal bytes into a freshly heap-allocated buffer; `*out_len` receives
/// its length. Free the result with `mrs_buffer_free`.
///
/// # Safety
///
/// `arena` must be a valid handle. `out_len` must point to a writable `usize`.
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

/// Releases a buffer returned by `mrs_dump`. `len` must be the same value `mrs_dump` wrote to
/// `*out_len`.
///
/// # Safety
///
/// `buf`/`len` must be exactly the pointer/length pair returned together by a single
/// `mrs_dump` call, not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mrs_buffer_free(buf: *mut u8, len: usize) {
    if !buf.is_null() {
        drop(unsafe { Vec::from_raw_parts(buf, len, len) });
    }
}

/// The id of the arena's root value.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_root(arena: &MrsArena) -> MrsValueId {
    arena.0.root()
}

/// The kind of the value `id`.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_kind(arena: &MrsArena, id: MrsValueId) -> MrsKind {
    arena.0.node(id).kind.into()
}

fn value<'r>(arena: &'r MrsArena, id: MrsValueId) -> ValueRef<'r, 'static> {
    ValueRef::new(&arena.0, id)
}

/// Writes the value to `*out` and returns `true` if it is a `true`/`false`, else returns `false`.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_as_bool(arena: &MrsArena, id: MrsValueId, out: &mut bool) -> bool {
    match value(arena, id).as_bool() {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

/// Writes the value to `*out` and returns `true` if it is a fixnum (or a bignum that fits), else
/// returns `false`.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_as_i64(arena: &MrsArena, id: MrsValueId, out: &mut i64) -> bool {
    match value(arena, id).as_i64() {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

/// Writes the value to `*out` and returns `true` if it is a float, else returns `false`.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_as_f64(arena: &MrsArena, id: MrsValueId, out: &mut f64) -> bool {
    match value(arena, id).as_f64() {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

/// Borrowed pointer into `arena`'s storage - valid as long as `arena` is not freed. Works for
/// `Str`, `Bytes`, and `Symbol` values.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_as_bytes(arena: &MrsArena, id: MrsValueId, out_ptr: &mut *const u8, out_len: &mut usize) -> bool {
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

/// Borrowed pointer to the value's declared class/module name, if any.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_class_name(arena: &MrsArena, id: MrsValueId, out_ptr: &mut *const u8, out_len: &mut usize) -> bool {
    match value(arena, id).class_name() {
        Some(b) => {
            *out_ptr = b.as_ptr();
            *out_len = b.len();
            true
        }
        None => false,
    }
}

/// Borrowed pointer to a `Class`/`Module` value's raw path (Ruby stores these as plain strings, not
/// symbols) - distinct from `mrs_class_name`, which is the *declared class of* a value, not a
/// `Class`/`Module` value's own name.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_as_path(arena: &MrsArena, id: MrsValueId, out_ptr: &mut *const u8, out_len: &mut usize) -> bool {
    match value(arena, id).as_path() {
        Some(b) => {
            *out_ptr = b.as_ptr();
            *out_len = b.len();
            true
        }
        None => false,
    }
}

/// A `Regexp`'s source pattern (borrowed) and its option bits (`1`=ignorecase, `2`=extended,
/// `4`=multiline - combinable).
#[unsafe(no_mangle)]
pub extern "C" fn mrs_as_regexp(
    arena: &MrsArena,
    id: MrsValueId,
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

/// This crate never transcodes - a `Str`/`Bytes`/`Regexp` value's bytes are exactly what was on the
/// wire, tagged with a declared encoding id.
///
/// `MRS_ENCODING_ASCII_8BIT` (0) is Ruby's default; also what an untagged `Bytes` value implicitly
/// means even though no ivar was ever written for it. `MRS_ENCODING_CUSTOM` (255) means the name
/// isn't in the fixed table - look it up with `mrs_encoding_name` instead of hardcoding against the
/// id.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_encoding_id(arena: &MrsArena, id: MrsValueId, out: &mut u8) -> bool {
    match value(arena, id).encoding_id() {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

/// Borrowed pointer to the name behind `mrs_encoding_id` - resolved from the fixed table for a
/// known id, or from the arena's custom-encoding side table for `MRS_ENCODING_CUSTOM`.
///
/// Valid as long as `arena` is not freed.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_encoding_name(
    arena: &MrsArena,
    id: MrsValueId,
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

/// A `Bignum`'s value as a decimal string, in a freshly heap-allocated buffer - release it with
/// `mrs_buffer_free`, exactly like `mrs_dump`'s result.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_as_bignum_decimal(
    arena: &MrsArena,
    id: MrsValueId,
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

/// The number of elements of an `Array` value, `0` for any other kind.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_array_len(arena: &MrsArena, id: MrsValueId) -> u32 {
    value(arena, id).len() as u32
}

/// The element at `index` of an `Array`, or `MRS_VALUE_ID_NONE` when out of range.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_array_get(arena: &MrsArena, id: MrsValueId, index: u32) -> MrsValueId {
    value(arena, id).at(index as usize).map_or(MRS_VALUE_ID_NONE, |v| v.id())
}

/// The number of entries of a `Hash` value, `0` for any other kind.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_hash_len(arena: &MrsArena, id: MrsValueId) -> u32 {
    value(arena, id).len() as u32
}

/// The key of the entry at `index` of a `Hash`, or `MRS_VALUE_ID_NONE` when out of range.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_hash_key_at(arena: &MrsArena, id: MrsValueId, index: u32) -> MrsValueId {
    value(arena, id)
        .entries()
        .nth(index as usize)
        .map_or(MRS_VALUE_ID_NONE, |(k, _)| k.id())
}

/// The value of the entry at `index` of a `Hash`, or `MRS_VALUE_ID_NONE` when out of range.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_hash_value_at(arena: &MrsArena, id: MrsValueId, index: u32) -> MrsValueId {
    value(arena, id)
        .entries()
        .nth(index as usize)
        .map_or(MRS_VALUE_ID_NONE, |(_, v)| v.id())
}

/// The number of instance variables or members of a `Struct`/`Object` value, `0` for any other
/// kind.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_members_len(arena: &MrsArena, id: MrsValueId) -> u32 {
    value(arena, id).len() as u32
}

/// Borrowed pointer to the name of the member at `index` of a `Struct`/`Object`. Valid as long as
/// `arena` is not freed.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_member_name_at(
    arena: &MrsArena,
    id: MrsValueId,
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

/// The value of the member at `index` of a `Struct`/`Object`, or `MRS_VALUE_ID_NONE` when out of
/// range.
#[unsafe(no_mangle)]
pub extern "C" fn mrs_member_value_at(arena: &MrsArena, id: MrsValueId, index: u32) -> MrsValueId {
    value(arena, id)
        .members()
        .nth(index as usize)
        .map_or(MRS_VALUE_ID_NONE, |(_, v)| v.id())
}
