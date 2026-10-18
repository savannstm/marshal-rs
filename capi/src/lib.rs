#![cfg_attr(not(feature = "std"), no_std)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
// The `extern "C"` rules are in the crate docs below, not repeated per function.
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::too_long_first_doc_paragraph)]
// Lengths come from `u32` fields or are checked against `i32::MAX`, so truncating casts are safe.
#![allow(clippy::cast_possible_truncation, clippy::option_if_let_else)]
//! C bindings for `marshal-rs`. Pointers handed out borrow the arena and stay valid until `mrs_arena_free`.
//! An arena may be read from several threads at once while none of them changes it. Out-parameters may be
//! `NULL`. Running out of memory aborts the process.

extern crate alloc;

use alloc::{boxed::Box, rc::Rc, string::String, vec::Vec};
use core::slice;
use marshal_rs::{Arena, Kind, ReadError, ValueId, ValueRef};

/// The value `id` names - or, if `id` is not a node of the arena (a stale or foreign id), makes the
/// calling function return `$none` instead of indexing out of bounds.
macro_rules! val {
    ($arena:expr, $id:expr, $none:expr) => {{
        let arena: &MrsArena = $arena;
        if !arena.0.contains($id) {
            return $none;
        }
        ValueRef::new(&arena.0, $id)
    }};
}

/// An index into an `MrsArena`. `MRS_VALUE_ID_NONE` means "no value". A foreign or stale id is harmless:
/// `mrs_kind` returns `MRS_KIND_INVALID`.
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

/// `mrs_encoding_id` of `US-ASCII`.
pub const MRS_ENCODING_US_ASCII: u8 = 66;

/// `mrs_encoding_id` of `UTF-8`.
pub const MRS_ENCODING_UTF_8: u8 = 74;

/// `mrs_as_regexp` / `mrs_push_regexp` option bit: case-insensitive (`/i`).
pub const MRS_REGEXP_IGNORE_CASE: u8 = 1;

/// `mrs_as_regexp` / `mrs_push_regexp` option bit: extended (`/x`).
pub const MRS_REGEXP_EXTENDED: u8 = 2;

/// `mrs_as_regexp` / `mrs_push_regexp` option bit: multiline (`/m`).
pub const MRS_REGEXP_MULTILINE: u8 = 4;

// The C constants are spelled as literals for cbindgen, which can't see into the core crate.
const _: () = {
    assert!(MRS_ENCODING_ASCII_8BIT == marshal_rs::ENCODING_ASCII_8BIT);
    assert!(MRS_ENCODING_US_ASCII == marshal_rs::ENCODING_US_ASCII);
    assert!(MRS_ENCODING_UTF_8 == marshal_rs::ENCODING_UTF_8);
    assert!(MRS_ENCODING_CUSTOM == marshal_rs::ENCODING_CUSTOM);
    assert!(MRS_REGEXP_IGNORE_CASE == marshal_rs::wire::REGEXP_IGNORE_CASE);
    assert!(MRS_REGEXP_EXTENDED == marshal_rs::wire::REGEXP_EXTENDED);
    assert!(MRS_REGEXP_MULTILINE == marshal_rs::wire::REGEXP_MULTILINE);
};

/// Host allocation hook: at least `size` bytes aligned to `align`, or `NULL`. See `mrs_set_allocator`.
pub type MrsAllocFn = extern "C" fn(size: usize, align: usize) -> *mut u8;

/// Host deallocation hook: gets back exactly the `(ptr, size, align)` triple a prior `MrsAllocFn`
/// call produced. See `mrs_set_allocator`.
pub type MrsFreeFn = extern "C" fn(ptr: *mut u8, size: usize, align: usize);

/// Registers the host's allocator. Required before any other `mrs_*` call in a `no_std` build; a no-op with `std`.
///
/// # Safety
///
/// `alloc`/`free` must stay valid for the life of the process, be callable from any thread that uses this
/// library, and `free` must receive exactly the `(ptr, size, align)` that `alloc` returned.
#[no_mangle]
pub extern "C" fn mrs_set_allocator(alloc: MrsAllocFn, free: MrsFreeFn) {
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

    /// Only the first call takes effect: switching allocators once memory is outstanding would free
    /// it through the wrong one.
    pub fn set(alloc: MrsAllocFn, free: MrsFreeFn) {
        if ALLOC_FN
            .compare_exchange(0, alloc as usize, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            FREE_FN.store(free as usize, Ordering::Release);
        }
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
        extern "C" {
            fn abort() -> !;
        }
        unsafe { abort() }
    }
}

/// The single copy of the input that loaded arenas borrow their strings from (instead of one heap block per
/// string); shared by the arenas of one `mrs_load_many`.
struct InputCopy(*mut [u8]);

impl InputCopy {
    fn new(bytes: &[u8]) -> Rc<Self> {
        Rc::new(Self(Box::into_raw(Box::<[u8]>::from(bytes))))
    }

    /// The copy, for an arena to borrow. It is valid for as long as this `InputCopy` lives, which
    /// [`MrsArena`] guarantees by dropping its arena first.
    fn bytes(&self) -> &'static [u8] {
        // SAFETY: the pointer came from a `Box` that is only freed in `drop`, and nothing writes to it.
        unsafe { &*self.0 }
    }
}

impl Drop for InputCopy {
    fn drop(&mut self) {
        // SAFETY: made by `Box::into_raw` in `new`, freed here once.
        drop(unsafe { Box::from_raw(self.0) });
    }
}

/// Opaque owning handle to a loaded or built `Arena`. Must be released exactly once via
/// `mrs_arena_free`.
///
/// The fields drop in order, so the arena (which borrows from the input copy) is gone before the copy is.
#[allow(dead_code)] // the second field is held for its Drop: the arena borrows from it
pub struct MrsArena(Arena<'static>, Rc<InputCopy>);

impl MrsArena {
    fn built(arena: Arena<'static>) -> Self {
        Self(arena, InputCopy::new(&[]))
    }
}

/// Mirrors (a subset of) `ReadError` as a stable, C-representable code.
#[repr(u32)]
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
    TrailingData = 9,
    EmptyName = 10,
    NegativeLength = 11,
}

/// Mirrors `marshal_rs::Kind`, in declaration order.
///
/// Spelled out here (rather than exporting `Kind` itself) so the C ABI doesn't silently change when
/// the core crate grows a variant: the exhaustive `From<Kind>` below stops compiling instead.
#[repr(u32)]
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
    /// Returned by `mrs_kind` for an id that is not a node of the arena.
    Invalid = 255,
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
            ReadError::UnexpectedEof { .. } => Self::UnexpectedEof,
            ReadError::InvalidHeader => Self::InvalidHeader,
            ReadError::InvalidTag { .. } => Self::InvalidTag,
            ReadError::SymbolTableFull => Self::SymbolTableFull,
            ReadError::UnknownSymbolLink { .. } => Self::UnknownSymbolLink,
            ReadError::UnknownObjectLink { .. } => Self::UnknownObjectLink,
            ReadError::LengthOverflow => Self::LengthOverflow,
            ReadError::NegativeLength { .. } => Self::NegativeLength,
            ReadError::Unsupported(_) => Self::Unsupported,
            ReadError::TrailingData { .. } => Self::TrailingData,
            ReadError::EmptyName { .. } => Self::EmptyName,
        }
    }
}

/// Where and why a load failed; see `mrs_load_ex`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MrsLoadError {
    pub code: MrsError,
    /// Byte offset into the failing stream for `UnexpectedEof`, `InvalidTag`, `TrailingData`, `EmptyName` and
    /// `NegativeLength`, else `0`.
    pub offset: usize,
}

impl From<ReadError> for MrsLoadError {
    fn from(err: ReadError) -> Self {
        let offset = match err {
            ReadError::UnexpectedEof { offset }
            | ReadError::InvalidTag { offset, .. }
            | ReadError::TrailingData { offset }
            | ReadError::EmptyName { offset }
            | ReadError::NegativeLength { offset, .. } => offset,
            _ => 0,
        };
        Self {
            code: err.into(),
            offset,
        }
    }
}

const NO_ERROR: MrsLoadError = MrsLoadError {
    code: MrsError::Ok,
    offset: 0,
};

/// Writes `value` to the C out-parameter `out`, unless it is `NULL` (a caller that only wants to know whether
/// the value is of that kind passes `NULL`). A raw pointer, not a reference: C hands over uninitialized
/// memory, which a Rust reference may not point to.
unsafe fn put<T>(out: *mut T, value: T) {
    if !out.is_null() {
        unsafe { out.write(value) };
    }
}

/// Builds a byte slice from a C `(pointer, length)`. `from_raw_parts` needs a non-null pointer even for
/// an empty slice, and C callers pass `NULL, 0`.
const unsafe fn bytes_arg<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(ptr, len) }
    }
}

/// Loads a Marshal byte stream into a new `MrsArena`, or returns `NULL` and, if `out_error` is not
/// `NULL`, writes the reason there. Free the result with `mrs_arena_free`.
///
/// # Safety
///
/// `buf` must point to `len` readable bytes (it may be `NULL` if `len` is `0`). `out_error` must be
/// `NULL` or point to a writable `MrsError`.
#[no_mangle]
pub unsafe extern "C" fn mrs_load(buf: *const u8, len: usize, out_error: *mut MrsError) -> *mut MrsArena {
    let mut detail = NO_ERROR;
    let arena = mrs_load_ex(buf, len, &mut detail);
    if !out_error.is_null() {
        unsafe { *out_error = detail.code };
    }
    arena
}

/// `mrs_load`, reporting the error offset too.
///
/// # Safety
///
/// As `mrs_load`, with `out_error` `NULL` or pointing to a writable `MrsLoadError`.
#[no_mangle]
pub unsafe extern "C" fn mrs_load_ex(buf: *const u8, len: usize, out_error: *mut MrsLoadError) -> *mut MrsArena {
    let input = InputCopy::new(unsafe { bytes_arg(buf, len) });
    let result = marshal_rs::load(input.bytes());
    let (arena, detail) = match result {
        Ok(arena) => (Box::into_raw(Box::new(MrsArena(arena, input))), NO_ERROR),
        Err(err) => (core::ptr::null_mut(), err.into()),
    };
    if !out_error.is_null() {
        unsafe { *out_error = detail };
    }
    arena
}

/// The arenas `mrs_load_many` loaded, taken out one at a time.
pub struct MrsArenaList(Vec<Option<MrsArena>>);

