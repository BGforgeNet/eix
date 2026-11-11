//! The game's byte streams (`ByteStream`): the memory stream and the file stream, as the addon's `aigraph` command
//! uses them to write the AI graph.
//!
//! Names follow ei-rebuild.
#![expect(
    unsafe_code,
    reason = "calls into game.exe's stream code and through stream vtables"
)]

use core::ffi::{CStr, c_void};

use super::code;

/// The bytes the addon's `aigraph` reserves on its stack for a memory stream; the stream's
/// constructor and its destructor use only its head.
pub const MEMORY_STREAM_BYTES: usize = 0x80C;

/// The bytes the addon reserves for a file stream.
pub const FILE_STREAM_BYTES: usize = 0x28;

/// A stream's memory, aligned as the game's objects are; the game's constructors fill it.
#[repr(C, align(8))]
#[derive(Debug)]
pub struct StreamMemory<const N: usize>([u8; N]);

impl<const N: usize> StreamMemory<N> {
    /// Zeroed memory for one stream.
    pub const fn new() -> Self {
        Self([0; N])
    }

    /// Its address, as the game's code takes `this`.
    pub fn as_mut_ptr(&mut self) -> *mut c_void {
        self.0.as_mut_ptr().cast()
    }
}

impl<const N: usize> Default for StreamMemory<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Constructs an empty memory stream at `this`: `CMemoryStream::Construct` 0x41DD70 (thiscall, plain `ret`).
///
/// # Safety
/// The process must be a supported game.exe, on the game's thread; `this` must be writable for [`MEMORY_STREAM_BYTES`].
pub unsafe fn construct_memory_stream(this: *mut c_void) {
    // SAFETY: 0x41DD70 is `CMemoryStream* __thiscall ()`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void) -> *mut c_void>(
            eix_sites::C_MEMORY_STREAM_CONSTRUCT,
        )
    };
    // SAFETY: the caller guarantees the memory and the thread.
    unsafe { f(this) };
}

/// Frees a memory stream's buffer: `CMemoryStream::Destruct` 0x4173D0 (thiscall, plain `ret`).
///
/// # Safety
/// As for [`construct_memory_stream`]; `this` must be a constructed memory stream.
pub unsafe fn destruct_memory_stream(this: *mut c_void) {
    // SAFETY: 0x4173D0 is `void __thiscall ()`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void)>(eix_sites::C_MEMORY_STREAM_DESTRUCT)
    };
    // SAFETY: the caller guarantees the stream and the thread.
    unsafe { f(this) }
}

/// Constructs a closed file stream at `this`: `FileStream::Construct` 0x478470 (thiscall, plain `ret`).
///
/// # Safety
/// As for [`construct_memory_stream`]; `this` must be writable for [`FILE_STREAM_BYTES`].
pub unsafe fn construct_file_stream(this: *mut c_void) {
    // SAFETY: 0x478470 is `FileStream* __thiscall ()`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void) -> *mut c_void>(
            eix_sites::FILE_STREAM_CONSTRUCT,
        )
    };
    // SAFETY: the caller guarantees the memory and the thread.
    unsafe { f(this) };
}

/// Ends a file stream: `FileStream::Destruct` 0x477FB0 (thiscall, plain `ret`).
///
/// # Safety
/// As for [`construct_memory_stream`]; `this` must be a constructed file stream.
pub unsafe fn destruct_file_stream(this: *mut c_void) {
    // SAFETY: 0x477FB0 is `void __thiscall ()`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void)>(eix_sites::FILE_STREAM_DESTRUCT)
    };
    // SAFETY: the caller guarantees the stream and the thread.
    unsafe { f(this) }
}

/// The function in vtable slot `slot` of the object at `this`.
///
/// # Safety
/// `this` must be a constructed object whose vtable has at least `slot + 1` entries of type `F`.
unsafe fn virtual_fn<F: Copy>(this: *mut c_void, slot: usize) -> F {
    // SAFETY: a constructed object starts with its vtable pointer.
    let vtable = unsafe { this.cast::<*const usize>().read() };
    // SAFETY: the caller guarantees the slot exists.
    let address = unsafe { vtable.wrapping_add(slot).read() };
    // SAFETY: the caller guarantees the slot's type.
    unsafe { crate::hook::as_fn_at::<F>(address) }
}

