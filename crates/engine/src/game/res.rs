//! Resource archives (`.res`): `MappedResource` and the managers that keep extra archives, as the ResOverride
//! group calls them.
#![expect(
    unsafe_code,
    reason = "calling game.exe's archive and string-array functions"
)]

use core::ffi::{CStr, c_void};

use eix_sites::Site;

use super::code;
use super::mfc::{GameArray, GameString};

/// A `MappedResource` (a `.res` archive), opaque: only its size is used, to allocate one.
///
/// Size from `TextureManager::AddResource` 0x675050, which allocates 0x58 bytes (`push 0x58; call operator new` at
/// 0x6750BC) and passes them to the constructor 0x671280.
#[repr(C, align(8))]
#[derive(Debug)]
pub struct MappedResource {
    _unknown00: [u8; 0x58],
}

const _: () = {
    assert!(core::mem::offset_of!(MappedResource, _unknown00) == 0x0);
    assert!(size_of::<MappedResource>() == 0x58);
};

impl MappedResource {
    /// Zeroed storage for one archive, which [`construct`] then makes an archive of.
    pub const fn zeroed() -> Self {
        Self {
            _unknown00: [0; 0x58],
        }
    }
}

/// An MFC `CStringArray`, as `MappedResource::ListFiles` fills it.
pub type StringArray = GameArray<GameString>;

/// `MappedResource::MappedResource` 0x671280: thiscall, no stack arguments (plain `ret`).
///
/// # Safety
/// The process must be a supported game.exe; `this` must be writable, zeroed, and hold no constructed archive.
pub unsafe fn construct(this: *mut MappedResource) {
    // SAFETY: 0x671280 takes `this` in ECX and returns it; nothing on the stack.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut MappedResource) -> *mut MappedResource>(
            eix_sites::MAPPED_RESOURCE_MAPPED_RESOURCE,
        )
    };
    // SAFETY: the caller guarantees the build and `this`.
    unsafe { f(this) };
}

/// `MappedResource::OpenResource` 0x671730: thiscall `(path, flags)`, `ret 8`; the flags word lands at +0x2C.
/// Returns whether the archive opened.
///
/// # Safety
/// The process must be a supported game.exe and `this` a constructed archive.
pub unsafe fn open(this: *mut MappedResource, path: &CStr, flags: u32) -> bool {
    // SAFETY: 0x671730 takes `this` in ECX, the path and the flags on the stack, and cleans them (`ret 8`).
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut MappedResource, *const u8, u32) -> u8>(
            eix_sites::MAPPED_RESOURCE_OPEN_RESOURCE,
        )
    };
    // SAFETY: the caller guarantees the build and `this`; `path` is NUL-terminated.
    unsafe { f(this, path.as_ptr().cast(), flags) != 0 }
}

/// The address of `MappedResource::GetFile` 0x671B40: thiscall `(name, size out)` returning the file's data or null,
/// `ret 8`; also the archive vtable's slot at 0x7432F4.
pub const GET_FILE: Site = eix_sites::MAPPED_RESOURCE_GET_FILE;

/// The type of [`GET_FILE`].
pub type GetFile =
    unsafe extern "thiscall" fn(*mut MappedResource, *const u8, *mut u32) -> *mut c_void;

/// `MappedResource::GetFile` 0x671B40 itself, past any hook on its callers.
///
/// # Safety
/// The process must be a supported game.exe, `this` an open archive, `name` a NUL-terminated string and `size` null or
/// writable.
pub unsafe fn get_file(this: *mut MappedResource, name: *const u8, size: *mut u32) -> *mut c_void {
    // SAFETY: 0x671B40 is `GetFile` with this type.
    let f = unsafe { code::<GetFile>(GET_FILE) };
    // SAFETY: the caller's guarantees.
    unsafe { f(this, name, size) }
}

/// The address of `MappedResource::ListFiles` 0x674AC0: thiscall `(CStringArray* out, pattern)`, `ret 8`; it empties
/// `out` first (`SetSize(0, -1)` at 0x674AD8), then adds the names that match.
pub const LIST_FILES: Site = eix_sites::MAPPED_RESOURCE_LIST_FILES;

/// The type of [`LIST_FILES`].
pub type ListFiles = unsafe extern "thiscall" fn(*mut MappedResource, *mut StringArray, *const u8);

/// `MappedResource::ListFiles` 0x674AC0 itself.
///
/// # Safety
/// The process must be a supported game.exe, `this` an open archive, `out` a constructed array and `pattern` null or a
/// NUL-terminated string.
pub unsafe fn list_files(this: *mut MappedResource, out: *mut StringArray, pattern: *const u8) {
    // SAFETY: 0x674AC0 is `ListFiles` with this type.
    let f = unsafe { code::<ListFiles>(LIST_FILES) };
    // SAFETY: the caller's guarantees.
    unsafe { f(this, out, pattern) }
}