/// Loads every Marshal stream concatenated in `buf` (an RPG Maker save file is several).
///
/// Returns `NULL` and writes the reason to `out_error` (if not `NULL`; its offset is within the failing stream).
/// Free the result with `mrs_arena_list_free`.
///
/// # Safety
///
/// As `mrs_load_ex`.
#[no_mangle]
pub unsafe extern "C" fn mrs_load_many(buf: *const u8, len: usize, out_error: *mut MrsLoadError) -> *mut MrsArenaList {
    let input = InputCopy::new(unsafe { bytes_arg(buf, len) });
    let result = marshal_rs::load_many(input.bytes());
    let (list, detail) = match result {
        Ok(arenas) => {
            let arenas = arenas.into_iter().map(|arena| Some(MrsArena(arena, Rc::clone(&input))));
            (Box::into_raw(Box::new(MrsArenaList(arenas.collect()))), NO_ERROR)
        }
        Err(err) => (core::ptr::null_mut(), err.into()),
    };
    if !out_error.is_null() {
        unsafe { *out_error = detail };
    }
    list
}

/// How many arenas the list holds.
#[no_mangle]
pub extern "C" fn mrs_arena_list_len(list: &MrsArenaList) -> usize {
    list.0.len()
}

/// Takes arena `index` out of the list; it is then yours to release with `mrs_arena_free`. `NULL` if
/// `index` is out of range or the arena was already taken.
#[no_mangle]
pub extern "C" fn mrs_arena_list_take(list: &mut MrsArenaList, index: usize) -> *mut MrsArena {
    list.0
        .get_mut(index)
        .and_then(Option::take)
        .map_or(core::ptr::null_mut(), |arena| Box::into_raw(Box::new(arena)))
}

/// Releases a list and every arena still in it. `NULL` is a no-op.
///
/// # Safety
///
/// `list` must come from `mrs_load_many` and not be freed already.
#[no_mangle]
pub unsafe extern "C" fn mrs_arena_list_free(list: *mut MrsArenaList) {
    if !list.is_null() {
        drop(unsafe { Box::from_raw(list) });
    }
}

/// Releases an `MrsArena` handle. `NULL` is a no-op.
///
/// # Safety
///
/// `arena` must be a handle previously returned by `mrs_load` (or built via this module)
/// and not already freed.
#[no_mangle]
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
#[no_mangle]
pub unsafe extern "C" fn mrs_dump(arena: *const MrsArena, out_len: *mut usize) -> *mut u8 {
    let arena = unsafe { &(*arena).0 };
    let (ptr, len) = leak_bytes(marshal_rs::dump(arena));
    unsafe { put(out_len, len) };
    ptr
}

/// Hands `bytes` to C as `(ptr, len)`, released by `mrs_buffer_free(ptr, len)`.
///
/// Goes through a boxed slice, the one conversion guaranteeing capacity == length, which `mrs_buffer_free`
/// relies on to rebuild the allocation with the layout it was made with.
fn leak_bytes(bytes: Vec<u8>) -> (*mut u8, usize) {
    let boxed = bytes.into_boxed_slice();
    let len = boxed.len();
    (Box::into_raw(boxed).cast::<u8>(), len)
}

/// Releases a buffer returned by `mrs_dump`. `len` must be the same value `mrs_dump` wrote to
/// `*out_len`.
///
/// # Safety
///
/// `buf`/`len` must be exactly the pointer/length pair returned together by a single
/// `mrs_dump` call, not yet freed.
#[no_mangle]
pub extern "C" fn mrs_buffer_free(buf: *mut u8, len: usize) {
    if !buf.is_null() {
        drop(unsafe { Box::from_raw(core::ptr::slice_from_raw_parts_mut(buf, len)) });
    }
}

/// The id of the arena's root value.
#[no_mangle]
pub const extern "C" fn mrs_root(arena: &MrsArena) -> MrsValueId {
    arena.0.root()
}

/// The kind of the value `id`.
#[no_mangle]
pub extern "C" fn mrs_kind(arena: &MrsArena, id: MrsValueId) -> MrsKind {
    val!(arena, id, MrsKind::Invalid).kind().into()
}

/// Writes the value to `*out` and returns `true` if it is a `true`/`false`, else returns `false`.
#[no_mangle]
pub unsafe extern "C" fn mrs_as_bool(arena: &MrsArena, id: MrsValueId, out: *mut bool) -> bool {
    match val!(arena, id, false).as_bool() {
        Some(v) => {
            put(out, v);
            true
        }
        None => false,
    }
}

/// Writes the value to `*out` and returns `true` if it is a fixnum (or a bignum that fits), else
/// returns `false`.
#[no_mangle]
pub unsafe extern "C" fn mrs_as_i64(arena: &MrsArena, id: MrsValueId, out: *mut i64) -> bool {
    match val!(arena, id, false).as_i64() {
        Some(v) => {
            put(out, v);
            true
        }
        None => false,
    }
}

/// Writes the value to `*out` and returns `true` if it is a float, else returns `false`.
#[no_mangle]
pub unsafe extern "C" fn mrs_as_f64(arena: &MrsArena, id: MrsValueId, out: *mut f64) -> bool {
    match val!(arena, id, false).as_f64() {
        Some(v) => {
            put(out, v);
            true
        }
        None => false,
    }
}

/// Borrowed pointer into `arena`'s storage - valid as long as `arena` is not freed. Works for
/// `Str`, `Bytes`, and `Symbol` values.
#[no_mangle]
pub unsafe extern "C" fn mrs_as_bytes(
    arena: &MrsArena,
    id: MrsValueId,
    out_ptr: *mut *const u8,
    out_len: *mut usize,
) -> bool {
    let v = val!(arena, id, false);
    let bytes = v.as_bytes().or_else(|| v.as_symbol_bytes());
    match bytes {
        Some(b) => {
            put(out_ptr, b.as_ptr());
            put(out_len, b.len());
            true
        }
        None => false,
    }
}

/// Borrowed pointer to the value's declared class/module name, if any.
#[no_mangle]
pub unsafe extern "C" fn mrs_class_name(
    arena: &MrsArena,
    id: MrsValueId,
    out_ptr: *mut *const u8,
    out_len: *mut usize,
) -> bool {
    match val!(arena, id, false).class_name() {
        Some(b) => {
            put(out_ptr, b.as_ptr());
            put(out_len, b.len());
            true
        }
        None => false,
    }
}

/// Borrowed pointer to a `Class`/`Module` value's path. Distinct from `mrs_class_name`, which is the value's declared class.
#[no_mangle]
pub unsafe extern "C" fn mrs_as_path(
    arena: &MrsArena,
    id: MrsValueId,
    out_ptr: *mut *const u8,
    out_len: *mut usize,
) -> bool {
    match val!(arena, id, false).as_path() {
        Some(b) => {
            put(out_ptr, b.as_ptr());
            put(out_len, b.len());
            true
        }
        None => false,
    }
}

/// A `Regexp`'s source pattern (borrowed) and its option bits (`1`=ignorecase, `2`=extended,
/// `4`=multiline - combinable).
#[no_mangle]
pub unsafe extern "C" fn mrs_as_regexp(
    arena: &MrsArena,
    id: MrsValueId,
    out_ptr: *mut *const u8,
    out_len: *mut usize,
    out_options: *mut u8,
) -> bool {
    match val!(arena, id, false).as_regexp() {
        Some((source, options)) => {
            put(out_ptr, source.as_ptr());
            put(out_len, source.len());
            put(out_options, options);
            true
        }
        None => false,
    }
}

/// Encoding ids. Bytes are never transcoded. `MRS_ENCODING_ASCII_8BIT` (0) is Ruby's default;
/// `MRS_ENCODING_CUSTOM` (255) means the name is not in the table, so use `mrs_encoding_name`.
#[no_mangle]
pub unsafe extern "C" fn mrs_encoding_id(arena: &MrsArena, id: MrsValueId, out: *mut u8) -> bool {
    match val!(arena, id, false).encoding_id() {
        Some(v) => {
            put(out, v);
            true
        }
        None => false,
    }
}

/// Borrowed pointer to the name behind `mrs_encoding_id`. Valid until `arena` is freed.
#[no_mangle]
pub unsafe extern "C" fn mrs_encoding_name(
    arena: &MrsArena,
    id: MrsValueId,
    out_ptr: *mut *const u8,
    out_len: *mut usize,
) -> bool {
    match val!(arena, id, false).encoding_name() {
        Some(b) => {
            put(out_ptr, b.as_ptr());
            put(out_len, b.len());
            true
        }
        None => false,
    }
}