/// Opens the file stream at `this` for writing `path`, as the addon's `aigraph` does: the
/// direction byte at +4 becomes 0, then the stream's file (at +0xC) opens through its vtable slot 8 with mode
/// 0x1001 (create, write) and no exception object, and the stream's state byte at +5 records whether it opened.
///
/// # Safety
/// As for [`destruct_file_stream`].
pub unsafe fn open_file_stream_for_writing(this: *mut c_void, path: &CStr) -> bool {
    let bytes = this.cast::<u8>();
    // SAFETY: the stream's direction byte, inside its memory.
    unsafe { bytes.wrapping_add(4).write(0) };
    let file = bytes.wrapping_add(0xC).cast::<c_void>();
    // SAFETY: the file object's slot 8 is `BOOL __thiscall Open(const char*, UINT, CFileException*)`.
    let open = unsafe {
        virtual_fn::<unsafe extern "thiscall" fn(*mut c_void, *const u8, u32, *mut c_void) -> i32>(
            file, 8,
        )
    };
    // SAFETY: the caller guarantees the stream; `path` is NUL-terminated.
    let opened = unsafe { open(file, path.as_ptr().cast(), 0x1001, core::ptr::null_mut()) } != 0;
    // SAFETY: the stream's state byte, inside its memory.
    unsafe { bytes.wrapping_add(5).write(u8::from(opened)) };
    opened
}

/// Writes the whole of `source` into the stream at `this`: its vtable slot 7, `CopyFrom(ByteStream*)` (thiscall,
/// `ret 4`), as the addon calls it.
///
/// # Safety
/// As for [`destruct_file_stream`]; `source` must be a constructed stream.
pub unsafe fn copy_from(this: *mut c_void, source: *mut c_void) {
    // SAFETY: slot 7 of a ByteStream is `void __thiscall CopyFrom(ByteStream*)`.
    let f = unsafe { virtual_fn::<unsafe extern "thiscall" fn(*mut c_void, *mut c_void)>(this, 7) };
    // SAFETY: the caller guarantees both streams.
    unsafe { f(this, source) }
}

/// Writes the memory stream `memory` into the open file stream `this` as a save record of kind `kind` and `version`:
/// `SaveFileWrite` 0x579B80 (ei-rebuild's name; fastcall: `this` and `kind` in `ecx` and `edx`, `ret 8`). The addon
/// passes 0x114 as the kind and the save version.
///
/// # Safety
/// As for [`destruct_file_stream`]; `memory` must be a constructed memory stream holding what the file takes.
pub unsafe fn save_file_write(this: *mut c_void, kind: u32, version: u32, memory: *mut c_void) {
    // SAFETY: 0x579B80 is `void __fastcall (FileStream*, int kind, int version, ByteStream*)`.
    let f = unsafe {
        code::<unsafe extern "fastcall" fn(*mut c_void, u32, u32, *mut c_void)>(
            eix_sites::SAVE_FILE_WRITE,
        )
    };
    // SAFETY: the caller guarantees both streams and the thread.
    unsafe { f(this, kind, version, memory) }
}

/// Closes the file of the stream at `this` (the object at +0xC) through its vtable slot 19 (+0x4C; thiscall, no
/// arguments), as the addon does after writing a party file.
///
/// # Safety
/// As for [`destruct_file_stream`].
pub unsafe fn close_file(this: *mut c_void) {
    let file = this.cast::<u8>().wrapping_add(0xC).cast::<c_void>();
    // SAFETY: the stream's file object is constructed with its vtable; slot 19 is `void __thiscall ()`.
    let f = unsafe { virtual_fn::<unsafe extern "thiscall" fn(*mut c_void)>(file, 19) };
    // SAFETY: the caller guarantees the stream and the thread.
    unsafe { f(file) }
}

/// Runs the stream's virtual destructor without freeing its memory: vtable slot 0 with flag 0 (thiscall, `ret 4`),
/// as the addon calls it after the explicit destructors.
///
/// # Safety
/// As for [`destruct_file_stream`]; the stream is not used afterwards.
pub unsafe fn destroy_in_place(this: *mut c_void) {
    // SAFETY: slot 0 of a ByteStream is its scalar deleting destructor, `void* __thiscall (unsigned int)`.
    let f = unsafe {
        virtual_fn::<unsafe extern "thiscall" fn(*mut c_void, u32) -> *mut c_void>(this, 0)
    };
    // SAFETY: the caller guarantees the stream; flag 0 frees nothing.
    unsafe { f(this, 0) };
}