/// `CStringArray::CStringArray` 0x6FE925: thiscall, plain `ret`; sets the vtable and clears the four words.
///
/// # Safety
/// The process must be a supported game.exe; `this` must be writable and hold no constructed array.
pub unsafe fn array_construct(this: *mut StringArray) {
    // SAFETY: 0x6FE925 takes `this` in ECX and nothing on the stack.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut StringArray)>(
            eix_sites::C_STRING_ARRAY_C_STRING_ARRAY_AT_6FE925,
        )
    };
    // SAFETY: the caller's guarantees.
    unsafe { f(this) }
}

/// `CStringArray::Append` 0x6FEB01: thiscall `(const CStringArray& src)`, `ret 4`; adds copies of `src`'s strings.
///
/// # Safety
/// The process must be a supported game.exe; both arrays constructed and distinct.
pub unsafe fn array_append(this: *mut StringArray, src: *const StringArray) {
    // SAFETY: 0x6FEB01 takes `this` in ECX and `src` on the stack, and cleans it (`ret 4`); it grows through
    // `SetSize` 0x6FE9B8 at 0x6FEB15.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut StringArray, *const StringArray) -> i32>(
            eix_sites::C_STRING_ARRAY_APPEND,
        )
    };
    // SAFETY: the caller's guarantees.
    unsafe { f(this, src) };
}

/// `CStringArray::~CStringArray` 0x6FE958: thiscall, plain `ret`; destroys the strings and frees the block.
///
/// # Safety
/// The process must be a supported game.exe and `this` a constructed array, not used afterwards.
pub unsafe fn array_destroy(this: *mut StringArray) {
    // SAFETY: 0x6FE958 takes `this` in ECX and nothing on the stack.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut StringArray)>(
            eix_sites::C_STRING_ARRAY_C_STRING_ARRAY_AT_6FE958,
        )
    };
    // SAFETY: the caller's guarantees.
    unsafe { f(this) }
}

/// `FigureManager::AddResource` 0x5AA360: thiscall `(path)`, `ret 4`; opens an extra figure archive.
///
/// # Safety
/// The process must be a supported game.exe and `manager` the figure manager (`FigureManager::Create`'s ESI at
/// 0x5AA544).
pub unsafe fn figures_add(manager: *mut c_void, path: &CStr) {
    // SAFETY: 0x5AA360 takes `this` in ECX and the path on the stack, and cleans it (`ret 4`).
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void, *const u8) -> u8>(
            eix_sites::FIGURE_MANAGER_ADD_RESOURCE,
        )
    };
    // SAFETY: the caller's guarantees; `path` is NUL-terminated.
    unsafe { f(manager, path.as_ptr().cast()) };
}

/// The path range `LinkFigureLoader::AddResource` takes: the first byte and the one past the last.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PathRange {
    /// The path's first byte.
    pub begin: *const u8,
    /// One past its last byte.
    pub end: *const u8,
}

const _: () = {
    assert!(core::mem::offset_of!(PathRange, begin) == 0x0);
    assert!(core::mem::offset_of!(PathRange, end) == 0x4);
    assert!(size_of::<PathRange>() == 8);
};

/// `LinkFigureLoader::AddResource` 0x4F6D30: thiscall `(const PathRange*)`, `ret 4`; opens an extra figure archive for
/// the linked figures.
///
/// # Safety
/// The process must be a supported game.exe, `loader` the loader (`LinkFigureLoader::Construct`'s ESI at 0x4F6952) and
/// `path` a range over a live string.
pub unsafe fn linked_figures_add(loader: *mut c_void, path: &PathRange) {
    // SAFETY: 0x4F6D30 takes `this` in ECX and the range pointer on the stack, and cleans it (`ret 4`).
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void, *const PathRange) -> u8>(
            eix_sites::LINK_FIGURE_LOADER_ADD_RESOURCE,
        )
    };
    // SAFETY: the caller's guarantees.
    unsafe { f(loader, path) };
}

/// `TextureManager::AddResource` 0x675050: thiscall `(path)`, `ret 4`; opens an extra texture archive (it allocates
/// and opens its own [`MappedResource`]).
///
/// # Safety
/// The process must be a supported game.exe and `manager` the texture manager (`TextureManager::Create`'s ESI at
/// 0x6915EF).
pub unsafe fn textures_add(manager: *mut c_void, path: &CStr) {
    // SAFETY: 0x675050 takes `this` in ECX and the path on the stack, and cleans it (`ret 4`).
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void, *const u8) -> u8>(
            eix_sites::TEXTURE_MANAGER_ADD_RESOURCE,
        )
    };
    // SAFETY: the caller's guarantees; `path` is NUL-terminated.
    unsafe { f(manager, path.as_ptr().cast()) };
}