/// A `Bignum`'s value as a decimal string, in a freshly heap-allocated buffer - release it with
/// `mrs_buffer_free`, exactly like `mrs_dump`'s result.
#[no_mangle]
pub unsafe extern "C" fn mrs_as_bignum_decimal(
    arena: &MrsArena,
    id: MrsValueId,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> bool {
    match val!(arena, id, false).as_bigint_decimal() {
        Some(text) => {
            // With nowhere to put the buffer there is nothing to leak, and nothing to free.
            if !out_ptr.is_null() {
                let (ptr, len) = leak_bytes(String::into_bytes(text));
                put(out_ptr, ptr);
                put(out_len, len);
            }
            true
        }
        None => false,
    }
}

/// A `Bignum`'s sign and little-endian magnitude bytes, borrowed from the arena. Unlike
/// `mrs_as_bignum_decimal` there is no size limit. Returns `false` for any other kind.
#[no_mangle]
pub unsafe extern "C" fn mrs_as_bignum_bytes(
    arena: &MrsArena,
    id: MrsValueId,
    out_negative: *mut bool,
    out_ptr: *mut *const u8,
    out_len: *mut usize,
) -> bool {
    match val!(arena, id, false).as_bignum_bytes() {
        Some((negative, magnitude)) => {
            put(out_negative, negative);
            put(out_ptr, magnitude.as_ptr());
            put(out_len, magnitude.len());
            true
        }
        None => false,
    }
}

/// A `Hash`'s default value, or `MRS_VALUE_ID_NONE` if it is not a hash or has none.
#[no_mangle]
pub extern "C" fn mrs_hash_default(arena: &MrsArena, id: MrsValueId) -> MrsValueId {
    val!(arena, id, MRS_VALUE_ID_NONE)
        .default_value()
        .map_or(MRS_VALUE_ID_NONE, |v| v.id())
}

/// The number of elements of an `Array` value, `0` for any other kind.
#[no_mangle]
pub extern "C" fn mrs_array_len(arena: &MrsArena, id: MrsValueId) -> u32 {
    val!(arena, id, 0).len() as u32
}

/// The element at `index` of an `Array`, or `MRS_VALUE_ID_NONE` when out of range.
#[no_mangle]
pub extern "C" fn mrs_array_get(arena: &MrsArena, id: MrsValueId, index: u32) -> MrsValueId {
    val!(arena, id, MRS_VALUE_ID_NONE)
        .at(index as usize)
        .map_or(MRS_VALUE_ID_NONE, |v| v.id())
}

/// The number of entries of a `Hash` value, `0` for any other kind.
#[no_mangle]
pub extern "C" fn mrs_hash_len(arena: &MrsArena, id: MrsValueId) -> u32 {
    val!(arena, id, 0).len() as u32
}

/// The key of the entry at `index` of a `Hash`, or `MRS_VALUE_ID_NONE` when out of range.
#[no_mangle]
pub extern "C" fn mrs_hash_key_at(arena: &MrsArena, id: MrsValueId, index: u32) -> MrsValueId {
    val!(arena, id, MRS_VALUE_ID_NONE)
        .entries()
        .nth(index as usize)
        .map_or(MRS_VALUE_ID_NONE, |(k, _)| k.id())
}

/// The value of the entry at `index` of a `Hash`, or `MRS_VALUE_ID_NONE` when out of range.
#[no_mangle]
pub extern "C" fn mrs_hash_value_at(arena: &MrsArena, id: MrsValueId, index: u32) -> MrsValueId {
    val!(arena, id, MRS_VALUE_ID_NONE)
        .entries()
        .nth(index as usize)
        .map_or(MRS_VALUE_ID_NONE, |(_, v)| v.id())
}

/// The number of instance variables or members of a `Struct`/`Object` value, `0` for any other
/// kind.
#[no_mangle]
pub extern "C" fn mrs_members_len(arena: &MrsArena, id: MrsValueId) -> u32 {
    val!(arena, id, 0).len() as u32
}

/// Borrowed pointer to the name of the member at `index` of a `Struct`/`Object`. Valid as long as
/// `arena` is not freed.
#[no_mangle]
pub unsafe extern "C" fn mrs_member_name_at(
    arena: &MrsArena,
    id: MrsValueId,
    index: u32,
    out_ptr: *mut *const u8,
    out_len: *mut usize,
) -> bool {
    match val!(arena, id, false).members().nth(index as usize) {
        Some((name, _)) => {
            put(out_ptr, name.as_ptr());
            put(out_len, name.len());
            true
        }
        None => false,
    }
}

/// The value of the member at `index` of a `Struct`/`Object`, or `MRS_VALUE_ID_NONE` when out of
/// range.
#[no_mangle]
pub extern "C" fn mrs_member_value_at(arena: &MrsArena, id: MrsValueId, index: u32) -> MrsValueId {
    val!(arena, id, MRS_VALUE_ID_NONE)
        .members()
        .nth(index as usize)
        .map_or(MRS_VALUE_ID_NONE, |(_, v)| v.id())
}

/// `mrs_flags` bit: an instance of a subclass of a built-in type (`TYPE_UCLASS`).
pub const MRS_FLAG_USER_CLASS: u8 = 1;
/// `mrs_flags` bit: a `_dump_data` wrapper; `mrs_body` is the value it returned.
pub const MRS_FLAG_DATA: u8 = 2;
/// `mrs_flags` bit: a `marshal_dump` wrapper; `mrs_body` is the value it returned.
pub const MRS_FLAG_USER_MARSHAL: u8 = 4;
/// `mrs_flags` bit: a `_dump` string (`TYPE_USERDEF`).
pub const MRS_FLAG_USER_DEFINED: u8 = 8;
/// `mrs_flags` bit: a module written with the legacy `TYPE_MODULE_OLD` tag.
pub const MRS_FLAG_OLD_MODULE: u8 = 16;

/// How a value was wrapped in the stream, as a mask of `MRS_FLAG_*`: an instance of a built-in
/// type's subclass, `_dump_data`, `marshal_dump`, `_dump`, or a module written with the legacy tag.
#[no_mangle]
pub extern "C" fn mrs_flags(arena: &MrsArena, id: MrsValueId) -> u8 {
    let v = val!(arena, id, 0);
    (u8::from(v.is_user_class()) * MRS_FLAG_USER_CLASS)
        | (u8::from(v.is_data()) * MRS_FLAG_DATA)
        | (u8::from(v.is_user_marshal()) * MRS_FLAG_USER_MARSHAL)
        | (u8::from(v.is_user_defined()) * MRS_FLAG_USER_DEFINED)
        | (u8::from(v.is_old_module()) * MRS_FLAG_OLD_MODULE)
}

/// The number of modules the value was `extend`ed with.
#[no_mangle]
pub extern "C" fn mrs_extension_count(arena: &MrsArena, id: MrsValueId) -> u32 {
    val!(arena, id, 0).extensions().count() as u32
}

/// Name of the module at `index` among the value's extensions.
#[no_mangle]
pub unsafe extern "C" fn mrs_extension_at(
    arena: &MrsArena,
    id: MrsValueId,
    index: u32,
    out_ptr: *mut *const u8,
    out_len: *mut usize,
) -> bool {
    match val!(arena, id, false).extensions().nth(index as usize) {
        Some(name) => {
            put(out_ptr, name.as_ptr());
            put(out_len, name.len());
            true
        }
        None => false,
    }
}

/// The number of instance variables the value carried besides its encoding (rare).
#[no_mangle]
pub extern "C" fn mrs_extra_ivar_count(arena: &MrsArena, id: MrsValueId) -> u32 {
    val!(arena, id, 0).extra_ivars().count() as u32
}

/// Name of the extra instance variable at `index`.
#[no_mangle]
pub unsafe extern "C" fn mrs_extra_ivar_name_at(
    arena: &MrsArena,
    id: MrsValueId,
    index: u32,
    out_ptr: *mut *const u8,
    out_len: *mut usize,
) -> bool {
    match val!(arena, id, false).extra_ivars().nth(index as usize) {
        Some((name, _)) => {
            put(out_ptr, name.as_ptr());
            put(out_len, name.len());
            true
        }
        None => false,
    }
}

/// Value of the extra instance variable at `index`, or `MRS_VALUE_ID_NONE`.
#[no_mangle]
pub extern "C" fn mrs_extra_ivar_value_at(arena: &MrsArena, id: MrsValueId, index: u32) -> MrsValueId {
    val!(arena, id, MRS_VALUE_ID_NONE)
        .extra_ivars()
        .nth(index as usize)
        .map_or(MRS_VALUE_ID_NONE, |(_, v)| v.id())
}

/// The value of the `Hash` entry whose key is the symbol `name`, or `MRS_VALUE_ID_NONE`. A linear
/// scan of the hash: for many lookups on a big hash, walk it once with `mrs_hash_key_at` instead.
///
/// # Safety
///
/// `name` must point to `name_len` readable bytes (it may be `NULL` if `name_len` is `0`).
#[no_mangle]
pub unsafe extern "C" fn mrs_hash_get_symbol(
    arena: &MrsArena,
    id: MrsValueId,
    name: *const u8,
    name_len: usize,
) -> MrsValueId {
    let wanted = unsafe { bytes_arg(name, name_len) };
    val!(arena, id, MRS_VALUE_ID_NONE)
        .entries()
        .find(|(key, _)| key.as_symbol_bytes() == Some(wanted))
        .map_or(MRS_VALUE_ID_NONE, |(_, v)| v.id())
}

/// The value of the `Object`/`Struct` member called `name` (an ivar may be given with or without its
/// `@`), or `MRS_VALUE_ID_NONE`.
///
/// # Safety
///
/// As `mrs_hash_get_symbol`.
#[no_mangle]
pub unsafe extern "C" fn mrs_member_get(
    arena: &MrsArena,
    id: MrsValueId,
    name: *const u8,
    name_len: usize,
) -> MrsValueId {
    let wanted = unsafe { bytes_arg(name, name_len) };
    val!(arena, id, MRS_VALUE_ID_NONE)
        .members()
        .find(|(member, _)| *member == wanted || member.strip_prefix(b"@") == Some(wanted))
        .map_or(MRS_VALUE_ID_NONE, |(_, v)| v.id())
}

/// The text of a `Float` as Ruby wrote it (`"1.5"`, `"inf"`, ...), borrowed from the arena.
#[no_mangle]
pub unsafe extern "C" fn mrs_as_float_text(
    arena: &MrsArena,
    id: MrsValueId,
    out_ptr: *mut *const u8,
    out_len: *mut usize,
) -> bool {
    match val!(arena, id, false).as_float_bytes() {
        Some(text) => {
            put(out_ptr, text.as_ptr());
            put(out_len, text.len());
            true
        }
        None => false,
    }
}

/// The body of a `marshal_dump`/`_dump_data` wrapper (`MRS_FLAG_DATA` or `MRS_FLAG_USER_MARSHAL` in
/// `mrs_flags`).
///
/// The value those returned, with a class and `extend`s of its own. `MRS_VALUE_ID_NONE` for any
/// other value. Everything else (`mrs_kind`, `mrs_array_get`, ...) already reads through a wrapper to its
/// body, while `mrs_class_name` names the wrapper's own class.
#[no_mangle]
pub extern "C" fn mrs_body(arena: &MrsArena, id: MrsValueId) -> MrsValueId {
    val!(arena, id, MRS_VALUE_ID_NONE)
        .body()
        .map_or(MRS_VALUE_ID_NONE, |v| v.id())
}

/// The version of this library, as `"major.minor.patch"`. A program that loads the library at run time can
/// check it against the header it was built with.
#[no_mangle]
pub const extern "C" fn mrs_version() -> *const core::ffi::c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

// -- building and editing: every function refuses what does not fit and changes nothing --

/// Marshal stores a length as a packed `i32`.
const MAX_LEN: usize = i32::MAX as usize;

/// A copy of `len` bytes at `ptr`, `None` if `len` is more than Marshal can store.
unsafe fn bytes_vec(ptr: *const u8, len: usize) -> Option<Vec<u8>> {
    (len <= MAX_LEN).then(|| unsafe { bytes_arg(ptr, len) }.to_vec())
}

/// `count` items at `ptr`, `None` if `count` is more than Marshal can store.
const unsafe fn items_arg<'a, T>(ptr: *const T, count: usize) -> Option<&'a [T]> {
    match count {
        0 => Some(&[]),
        _ if count > MAX_LEN => None,
        _ => Some(unsafe { slice::from_raw_parts(ptr, count) }),
    }
}

/// An object or struct member: a name and the value it holds.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MrsMember {
    pub name: *const u8,
    pub name_len: usize,
    pub value: MrsValueId,
}

unsafe fn members_arg(ptr: *const MrsMember, count: usize) -> Option<Vec<(Vec<u8>, ValueId)>> {
    unsafe { items_arg(ptr, count) }?
        .iter()
        .map(|member| Some((unsafe { bytes_vec(member.name, member.name_len) }?, member.value)))
        .collect()
}

/// A new arena holding only `nil` (also its root). Free it with `mrs_arena_free`.
#[no_mangle]
pub extern "C" fn mrs_arena_new() -> *mut MrsArena {
    Box::into_raw(Box::new(MrsArena::built(Arena::builder())))
}

/// Makes `id` the value `mrs_dump` writes. `false` if `id` is not a node of the arena.
#[no_mangle]
pub extern "C" fn mrs_set_root(arena: &mut MrsArena, id: MrsValueId) -> bool {
    arena.0.set_root(id)
}

#[no_mangle]
pub extern "C" fn mrs_push_nil(arena: &mut MrsArena) -> MrsValueId {
    arena.0.push_nil()
}

#[no_mangle]
pub extern "C" fn mrs_push_bool(arena: &mut MrsArena, value: bool) -> MrsValueId {
    arena.0.push_bool(value)
}

#[no_mangle]
pub extern "C" fn mrs_push_fixnum(arena: &mut MrsArena, value: i32) -> MrsValueId {
    arena.0.push_fixnum(value)
}

/// Pushes `len` bytes as a binary string (`ASCII-8BIT`). `MRS_VALUE_ID_NONE` if `len` is above `i32::MAX`.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_bytes(arena: &mut MrsArena, bytes: *const u8, len: usize) -> MrsValueId {
    unsafe { bytes_vec(bytes, len) }.map_or(MRS_VALUE_ID_NONE, |bytes| arena.0.push_bytes(bytes))
}

/// Pushes `len` bytes of UTF-8 text as a string. `MRS_VALUE_ID_NONE` if they are not valid UTF-8 or
/// `len` is above `i32::MAX`.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_string(arena: &mut MrsArena, text: *const u8, len: usize) -> MrsValueId {
    unsafe { bytes_vec(text, len) }
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map_or(MRS_VALUE_ID_NONE, |text| arena.0.push_string(text))
}

/// Pushes a string of `len` bytes in the encoding called `encoding` (for example `"Shift_JIS"`); the bytes
/// are stored as they are. `MRS_VALUE_ID_NONE` if either length is above `i32::MAX`.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_string_with_encoding(
    arena: &mut MrsArena,
    bytes: *const u8,
    len: usize,
    encoding: *const u8,
    encoding_len: usize,
) -> MrsValueId {
    match (unsafe { bytes_vec(bytes, len) }, unsafe {
        bytes_vec(encoding, encoding_len)
    }) {
        (Some(bytes), Some(encoding)) if !encoding.is_empty() => arena.0.push_str_with_encoding_name(bytes, &encoding),
        _ => MRS_VALUE_ID_NONE,
    }
}

/// Pushes a symbol. It is tagged UTF-8 if its bytes are non-ASCII UTF-8, and has no tag (as Ruby writes an
/// ASCII or a binary symbol) otherwise; use `mrs_push_symbol_with_encoding` to say which.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_symbol(arena: &mut MrsArena, name: *const u8, len: usize) -> MrsValueId {
    unsafe { bytes_vec(name, len) }.map_or(MRS_VALUE_ID_NONE, |name| arena.0.push_symbol(name))
}

/// Pushes a symbol tagged with the encoding called `encoding` (for example `"Shift_JIS"`; `"ASCII-8BIT"` is no
/// tag at all); the bytes are stored as they are. `MRS_VALUE_ID_NONE` if either length is above `i32::MAX`.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_symbol_with_encoding(
    arena: &mut MrsArena,
    name: *const u8,
    len: usize,
    encoding: *const u8,
    encoding_len: usize,
) -> MrsValueId {
    match (unsafe { bytes_vec(name, len) }, unsafe {
        bytes_vec(encoding, encoding_len)
    }) {
        (Some(name), Some(encoding)) if !encoding.is_empty() => arena.0.push_symbol_with_encoding_name(name, &encoding),
        _ => MRS_VALUE_ID_NONE,
    }
}

/// Pushes a float from its text as Ruby writes it (`"1.5"`, `"inf"`, `"nan"`, or the text of Ruby before 1.9
/// with its NUL and mantissa bytes), stored as it is. `MRS_VALUE_ID_NONE` if `len` is above `i32::MAX`.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_float_text(arena: &mut MrsArena, text: *const u8, len: usize) -> MrsValueId {
    unsafe { bytes_vec(text, len) }.map_or(MRS_VALUE_ID_NONE, |text| arena.0.push_float(text))
}

/// Pushes a bignum from its sign and little-endian magnitude bytes.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_bignum(
    arena: &mut MrsArena,
    negative: bool,
    magnitude: *const u8,
    len: usize,
) -> MrsValueId {
    unsafe { bytes_vec(magnitude, len) }.map_or(MRS_VALUE_ID_NONE, |bytes| arena.0.push_bignum(negative, bytes))
}

/// Pushes a regexp. `encoding` names its declared encoding; pass `NULL, 0` for none.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_regexp(
    arena: &mut MrsArena,
    source: *const u8,
    len: usize,
    options: u8,
    encoding: *const u8,
    encoding_len: usize,
) -> MrsValueId {
    match (unsafe { bytes_vec(source, len) }, unsafe {
        bytes_vec(encoding, encoding_len)
    }) {
        (Some(source), Some(encoding)) if encoding.is_empty() => arena.0.push_regexp(source, options),
        (Some(source), Some(encoding)) => arena.0.push_regexp_with_encoding_name(source, options, &encoding),
        _ => MRS_VALUE_ID_NONE,
    }
}

/// Pushes a class reference (`Foo::Bar`). `MRS_VALUE_ID_NONE` if `path` is empty.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_class(arena: &mut MrsArena, path: *const u8, len: usize) -> MrsValueId {
    unsafe { bytes_vec(path, len) }
        .and_then(|path| arena.0.push_class(path))
        .unwrap_or(MRS_VALUE_ID_NONE)
}

/// Pushes a module reference; `old` writes it with the legacy tag. `MRS_VALUE_ID_NONE` if `path` is empty.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_module(arena: &mut MrsArena, path: *const u8, len: usize, old: bool) -> MrsValueId {
    unsafe { bytes_vec(path, len) }
        .and_then(|path| arena.0.push_module(path, old))
        .unwrap_or(MRS_VALUE_ID_NONE)
}

/// Pushes an array of the `count` values in `items`. `MRS_VALUE_ID_NONE` if any is not a node of the arena.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_array(arena: &mut MrsArena, items: *const MrsValueId, count: usize) -> MrsValueId {
    unsafe { items_arg(items, count) }
        .and_then(|items| arena.0.push_array(items))
        .unwrap_or(MRS_VALUE_ID_NONE)
}

/// Pushes a hash of `count` entries (`keys[i] => values[i]`). `default_value` is the hash's default, or
/// `MRS_VALUE_ID_NONE` for none. `MRS_VALUE_ID_NONE` if any id is not a node of the arena.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_hash(
    arena: &mut MrsArena,
    keys: *const MrsValueId,
    values: *const MrsValueId,
    count: usize,
    default_value: MrsValueId,
) -> MrsValueId {
    let (Some(keys), Some(values)) = (unsafe { items_arg(keys, count) }, unsafe { items_arg(values, count) }) else {
        return MRS_VALUE_ID_NONE;
    };
    let pairs: Vec<(ValueId, ValueId)> = keys.iter().copied().zip(values.iter().copied()).collect();
    let default = (default_value != MRS_VALUE_ID_NONE).then_some(default_value);
    arena.0.push_hash(&pairs, default).unwrap_or(MRS_VALUE_ID_NONE)
}

/// Pushes an object of class `class` with the instance variables `members` (names with their `@`).
/// `MRS_VALUE_ID_NONE` if the class is empty or any value is not a node of the arena.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_object(
    arena: &mut MrsArena,
    class: *const u8,
    class_len: usize,
    members: *const MrsMember,
    count: usize,
) -> MrsValueId {
    match (unsafe { bytes_vec(class, class_len) }, unsafe {
        members_arg(members, count)
    }) {
        (Some(class), Some(members)) => arena.0.push_object(class, &members).unwrap_or(MRS_VALUE_ID_NONE),
        _ => MRS_VALUE_ID_NONE,
    }
}

/// Pushes a `Struct` instance of class `class` with the given members; see `mrs_push_object`.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_struct(
    arena: &mut MrsArena,
    class: *const u8,
    class_len: usize,
    members: *const MrsMember,
    count: usize,
) -> MrsValueId {
    match (unsafe { bytes_vec(class, class_len) }, unsafe {
        members_arg(members, count)
    }) {
        (Some(class), Some(members)) => arena.0.push_struct(class, &members).unwrap_or(MRS_VALUE_ID_NONE),
        _ => MRS_VALUE_ID_NONE,
    }
}

/// Pushes a `marshal_dump` wrapper of class `class` around `body`. `MRS_VALUE_ID_NONE` if the class is empty
/// or `body` is not a node of the arena.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_user_marshal(
    arena: &mut MrsArena,
    class: *const u8,
    class_len: usize,
    body: MrsValueId,
) -> MrsValueId {
    unsafe { bytes_vec(class, class_len) }
        .and_then(|class| arena.0.push_user_marshal(class, body))
        .unwrap_or(MRS_VALUE_ID_NONE)
}

/// Pushes a `_dump_data` wrapper; see `mrs_push_user_marshal`.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_data(
    arena: &mut MrsArena,
    class: *const u8,
    class_len: usize,
    body: MrsValueId,
) -> MrsValueId {
    unsafe { bytes_vec(class, class_len) }
        .and_then(|class| arena.0.push_data(class, body))
        .unwrap_or(MRS_VALUE_ID_NONE)
}

/// Makes `id` an instance of the subclass `class` of its built-in type. `false` unless `id` is a string,
/// regexp, array or hash that is not a `_dump` string.
#[no_mangle]
pub unsafe extern "C" fn mrs_set_user_class(
    arena: &mut MrsArena,
    id: MrsValueId,
    class: *const u8,
    len: usize,
) -> bool {
    unsafe { bytes_vec(class, len) }.is_some_and(|class| arena.0.set_user_class(id, class))
}

/// Undoes `mrs_set_user_class`; `false` if `id` has none.
#[no_mangle]
pub extern "C" fn mrs_clear_user_class(arena: &mut MrsArena, id: MrsValueId) -> bool {
    arena.0.clear_user_class(id)
}

/// Makes the bytes or string `id` the `_dump` result of `class`. `false` for any other kind.
#[no_mangle]
pub unsafe extern "C" fn mrs_set_user_defined(
    arena: &mut MrsArena,
    id: MrsValueId,
    class: *const u8,
    len: usize,
) -> bool {
    unsafe { bytes_vec(class, len) }.is_some_and(|class| arena.0.set_user_defined(id, class))
}

/// Records that `id` was `extend`ed with `module`, after any it already has. `false` for `nil`, a boolean, a
/// fixnum or a symbol, or an empty name.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_extension(
    arena: &mut MrsArena,
    id: MrsValueId,
    module: *const u8,
    len: usize,
) -> bool {
    unsafe { bytes_vec(module, len) }.is_some_and(|module| arena.0.push_extension(id, module))
}

/// Gives `id` an instance variable `name` (with its `@`) holding `value`, besides its encoding. `false` for
/// `nil`, a boolean, a fixnum or a symbol, an empty name, or a `value` that is not a node of the arena.
#[no_mangle]
pub unsafe extern "C" fn mrs_push_extra_ivar(
    arena: &mut MrsArena,
    id: MrsValueId,
    name: *const u8,
    name_len: usize,
    value: MrsValueId,
) -> bool {
    unsafe { bytes_vec(name, name_len) }.is_some_and(|name| arena.0.push_extra_ivar(id, &name, value))
}

/// Points element `index` of the array `id` at `new_value`. `false` if `id` is not an array, `index` is
/// out of range or `new_value` is not a node of the arena.
#[no_mangle]
pub extern "C" fn mrs_set_array_value(arena: &mut MrsArena, id: MrsValueId, index: u32, new_value: MrsValueId) -> bool {
    arena.0.set_array_value(id, index as usize, new_value).is_some()
}

/// Points the key of entry `index` of the hash `id` at `new_key`; `false` as `mrs_set_array_value`.
#[no_mangle]
pub extern "C" fn mrs_set_hash_key(arena: &mut MrsArena, id: MrsValueId, index: u32, new_key: MrsValueId) -> bool {
    arena.0.set_hash_key(id, index as usize, new_key).is_some()
}

/// Points the value of entry `index` of the hash `id` at `new_value`; `false` as `mrs_set_array_value`.
#[no_mangle]
pub extern "C" fn mrs_set_hash_value(arena: &mut MrsArena, id: MrsValueId, index: u32, new_value: MrsValueId) -> bool {
    arena.0.set_hash_value(id, index as usize, new_value).is_some()
}

/// Repoints the default value of the hash `id` at `new_default`. `false` if `id` has no default to replace:
/// this never adds one, so build a hash with a default through `mrs_push_hash`.
#[no_mangle]
pub extern "C" fn mrs_set_hash_default(arena: &mut MrsArena, id: MrsValueId, new_default: MrsValueId) -> bool {
    arena.0.set_hash_default(id, new_default).is_some()
}

/// Points the member `name` of the object or struct `id` at `new_value`; never adds a member. `false` if there
/// is no such member or `new_value` is not a node of the arena.
#[no_mangle]
pub unsafe extern "C" fn mrs_set_member_value(
    arena: &mut MrsArena,
    id: MrsValueId,
    name: *const u8,
    name_len: usize,
    new_value: MrsValueId,
) -> bool {
    unsafe { bytes_vec(name, name_len) }.is_some_and(|name| arena.0.set_member_value(id, &name, new_value).is_some())
}

/// Overwrites the fixnum `id` in place (every reference to it sees the change). `false` if it is not a fixnum.
#[no_mangle]
pub extern "C" fn mrs_set_fixnum_value(arena: &mut MrsArena, id: MrsValueId, value: i32) -> bool {
    arena.0.set_fixnum_value(id, value).is_some()
}

/// Overwrites the text of the string `id` in place and tags it UTF-8, whatever encoding it had. `false` if `id`
/// is not a string, `text` is not valid UTF-8, or `len` is above `i32::MAX`.
#[no_mangle]
pub unsafe extern "C" fn mrs_set_string_text(
    arena: &mut MrsArena,
    id: MrsValueId,
    text: *const u8,
    len: usize,
) -> bool {
    unsafe { bytes_vec(text, len) }
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .is_some_and(|text| arena.0.set_string_text(id, text).is_some())
}

/// Overwrites the bytes of the string `id` in place, keeping its encoding tag: `bytes` are taken to be in that
/// encoding, and are not checked. `false` if `id` is not a string with a declared encoding, or `len` is above
/// `i32::MAX`.
#[no_mangle]
pub unsafe extern "C" fn mrs_set_string_content(
    arena: &mut MrsArena,
    id: MrsValueId,
    bytes: *const u8,
    len: usize,
) -> bool {
    unsafe { bytes_vec(bytes, len) }.is_some_and(|bytes| arena.0.set_str_content(id, bytes).is_some())
}

/// Overwrites the bytes of the `Bytes` value `id` (raw bytes with no declared encoding) in place. `false` if `id`
/// is not one, or `len` is above `i32::MAX`.
#[no_mangle]
pub unsafe extern "C" fn mrs_set_bytes_content(
    arena: &mut MrsArena,
    id: MrsValueId,
    bytes: *const u8,
    len: usize,
) -> bool {
    unsafe { bytes_vec(bytes, len) }.is_some_and(|bytes| arena.0.set_bytes_content(id, bytes).is_some())
}

/// Overwrites the bytes of the string `id` in place and tags them with the encoding called `encoding` (for
/// example `"Shift_JIS"`); nothing is transcoded or checked. `false` if `id` is not a string, `encoding` is
/// empty, or either length is above `i32::MAX`.
#[no_mangle]
pub unsafe extern "C" fn mrs_set_string_with_encoding(
    arena: &mut MrsArena,
    id: MrsValueId,
    bytes: *const u8,
    len: usize,
    encoding: *const u8,
    encoding_len: usize,
) -> bool {
    match (unsafe { bytes_vec(bytes, len) }, unsafe {
        bytes_vec(encoding, encoding_len)
    }) {
        (Some(bytes), Some(encoding)) if !encoding.is_empty() => {
            arena.0.set_str_content(id, bytes).is_some() && arena.0.set_encoding_name(id, &encoding)
        }
        _ => false,
    }
}

/// Changes the declared encoding of the string, regexp or symbol `id` to the one with `encoding_id` (see
/// `mrs_encoding_id`), without touching its bytes - like `force_encoding`. `false` for any other kind of value,
/// or an id outside the table (including `MRS_ENCODING_CUSTOM`; use `mrs_set_encoding_name` for those).
#[no_mangle]
pub extern "C" fn mrs_set_encoding(arena: &mut MrsArena, id: MrsValueId, encoding_id: u8) -> bool {
    arena.0.set_encoding(id, encoding_id)
}

/// `mrs_set_encoding` for an encoding known by its name, which may be one this library has no id for.
#[no_mangle]
pub unsafe extern "C" fn mrs_set_encoding_name(
    arena: &mut MrsArena,
    id: MrsValueId,
    encoding: *const u8,
    len: usize,
) -> bool {
    unsafe { bytes_vec(encoding, len) }.is_some_and(|name| !name.is_empty() && arena.0.set_encoding_name(id, &name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr::{null, null_mut};

    unsafe fn load_bytes(bytes: &[u8]) -> *mut MrsArena {
        mrs_load(bytes.as_ptr(), bytes.len(), null_mut())
    }

    unsafe fn dump_bytes(arena: *const MrsArena) -> Vec<u8> {
        let mut len = 0;
        let ptr = mrs_dump(arena, &mut len);
        let bytes = unsafe { slice::from_raw_parts(ptr, len) }.to_vec();
        mrs_buffer_free(ptr, len);
        bytes
    }

    #[test]
    fn load_reports_why_and_where() {
        let mut error = NO_ERROR;
        unsafe {
            assert!(mrs_load_ex(null(), 0, &mut error).is_null());
        }
        assert_eq!(error.code, MrsError::InvalidHeader);

        let bytes = b"\x04\x080x";
        unsafe {
            assert!(mrs_load_ex(bytes.as_ptr(), bytes.len(), &mut error).is_null());
        }
        assert_eq!((error.code, error.offset), (MrsError::TrailingData, 3));
        // `NULL` for the error is allowed
        unsafe {
            assert!(mrs_load(bytes.as_ptr(), bytes.len(), null_mut()).is_null());
        }
    }

    #[test]
    fn out_parameters_may_be_null_and_uninitialized_memory_is_fine() {
        unsafe {
            let arena = load_bytes(b"\x04\x08i\x06");
            let root = mrs_root(&*arena);
            assert!(mrs_as_i64(&*arena, root, null_mut()));
            let mut value = core::mem::MaybeUninit::<i64>::uninit();
            assert!(mrs_as_i64(&*arena, root, value.as_mut_ptr()));
            assert_eq!(value.assume_init(), 1);
            assert!(!mrs_as_bool(&*arena, root, null_mut()));
            assert!(!mrs_as_bytes(&*arena, root, null_mut(), null_mut()));
            mrs_arena_free(arena);
        }
    }

    #[test]
    fn ids_outside_the_arena_are_harmless() {
        unsafe {
            let arena = load_bytes(b"\x04\x08[\x060");
            for bogus in [123_456, MRS_VALUE_ID_NONE] {
                assert_eq!(mrs_kind(&*arena, bogus), MrsKind::Invalid);
                assert_eq!(mrs_array_len(&*arena, bogus), 0);
                assert_eq!(mrs_array_get(&*arena, bogus, 0), MRS_VALUE_ID_NONE);
                assert_eq!(mrs_flags(&*arena, bogus), 0);
                assert_eq!(mrs_body(&*arena, bogus), MRS_VALUE_ID_NONE);
                assert_eq!(mrs_hash_get_symbol(&*arena, bogus, null(), 0), MRS_VALUE_ID_NONE);
            }
            mrs_arena_free(arena);
        }
    }

    #[test]
    fn strings_borrow_from_one_copy_of_the_input_and_outlive_the_callers_buffer() {
        unsafe {
            let arena = {
                let input = b"\x04\x08I\"\x08abc\x06:\x06ET".to_vec();
                load_bytes(&input)
            };
            let (mut ptr, mut len) = (null(), 0);
            assert!(mrs_as_bytes(&*arena, mrs_root(&*arena), &mut ptr, &mut len));
            assert_eq!(slice::from_raw_parts(ptr, len), b"abc");
            assert_eq!(dump_bytes(arena), b"\x04\x08I\"\x08abc\x06:\x06ET");
            mrs_arena_free(arena);
        }
    }

    #[test]
    fn many_streams_come_out_one_by_one() {
        unsafe {
            let bytes = b"\x04\x08i\x06\x04\x080";
            let mut error = NO_ERROR;
            let list = mrs_load_many(bytes.as_ptr(), bytes.len(), &mut error);
            assert!(!list.is_null());
            assert_eq!(mrs_arena_list_len(&*list), 2);
            let first = mrs_arena_list_take(&mut *list, 0);
            assert!(!first.is_null());
            assert!(mrs_arena_list_take(&mut *list, 0).is_null());
            assert!(mrs_arena_list_take(&mut *list, 2).is_null());
            // an arena taken out outlives the list, and the one left behind is freed with it
            mrs_arena_list_free(list);
            assert_eq!(dump_bytes(first), b"\x04\x08i\x06");
            mrs_arena_free(first);
        }
    }

    #[test]
    fn builder_refuses_what_does_not_fit_and_builds_everything_else() {
        unsafe {
            let arena = &mut *mrs_arena_new();
            let one = mrs_push_fixnum(arena, 1);
            let text = mrs_push_string(arena, b"hi".as_ptr(), 2);
            assert_eq!(mrs_push_string(arena, b"\xff".as_ptr(), 1), MRS_VALUE_ID_NONE);
            let float = mrs_push_float_text(arena, b"1.5".as_ptr(), 3);
            let symbol = mrs_push_symbol(arena, b"k".as_ptr(), 1);
            assert_eq!(mrs_push_class(arena, null(), 0), MRS_VALUE_ID_NONE);

            let hash = mrs_push_hash(arena, [symbol].as_ptr(), [one].as_ptr(), 1, float);
            assert_ne!(hash, MRS_VALUE_ID_NONE);
            assert_eq!(
                mrs_push_hash(arena, [999].as_ptr(), [one].as_ptr(), 1, MRS_VALUE_ID_NONE),
                MRS_VALUE_ID_NONE
            );

            let member = MrsMember {
                name: b"@a".as_ptr(),
                name_len: 2,
                value: text,
            };
            let object = mrs_push_object(arena, b"Foo".as_ptr(), 3, &member, 1);
            assert_ne!(object, MRS_VALUE_ID_NONE);
            assert_eq!(mrs_push_object(arena, null(), 0, null(), 0), MRS_VALUE_ID_NONE);
            assert!(mrs_set_member_value(arena, object, b"a".as_ptr(), 1, one));
            assert!(!mrs_set_member_value(arena, object, b"missing".as_ptr(), 7, one));
            assert!(mrs_push_extension(arena, object, b"M".as_ptr(), 1));
            assert!(!mrs_push_extension(arena, one, b"M".as_ptr(), 1));
            assert!(mrs_push_extra_ivar(arena, object, b"@z".as_ptr(), 2, one));

            let wrapper = mrs_push_user_marshal(arena, b"W".as_ptr(), 1, object);
            assert_ne!(wrapper, MRS_VALUE_ID_NONE);
            assert_eq!(mrs_body(arena, wrapper), object);
            let items = [wrapper, hash];
            let array = mrs_push_array(arena, items.as_ptr(), 2);
            assert!(mrs_set_root(arena, array));
            assert!(mrs_set_user_class(arena, array, b"MyArray".as_ptr(), 7));
            assert!(!mrs_set_user_class(arena, object, b"X".as_ptr(), 1));

            let bytes = dump_bytes(arena);
            let back = load_bytes(&bytes);
            assert!(!back.is_null());
            assert_eq!(dump_bytes(back), bytes);
            mrs_arena_free(back);
            mrs_arena_free(arena);
        }
    }

    #[test]
    fn dump_pointers_and_bignum_text_are_freed_exactly() {
        unsafe {
            let arena = load_bytes(b"\x04\x08l+\x07\x01\x02\x03\x04");
            let (mut ptr, mut len) = (null_mut(), 0);
            assert!(mrs_as_bignum_decimal(&*arena, mrs_root(&*arena), &mut ptr, &mut len));
            assert_eq!(slice::from_raw_parts(ptr, len), b"67305985");
            mrs_buffer_free(ptr, len);
            // with nowhere to put the text nothing is allocated, so nothing leaks
            assert!(mrs_as_bignum_decimal(
                &*arena,
                mrs_root(&*arena),
                null_mut(),
                null_mut()
            ));
            mrs_arena_free(arena);
        }
    }

    #[test]
    fn encodings_of_strings_and_symbols_can_be_read_and_changed() {
        unsafe fn name_of(arena: &MrsArena, id: MrsValueId) -> Vec<u8> {
            let (mut ptr, mut len) = (null(), 0);
            assert!(mrs_encoding_name(arena, id, &mut ptr, &mut len));
            slice::from_raw_parts(ptr, len).to_vec()
        }

        unsafe {
            let arena = &mut *mrs_arena_new();
            let text = mrs_push_string_with_encoding(arena, b"abc".as_ptr(), 3, b"Shift_JIS".as_ptr(), 9);
            let symbol = mrs_push_symbol_with_encoding(arena, b"\x83A".as_ptr(), 2, b"Shift_JIS".as_ptr(), 9);
            let one = mrs_push_fixnum(arena, 1);
            assert_eq!(name_of(arena, text), b"Shift_JIS");
            assert_eq!(name_of(arena, symbol), b"Shift_JIS");

            // `force_encoding`: the bytes stay, the tag changes - a name the table does not have included
            assert!(mrs_set_encoding_name(arena, symbol, b"EUC-JP".as_ptr(), 6));
            assert_eq!(name_of(arena, symbol), b"EUC-JP");
            assert!(mrs_set_encoding_name(arena, text, b"Made-Up".as_ptr(), 7));
            let mut id = 0;
            assert!(mrs_encoding_id(arena, text, &mut id));
            assert_eq!(id, MRS_ENCODING_CUSTOM);
            assert_eq!(name_of(arena, text), b"Made-Up");
            assert!(mrs_set_encoding(arena, text, 0));
            assert_eq!(name_of(arena, text), b"ASCII-8BIT");

            // new text is UTF-8 whatever the old was; new bytes can say what they are
            assert!(mrs_set_string_text(arena, text, b"yo".as_ptr(), 2));
            assert_eq!(name_of(arena, text), b"UTF-8");
            assert!(mrs_set_string_with_encoding(
                arena,
                text,
                b"\x83A".as_ptr(),
                2,
                b"Shift_JIS".as_ptr(),
                9
            ));
            assert_eq!(name_of(arena, text), b"Shift_JIS");
            assert!(mrs_set_string_content(arena, text, b"\x83B".as_ptr(), 2));
            assert_eq!(name_of(arena, text), b"Shift_JIS");

            // nothing else has an encoding
            assert!(!mrs_set_encoding(arena, one, 0));
            assert!(!mrs_set_encoding(arena, text, MRS_ENCODING_CUSTOM));
            assert!(!mrs_set_encoding_name(arena, text, null(), 0));
            mrs_arena_free(arena);
        }
    }

    #[test]
    fn hash_default_and_bytes_content_can_be_replaced() {
        unsafe {
            let arena = &mut *mrs_arena_new();
            let key = mrs_push_symbol(arena, b"k".as_ptr(), 1);
            let one = mrs_push_fixnum(arena, 1);
            let two = mrs_push_fixnum(arena, 2);
            let hash = mrs_push_hash(arena, [key].as_ptr(), [one].as_ptr(), 1, two);
            assert!(mrs_set_hash_default(arena, hash, one));
            assert_eq!(mrs_hash_default(arena, hash), one);
            let no_default = mrs_push_hash(arena, [key].as_ptr(), [one].as_ptr(), 1, MRS_VALUE_ID_NONE);
            assert!(!mrs_set_hash_default(arena, no_default, one));

            let bytes = mrs_push_bytes(arena, b"ab".as_ptr(), 2);
            assert!(mrs_set_bytes_content(arena, bytes, b"xyz".as_ptr(), 3));
            assert!(!mrs_set_bytes_content(arena, one, b"x".as_ptr(), 1));
            let (mut ptr, mut len) = (null(), 0);
            assert!(mrs_as_bytes(arena, bytes, &mut ptr, &mut len));
            assert_eq!(slice::from_raw_parts(ptr, len), b"xyz");
            mrs_arena_free(arena);
        }
    }

    #[test]
    fn version_is_a_c_string() {
        let version = unsafe { core::ffi::CStr::from_ptr(mrs_version()) };
        assert_eq!(version.to_str().unwrap(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn hash_and_array_entries_are_read_and_replaced_by_index() {
        unsafe {
            let arena = &mut *mrs_arena_new();
            let key = mrs_push_symbol(arena, b"k".as_ptr(), 1);
            let one = mrs_push_fixnum(arena, 1);
            let two = mrs_push_fixnum(arena, 2);
            let hash = mrs_push_hash(arena, [key].as_ptr(), [one].as_ptr(), 1, MRS_VALUE_ID_NONE);
            assert_eq!(mrs_hash_key_at(arena, hash, 0), key);
            assert_eq!(mrs_hash_value_at(arena, hash, 0), one);
            assert_eq!(mrs_hash_key_at(arena, hash, 1), MRS_VALUE_ID_NONE);
            assert_eq!(mrs_hash_value_at(arena, hash, 1), MRS_VALUE_ID_NONE);

            assert!(mrs_set_hash_key(arena, hash, 0, two));
            assert!(mrs_set_hash_value(arena, hash, 0, key));
            assert_eq!(mrs_hash_key_at(arena, hash, 0), two);
            assert_eq!(mrs_hash_value_at(arena, hash, 0), key);
            assert!(!mrs_set_hash_value(arena, hash, 5, one));

            let items = [one, two];
            let array = mrs_push_array(arena, items.as_ptr(), 2);
            assert_eq!(mrs_array_get(arena, array, 1), two);
            assert_eq!(mrs_array_get(arena, array, 2), MRS_VALUE_ID_NONE);
            assert!(mrs_set_array_value(arena, array, 0, key));
            assert_eq!(mrs_array_get(arena, array, 0), key);
            assert!(!mrs_set_array_value(arena, array, 9, one));
            mrs_arena_free(arena);
        }
    }

    #[test]
    fn floats_and_bignums_come_out_as_numbers_and_text() {
        unsafe {
            let arena = &mut *mrs_arena_new();
            let float = mrs_push_float_text(arena, b"1.5".as_ptr(), 3);
            let mut value = 0.0f64;
            assert!(mrs_as_f64(arena, float, &mut value));
            assert_eq!(value, 1.5);
            let (mut text, mut text_len) = (null(), 0);
            assert!(mrs_as_float_text(arena, float, &mut text, &mut text_len));
            assert_eq!(slice::from_raw_parts(text, text_len), b"1.5");

            let magnitude = [0x01u8, 0x02];
            let big = mrs_push_bignum(arena, true, magnitude.as_ptr(), 2);
            let (mut negative, mut digits, mut digits_len) = (false, null(), 0);
            assert!(mrs_as_bignum_bytes(
                arena,
                big,
                &mut negative,
                &mut digits,
                &mut digits_len
            ));
            assert!(negative);
            assert_eq!(slice::from_raw_parts(digits, digits_len), magnitude);
            mrs_arena_free(arena);
        }
    }

    #[test]
    fn regexps_paths_and_modules_are_read_back() {
        unsafe {
            let arena = &mut *mrs_arena_new();
            let regexp = mrs_push_regexp(arena, b"a+b".as_ptr(), 3, MRS_REGEXP_IGNORE_CASE, b"UTF-8".as_ptr(), 5);
            let (mut source, mut source_len, mut options) = (null(), 0, 0u8);
            assert!(mrs_as_regexp(arena, regexp, &mut source, &mut source_len, &mut options));
            assert_eq!(slice::from_raw_parts(source, source_len), b"a+b");
            assert_eq!(options, MRS_REGEXP_IGNORE_CASE);

            let class = mrs_push_class(arena, b"Foo::Bar".as_ptr(), 8);
            assert_eq!(mrs_kind(arena, class), MrsKind::Class);
            let (mut path, mut path_len) = (null(), 0);
            assert!(mrs_as_path(arena, class, &mut path, &mut path_len));
            assert_eq!(slice::from_raw_parts(path, path_len), b"Foo::Bar");

            let module = mrs_push_module(arena, b"Mod".as_ptr(), 3, false);
            assert_eq!(mrs_kind(arena, module), MrsKind::Module);
            mrs_arena_free(arena);
        }
    }

    #[test]
    fn object_members_extensions_and_extra_ivars_are_listed() {
        unsafe {
            let arena = &mut *mrs_arena_new();
            let text = mrs_push_string(arena, b"hi".as_ptr(), 2);
            let member = MrsMember {
                name: b"@a".as_ptr(),
                name_len: 2,
                value: text,
            };
            let object = mrs_push_object(arena, b"Foo".as_ptr(), 3, &member, 1);
            assert_eq!(mrs_members_len(arena, object), 1);
            assert_eq!(mrs_member_value_at(arena, object, 0), text);
            assert_eq!(mrs_member_get(arena, object, b"a".as_ptr(), 1), text);
            assert_eq!(mrs_member_get(arena, object, b"missing".as_ptr(), 7), MRS_VALUE_ID_NONE);
            let (mut name, mut name_len) = (null(), 0);
            assert!(mrs_member_name_at(arena, object, 0, &mut name, &mut name_len));
            assert_eq!(slice::from_raw_parts(name, name_len), b"@a");
            assert!(!mrs_member_name_at(arena, object, 4, &mut name, &mut name_len));

            let (mut class, mut class_len) = (null(), 0);
            assert!(mrs_class_name(arena, object, &mut class, &mut class_len));
            assert_eq!(slice::from_raw_parts(class, class_len), b"Foo");

            assert!(mrs_push_extension(arena, object, b"M".as_ptr(), 1));
            assert_eq!(mrs_extension_count(arena, object), 1);
            let (mut module, mut module_len) = (null(), 0);
            assert!(mrs_extension_at(arena, object, 0, &mut module, &mut module_len));
            assert_eq!(slice::from_raw_parts(module, module_len), b"M");

            // An array with one instance variable, `@z => 1`. On an object such a variable is a member instead.
            let loaded = load_bytes(b"\x04\x08I[\x00\x06:\x07@zi\x06");
            let root = mrs_root(&*loaded);
            assert_eq!(mrs_extra_ivar_count(&*loaded, root), 1);
            let (mut ivar, mut ivar_len) = (null(), 0);
            assert!(mrs_extra_ivar_name_at(&*loaded, root, 0, &mut ivar, &mut ivar_len));
            assert_eq!(slice::from_raw_parts(ivar, ivar_len), b"@z");
            let mut value = 0i64;
            assert!(mrs_as_i64(
                &*loaded,
                mrs_extra_ivar_value_at(&*loaded, root, 0),
                &mut value
            ));
            assert_eq!(value, 1);
            mrs_arena_free(loaded);
            mrs_arena_free(arena);
        }
    }

    #[test]
    fn nil_bools_structs_and_data_are_built_and_flagged() {
        unsafe {
            let arena = &mut *mrs_arena_new();
            let nil = mrs_push_nil(arena);
            let yes = mrs_push_bool(arena, true);
            let no = mrs_push_bool(arena, false);
            assert_eq!(mrs_kind(arena, nil), MrsKind::Nil);
            assert_eq!(mrs_kind(arena, yes), MrsKind::True);
            assert_eq!(mrs_kind(arena, no), MrsKind::False);

            let member = MrsMember {
                name: b"@a".as_ptr(),
                name_len: 2,
                value: yes,
            };
            let point = mrs_push_struct(arena, b"Point".as_ptr(), 5, &member, 1);
            assert_eq!(mrs_kind(arena, point), MrsKind::Struct);
            assert_eq!(mrs_member_get(arena, point, b"a".as_ptr(), 1), yes);

            let text = mrs_push_string(arena, b"x".as_ptr(), 1);
            let blob = mrs_push_data(arena, b"Blob".as_ptr(), 4, text);
            assert_ne!(blob, MRS_VALUE_ID_NONE);
            assert_ne!(mrs_flags(arena, blob) & MRS_FLAG_DATA, 0);
            mrs_arena_free(arena);
        }
    }

    #[test]
    fn user_defined_and_user_class_can_be_set_and_cleared() {
        unsafe {
            let arena = &mut *mrs_arena_new();
            let bytes = mrs_push_bytes(arena, b"ab".as_ptr(), 2);
            let one = mrs_push_fixnum(arena, 1);
            let array = mrs_push_array(arena, [one].as_ptr(), 1);

            assert!(mrs_set_user_defined(arena, bytes, b"Dumped".as_ptr(), 6));
            assert_ne!(mrs_flags(arena, bytes) & MRS_FLAG_USER_DEFINED, 0);
            assert!(!mrs_set_user_defined(arena, one, b"Dumped".as_ptr(), 6));

            assert!(mrs_set_user_class(arena, array, b"MyArray".as_ptr(), 7));
            assert!(mrs_clear_user_class(arena, array));
            assert_eq!(mrs_flags(arena, array) & MRS_FLAG_USER_CLASS, 0);

            assert!(mrs_set_fixnum_value(arena, one, 9));
            let mut value = 0i64;
            assert!(mrs_as_i64(arena, one, &mut value));
            assert_eq!(value, 9);
            assert!(!mrs_set_fixnum_value(arena, array, 9));
            mrs_arena_free(arena);
        }
    }

    #[test]
    fn malformed_streams_report_each_reason() {
        let cases: [(&[u8], MrsError); 8] = [
            (b"\x04\x08", MrsError::UnexpectedEof),
            (b"\x04\x08[\x07", MrsError::LengthOverflow),
            (b"\x04\x08X", MrsError::InvalidTag),
            (b"\x04\x08;\x00", MrsError::UnknownSymbolLink),
            (b"\x04\x08@\x06", MrsError::UnknownObjectLink),
            (b"\x04\x08[\xf6", MrsError::NegativeLength),
            (b"\x04\x08c\x00", MrsError::EmptyName),
            (b"\x04\x080x", MrsError::TrailingData),
        ];

        for (bytes, code) in cases {
            let mut error = NO_ERROR;
            unsafe {
                assert!(mrs_load_ex(bytes.as_ptr(), bytes.len(), &mut error).is_null());
            }
            assert_eq!(error.code, code, "{bytes:?}");
        }
    }

    #[test]
    fn load_functions_report_through_their_out_parameters_and_frees_accept_null() {
        let mut code = NO_ERROR.code;
        unsafe {
            assert!(mrs_load(b"x".as_ptr(), 1, &mut code).is_null());
        }
        assert_eq!(code, MrsError::InvalidHeader);

        let mut error = NO_ERROR;
        unsafe {
            assert!(mrs_load_many(b"x".as_ptr(), 1, &mut error).is_null());
        }
        assert_eq!(error.code, MrsError::InvalidHeader);
        unsafe {
            assert!(mrs_load_many(b"x".as_ptr(), 1, null_mut()).is_null());
        }

        unsafe {
            mrs_arena_list_free(null_mut());
        }
        unsafe {
            mrs_arena_free(null_mut());
        }
        mrs_buffer_free(null_mut(), 0);
    }

    #[test]
    fn every_kind_the_builders_make_is_reported() {
        unsafe {
            let arena = &mut *mrs_arena_new();
            let nil = mrs_push_nil(arena);
            let yes = mrs_push_bool(arena, true);
            let one = mrs_push_fixnum(arena, 1);
            let big = mrs_push_bignum(arena, false, [1u8; 9].as_ptr(), 9);
            let float = mrs_push_float_text(arena, b"1.5".as_ptr(), 3);
            let bytes = mrs_push_bytes(arena, b"b".as_ptr(), 1);
            let text = mrs_push_string(arena, b"s".as_ptr(), 1);
            let symbol = mrs_push_symbol(arena, b"y".as_ptr(), 1);
            let regexp = mrs_push_regexp(arena, b"r".as_ptr(), 1, 0, null(), 0);
            let array = mrs_push_array(arena, [one].as_ptr(), 1);
            let hash = mrs_push_hash(arena, [symbol].as_ptr(), [one].as_ptr(), 1, MRS_VALUE_ID_NONE);
            let class = mrs_push_class(arena, b"Foo".as_ptr(), 3);
            let module = mrs_push_module(arena, b"M".as_ptr(), 1, false);
            let member = MrsMember {
                name: b"@a".as_ptr(),
                name_len: 2,
                value: one,
            };
            let object = mrs_push_object(arena, b"Foo".as_ptr(), 3, &member, 1);
            let structure = mrs_push_struct(arena, b"S".as_ptr(), 1, &member, 1);

            for (id, kind) in [
                (nil, MrsKind::Nil),
                (yes, MrsKind::True),
                (one, MrsKind::Fixnum),
                (big, MrsKind::Bignum),
                (float, MrsKind::Float),
                (bytes, MrsKind::Bytes),
                (text, MrsKind::Str),
                (symbol, MrsKind::Symbol),
                (regexp, MrsKind::Regexp),
                (array, MrsKind::Array),
                (hash, MrsKind::Hash),
                (class, MrsKind::Class),
                (module, MrsKind::Module),
                (object, MrsKind::Object),
                (structure, MrsKind::Struct),
            ] {
                assert_eq!(mrs_kind(arena, id), kind, "{kind:?}");
            }
            assert_eq!(mrs_kind(arena, 123_456), MrsKind::Invalid);
            mrs_arena_free(arena);
        }
    }

    #[test]
    fn accessors_refuse_other_kinds_and_ids_outside_the_arena() {
        unsafe {
            let arena = &mut *mrs_arena_new();
            let one = mrs_push_fixnum(arena, 1);
            let (mut ptr, mut len) = (null(), 0usize);
            let (mut options, mut encoding, mut negative) = (0u8, 0u8, false);
            let mut decimal = null_mut();

            for id in [one, MRS_VALUE_ID_NONE] {
                assert!(!mrs_class_name(arena, id, &mut ptr, &mut len));
                assert!(!mrs_as_path(arena, id, &mut ptr, &mut len));
                assert!(!mrs_as_regexp(arena, id, &mut ptr, &mut len, &mut options));
                assert!(!mrs_encoding_name(arena, id, &mut ptr, &mut len));
                assert!(!mrs_encoding_id(arena, id, &mut encoding));
                assert!(!mrs_as_bignum_decimal(arena, id, &mut decimal, &mut len));
                assert!(!mrs_as_bignum_bytes(arena, id, &mut negative, &mut ptr, &mut len));
                assert!(!mrs_as_float_text(arena, id, &mut ptr, &mut len));
                assert!(!mrs_as_bytes(arena, id, &mut ptr, &mut len));
                assert!(!mrs_member_name_at(arena, id, 0, &mut ptr, &mut len));
                assert!(!mrs_extension_at(arena, id, 0, &mut ptr, &mut len));
                assert!(!mrs_extra_ivar_name_at(arena, id, 0, &mut ptr, &mut len));
                assert_eq!(mrs_extension_count(arena, id), 0);
                assert_eq!(mrs_extra_ivar_count(arena, id), 0);
                assert_eq!(mrs_members_len(arena, id), 0);
                assert_eq!(mrs_array_len(arena, id), 0);
                assert_eq!(mrs_hash_len(arena, id), 0);
                assert_eq!(mrs_array_get(arena, id, 0), MRS_VALUE_ID_NONE);
                assert_eq!(mrs_hash_default(arena, id), MRS_VALUE_ID_NONE);
                assert_eq!(mrs_hash_key_at(arena, id, 0), MRS_VALUE_ID_NONE);
                assert_eq!(mrs_hash_value_at(arena, id, 0), MRS_VALUE_ID_NONE);
                assert_eq!(mrs_member_value_at(arena, id, 0), MRS_VALUE_ID_NONE);
                assert_eq!(mrs_member_get(arena, id, b"a".as_ptr(), 1), MRS_VALUE_ID_NONE);
                assert_eq!(mrs_extra_ivar_value_at(arena, id, 0), MRS_VALUE_ID_NONE);
                assert_eq!(mrs_body(arena, id), MRS_VALUE_ID_NONE);
                assert_eq!(mrs_flags(arena, id), 0);

                assert!(!mrs_set_string_text(arena, id, b"x".as_ptr(), 1));
                assert!(!mrs_set_string_content(arena, id, b"x".as_ptr(), 1));
                assert!(!mrs_set_bytes_content(arena, id, b"x".as_ptr(), 1));
                assert!(!mrs_set_string_with_encoding(
                    arena,
                    id,
                    b"x".as_ptr(),
                    1,
                    b"UTF-8".as_ptr(),
                    5
                ));
                assert!(!mrs_set_encoding(arena, id, 1));
                assert!(!mrs_set_encoding_name(arena, id, b"UTF-8".as_ptr(), 5));
                assert!(!mrs_set_user_class(arena, id, b"Foo".as_ptr(), 3));
                assert!(!mrs_set_user_defined(arena, id, b"Foo".as_ptr(), 3));
                assert!(!mrs_clear_user_class(arena, id));
                assert!(!mrs_push_extension(arena, id, b"M".as_ptr(), 1));
                assert!(!mrs_push_extra_ivar(arena, id, b"@z".as_ptr(), 2, one));
                assert!(!mrs_set_member_value(arena, id, b"a".as_ptr(), 1, one));
                assert!(!mrs_set_array_value(arena, id, 0, one));
                assert!(!mrs_set_hash_key(arena, id, 0, one));
                assert!(!mrs_set_hash_default(arena, id, one));
            }

            assert!(!mrs_set_root(arena, MRS_VALUE_ID_NONE));
            mrs_arena_free(arena);
        }
    }

    #[test]
    fn builders_refuse_empty_encodings_and_null_arguments() {
        unsafe {
            let arena = &mut *mrs_arena_new();
            let one = mrs_push_fixnum(arena, 1);
            let symbol = mrs_push_symbol(arena, b"k".as_ptr(), 1);

            assert_eq!(
                mrs_push_string_with_encoding(arena, b"x".as_ptr(), 1, b"".as_ptr(), 0),
                MRS_VALUE_ID_NONE
            );
            // A length past the limit is refused before the pointer is read, so `NULL` is safe to pass with it.
            assert_eq!(
                mrs_push_string_with_encoding(arena, null(), usize::MAX, b"UTF-8".as_ptr(), 5),
                MRS_VALUE_ID_NONE
            );
            assert_eq!(
                mrs_push_symbol_with_encoding(arena, b"k".as_ptr(), 1, b"".as_ptr(), 0),
                MRS_VALUE_ID_NONE
            );
            assert_eq!(
                mrs_push_regexp(arena, null(), usize::MAX, 0, null(), 0),
                MRS_VALUE_ID_NONE
            );
            assert_eq!(
                mrs_push_hash(arena, [symbol].as_ptr(), [one].as_ptr(), usize::MAX, MRS_VALUE_ID_NONE),
                MRS_VALUE_ID_NONE
            );

            let member = MrsMember {
                name: b"@a".as_ptr(),
                name_len: 2,
                value: one,
            };
            assert_eq!(
                mrs_push_object(arena, null(), usize::MAX, &member, 1),
                MRS_VALUE_ID_NONE
            );
            assert_eq!(
                mrs_push_object(arena, b"Foo".as_ptr(), 3, null(), usize::MAX),
                MRS_VALUE_ID_NONE
            );
            assert_eq!(
                mrs_push_struct(arena, b"S".as_ptr(), 1, null(), usize::MAX),
                MRS_VALUE_ID_NONE
            );

            assert!(!mrs_set_string_with_encoding(
                arena,
                symbol,
                b"x".as_ptr(),
                1,
                b"".as_ptr(),
                0
            ));
            assert!(!mrs_as_bool(arena, one, &mut false));
            assert!(!mrs_as_i64(arena, symbol, &mut 0));
            assert!(!mrs_as_f64(arena, one, &mut 0.0));

            let yes = mrs_push_bool(arena, true);
            let mut flag = false;
            assert!(mrs_as_bool(arena, yes, &mut flag));
            assert!(flag);
            mrs_arena_free(arena);
        }
    }

    #[test]
    fn hash_symbol_lookup_finds_the_value_for_a_symbol_key() {
        unsafe {
            let arena = &mut *mrs_arena_new();
            let key = mrs_push_symbol(arena, b"k".as_ptr(), 1);
            let one = mrs_push_fixnum(arena, 1);
            let hash = mrs_push_hash(arena, [key].as_ptr(), [one].as_ptr(), 1, MRS_VALUE_ID_NONE);

            assert_eq!(mrs_hash_get_symbol(arena, hash, b"k".as_ptr(), 1), one);
            assert_eq!(mrs_hash_get_symbol(arena, hash, b"z".as_ptr(), 1), MRS_VALUE_ID_NONE);
            mrs_arena_free(arena);
        }
    }

    #[test]
    fn a_symbol_encoding_that_is_not_a_string_is_unsupported() {
        // A symbol whose `E` ivar is an integer rather than `true`, `false` or a string.
        let bytes = b"\x04\x08I:\x06a\x06:\x06Ei\x06";
        let mut error = NO_ERROR;
        unsafe {
            assert!(mrs_load_ex(bytes.as_ptr(), bytes.len(), &mut error).is_null());
        }
        assert_eq!(error.code, MrsError::Unsupported);
    }

    #[cfg(feature = "std")]
    extern "C" fn refuse_alloc(_size: usize, _align: usize) -> *mut u8 {
        null_mut()
    }

    #[cfg(feature = "std")]
    extern "C" fn ignore_free(_ptr: *mut u8, _size: usize, _align: usize) {}

    #[cfg(feature = "std")]
    #[test]
    fn the_host_allocator_hook_is_accepted_with_std() {
        // With `std` the hook is ignored; it must not change anything else the tests rely on.
        mrs_set_allocator(refuse_alloc, ignore_free);
    }
}
