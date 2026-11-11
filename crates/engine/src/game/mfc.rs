//! The MFC 4.2 and CRT pieces game.exe links statically: `CString`, `CArray`, `operator new` and `delete`.
//!
//! Memory the game frees must come from its own allocator and strings it keeps from its own `CString` functions; Rust's
//! allocator and types never stand in for them.
#![expect(
    unsafe_code,
    reason = "reading the game's strings and arrays, calling its allocator"
)]

use core::ffi::{CStr, c_void};
use core::mem::offset_of;

use super::code;

/// An MFC `CString`: one pointer to the characters, which follow a [`StringData`] header.
///
/// Layout from `CString::AllocBuffer` 0x7007DD (header of 12 bytes, characters after it) and `CString::AssignCopy`
/// 0x700A7C (length at -8); the shared empty string is the header at 0x796E4C, its characters at 0x796E48's pointer.
#[repr(transparent)]
#[derive(Debug)]
pub struct GameString {
    /// The characters, NUL-terminated, right after the header.
    pub chars: *mut u8,
}

/// The header before a [`GameString`]'s characters.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StringData {
    /// Reference count: `~CString` 0x700970 decrements it at -12.
    pub refs: i32,
    /// Length in bytes, without the NUL: `AssignCopy` 0x700A7C stores it at -8.
    pub len: i32,
    /// Bytes allocated, without the NUL: `AllocBeforeWrite` 0x700947 compares the new length with it at -4.
    pub capacity: i32,
}

const _: () = {
    assert!(offset_of!(StringData, refs) == 0x0);
    assert!(offset_of!(StringData, len) == 0x4);
    assert!(offset_of!(StringData, capacity) == 0x8);
    assert!(size_of::<StringData>() == 0xC);
    assert!(size_of::<GameString>() == 4);
};

impl GameString {
    /// The header before the characters; valid only for a constructed string (the game's or one made by
    /// [`construct`]).
    pub fn data(&self) -> *const StringData {
        self.chars.cast::<StringData>().wrapping_sub(1)
    }

    /// The characters, without the NUL.
    ///
    /// # Safety
    /// As for [`data`](Self::data); the string must not change while the slice lives.
    pub unsafe fn as_bytes(&self) -> &[u8] {
        // SAFETY: the caller guarantees a constructed string, so the header precedes the characters.
        let len = unsafe { (*self.data()).len };
        // SAFETY: a constructed string holds `len` characters; a negative length never occurs in one.
        unsafe { core::slice::from_raw_parts(self.chars, usize::try_from(len).unwrap_or(0)) }
    }
}

/// An MFC `CArray` (and `CPtrArray`, `CStringArray`): a vtable, then the element block and its counts.
///
/// Layout from `CStringArray::CStringArray` 0x6FE925 (the vtable at +0, then four cleared words) and `CArray::SetSize`
/// 0x44F7E0 (grow-by at +0x10, size at +0x8, capacity at +0xC); `CArray::ElementAt` 0x450750 indexes +0x4.
#[repr(C)]
#[derive(Debug)]
pub struct GameArray<T> {
    /// The array's vtable.
    pub vtable: *const c_void,
    /// The elements.
    pub data: *mut T,
    /// Elements in use.
    pub len: i32,
    /// Elements allocated.
    pub capacity: i32,
    /// How many elements a growth adds (0: the default).
    pub grow_by: i32,
}

const _: () = {
    assert!(offset_of!(GameArray<u32>, vtable) == 0x0);
    assert!(offset_of!(GameArray<u32>, data) == 0x4);
    assert!(offset_of!(GameArray<u32>, len) == 0x8);
    assert!(offset_of!(GameArray<u32>, capacity) == 0xC);
    assert!(offset_of!(GameArray<u32>, grow_by) == 0x10);
    assert!(size_of::<GameArray<u32>>() == 0x14);
};

impl<T> GameArray<T> {
    /// The elements in use.
    ///
    /// # Safety
    /// `self` must be a constructed array that does not change while the slice lives.
    pub unsafe fn as_slice(&self) -> &[T] {
        match usize::try_from(self.len) {
            Ok(len) if len > 0 && !self.data.is_null() => {
                // SAFETY: a constructed array holds `len` elements at `data`.
                unsafe { core::slice::from_raw_parts(self.data, len) }
            }
            _ => &[],
        }
    }
}

/// An MFC `CList` of pointers, as the sector map's queries fill it: a vtable, then the head, tail, count, free list,
/// blocks and block size (`CSectorMap::FindUnitsInRadius` 0x5C0BD0 appends to it).
#[repr(C)]
#[derive(Debug)]
pub struct PtrList {
    vtable: usize,
    /// The first node.
    pub head: *mut PtrListNode,
    tail: *mut PtrListNode,
    /// The number of nodes.
    pub count: i32,
    free: *mut PtrListNode,
    blocks: *mut c_void,
    block_size: i32,
}

/// A node of a [`PtrList`].
#[repr(C)]
#[derive(Debug)]
pub struct PtrListNode {
    /// The next node, or null.
    pub next: *mut PtrListNode,
    /// The node before, or null.
    pub prev: *mut PtrListNode,
    /// The element.
    pub data: *mut c_void,
}

const _: () = {
    assert!(offset_of!(PtrList, head) == 0x4);
    assert!(offset_of!(PtrList, count) == 0xC);
    assert!(size_of::<PtrList>() == 0x1C);
    assert!(offset_of!(PtrListNode, data) == 0x8);
};

impl PtrList {
    /// An empty list as the game's constructors make one: the `CList` vtable 0x73D454 and a block size of 10.
    pub fn new() -> Self {
        Self {
            vtable: eix_sites::PTR_LIST_CLASS_VTABLE.here(),
            head: core::ptr::null_mut(),
            tail: core::ptr::null_mut(),
            count: 0,
            free: core::ptr::null_mut(),
            blocks: core::ptr::null_mut(),
            block_size: 10,
        }
    }

    /// The elements, head first.
    ///
    /// # Safety
    /// The list's nodes must be live and not change while the result is used.
    pub unsafe fn elements(&self) -> alloc::vec::Vec<*mut c_void> {
        let mut out = alloc::vec::Vec::new();
        let mut node = self.head;
        while !node.is_null() {
            // SAFETY: a live node of this list.
            let n = unsafe { &*node };
            out.push(n.data);
            node = n.next;
        }
        out
    }

    /// Constructs an empty list at `this` that allocates its nodes `block_size` at a time: the `CList` constructor
    /// 0x556D10 (thiscall, `ret 4`), which stores the vtable 0x73EDB0 and the block size at +0x18.
    ///
    /// # Safety
    /// The process must be a supported game.exe; `this` must be writable and hold no constructed list.
    pub unsafe fn construct(this: *mut Self, block_size: i32) {
        // SAFETY: 0x556D10 is `CList* __thiscall (int)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, i32) -> *mut Self>(eix_sites::TEXT_556D10)
        };
        // SAFETY: the caller guarantees the build and the memory.
        unsafe { f(this, block_size) };
    }

    /// Empties the list and frees its node blocks: `PtrList::RemoveAll` 0x556D40 (thiscall, plain `ret`).
    ///
    /// # Safety
    /// The process must be a supported game.exe and `this` a constructed list.
    pub unsafe fn remove_all(this: *mut Self) {
        // SAFETY: 0x556D40 is `void __thiscall ()`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self)>(eix_sites::PTR_LIST_REMOVE_ALL)
        };
        // SAFETY: the caller guarantees the build and the list.
        unsafe { f(this) }
    }

    /// Frees the nodes: `CList::~CList` 0x427830 (thiscall, plain `ret`).
    ///
    /// # Safety
    /// The process must be a supported game.exe; the list must not be used afterwards.
    pub unsafe fn destroy(this: *mut Self) {
        // SAFETY: 0x427830 is `void __thiscall ()`.
        let f = unsafe { code::<unsafe extern "thiscall" fn(*mut Self)>(eix_sites::C_LIST_C_LIST) };
        // SAFETY: the caller guarantees the build and the list.
        unsafe { f(this) }
    }
}

impl Default for PtrList {
    fn default() -> Self {
        Self::new()
    }
}

/// `operator new` 0x7005AF: cdecl, `malloc` with the new-handler retry; null when out of memory.
///
/// # Safety
/// The process must be a supported game.exe. The block is freed only by [`delete`].
pub unsafe fn new(size: usize) -> *mut c_void {
    // SAFETY: 0x7005AF is `void* __cdecl operator new(size_t)` (one stack argument, plain `ret`).
    let f = unsafe { code::<unsafe extern "C" fn(usize) -> *mut c_void>(eix_sites::OPERATOR_NEW) };
    // SAFETY: the caller guarantees the build.
    unsafe { f(size) }
}

/// `operator delete` 0x7005D8: cdecl, `free`.
///
/// # Safety
/// The process must be a supported game.exe and `block` null or from the game's allocator, not freed yet.
pub unsafe fn delete(block: *mut c_void) {
    // SAFETY: 0x7005D8 is `void __cdecl operator delete(void*)` (one stack argument, plain `ret`).
    let f = unsafe { code::<unsafe extern "C" fn(*mut c_void)>(eix_sites::OPERATOR_DELETE) };
    // SAFETY: the caller guarantees the build and the block.
    unsafe { f(block) }
}

/// The MFC `CDC` of the device context `dc`: `CDC::FromHandle` 0x708D79 (stdcall, `ret 4`), a temporary object of the
/// thread's handle map; null for a null `dc`.
///
/// # Safety
/// The process must be a supported game.exe, on the game's thread.
pub unsafe fn cdc_from_handle(dc: *mut c_void) -> *mut c_void {
    // SAFETY: 0x708D79 is `CDC* PASCAL CDC::FromHandle(HDC)`.
    let f = unsafe {
        code::<unsafe extern "system" fn(*mut c_void) -> *mut c_void>(eix_sites::CDC_FROM_HANDLE)
    };
    // SAFETY: the caller guarantees the build and the thread.
    unsafe { f(dc) }
}

/// Constructs a game string at `this` from `text`: `CString::CString(LPCTSTR)` 0x7009DE (thiscall, `ret 4`).
///
/// # Safety
/// The process must be a supported game.exe; `this` must be writable and hold no constructed string.
pub unsafe fn construct(this: *mut GameString, text: &CStr) {
    // SAFETY: 0x7009DE takes `this` in ECX and the text on the stack, and cleans it (`ret 4`).
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut GameString, *const u8) -> *mut GameString>(
            eix_sites::SHIM_TEXT_CONSTRUCT,
        )
    };
    // SAFETY: the caller guarantees the build and `this`; `text` is NUL-terminated.
    unsafe { f(this, text.as_ptr().cast()) };
}

/// Constructs an empty game string at `this`: `CString::CString()` 0x6E7195 (thiscall, plain `ret`) points it at the
/// shared empty string (the characters pointer at 0x796E48).
///
/// # Safety
/// The process must be a supported game.exe; `this` must be writable and hold no constructed string.
pub unsafe fn construct_empty(this: *mut GameString) {
    // SAFETY: 0x6E7195 takes `this` in ECX and nothing on the stack.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut GameString) -> *mut GameString>(
            eix_sites::GET_OWNER__84,
        )
    };
    // SAFETY: the caller guarantees the build and `this`.
    unsafe { f(this) };
}

/// Assigns `text` to the game string at `this`: `CString::operator=(LPCTSTR)` 0x700AF9 (thiscall, `ret 4`).
///
/// # Safety
/// The process must be a supported game.exe and `this` a constructed string.
pub unsafe fn assign(this: *mut GameString, text: &CStr) {
    // SAFETY: 0x700AF9 takes `this` in ECX and the text on the stack, and cleans it (`ret 4`).
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut GameString, *const u8) -> *mut GameString>(
            eix_sites::SHIM_ASSIGN_TEXT,
        )
    };
    // SAFETY: the caller guarantees the build and `this`; `text` is NUL-terminated.
    unsafe { f(this, text.as_ptr().cast()) };
}

/// Appends `text` to the game string at `this`: `CString::operator+=(LPCTSTR)` 0x700D4C (thiscall, `ret 4`), as
/// `CWorldServer::CreateUnits` 0x58FA27 adds a line to the zone's start script.
///
/// # Safety
/// The process must be a supported game.exe and `this` a constructed string.
pub unsafe fn append(this: *mut GameString, text: &CStr) {
    // SAFETY: 0x700D4C takes `this` in ECX and the text on the stack, and cleans it (`ret 4`).
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut GameString, *const u8) -> *mut GameString>(
            eix_sites::SHIM_APPEND_TEXT,
        )
    };
    // SAFETY: the caller guarantees the build and `this`; `text` is NUL-terminated.
    unsafe { f(this, text.as_ptr().cast()) };
}

/// Destroys the game string at `this`: `CString::~CString` 0x700970 (thiscall, plain `ret`).
///
/// # Safety
/// The process must be a supported game.exe and `this` a constructed string, not used afterwards.
pub unsafe fn destroy(this: *mut GameString) {
    // SAFETY: 0x700970 takes `this` in ECX and nothing on the stack.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut GameString)>(eix_sites::C_STRING_C_STRING)
    };
    // SAFETY: the caller guarantees the build and the string.
    unsafe { f(this) }
}

/// One argument of [`format`], as a varargs call passes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FormatArg {
    /// `%d` and kin: one word.
    Int(i32),
    /// `%s`: a NUL-terminated string's address, one word.
    Str(*const u8),
    /// `%f` and kin: a double, two words.
    Double(f64),
}

/// The words `args` take on a varargs call's stack, in order.
fn va_words(args: &[FormatArg]) -> alloc::vec::Vec<u32> {
    let mut words = alloc::vec::Vec::with_capacity(args.len() * 2);
    for a in args {
        match *a {
            FormatArg::Int(v) => words.push(v as u32),
            FormatArg::Str(p) => words.push(p.expose_provenance() as u32),
            FormatArg::Double(v) => {
                let bits = v.to_bits();
                words.push(bits as u32);
                words.push((bits >> 32) as u32);
            }
        }
    }
    words
}

/// Formats `args` by `format` into the game string at `this`, with the game's own printf: `CString::FormatV`
/// 0x6FDC12 (thiscall: the format and a `va_list`, `ret 8`), which `CString::Format` 0x6FDF5F calls.
///
/// # Safety
/// The process must be a supported game.exe, `this` a constructed string, and `args` what `format` asks for (each `Str`
/// a live string), on the game's thread.
pub unsafe fn format(this: *mut GameString, format: &CStr, args: &[FormatArg]) {
    let words = va_words(args);
    // SAFETY: 0x6FDC12 takes `this` in ECX, the format and the argument pointer on the stack, and cleans them.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut GameString, *const u8, *const u32)>(
            eix_sites::C_STRING_FORMAT_V,
        )
    };
    // SAFETY: the caller guarantees the build, the string and the arguments; `words` lives across the call and is
    // laid out as the stack of a varargs call (an MSVC `va_list` is a pointer to it).
    unsafe { f(this, format.as_ptr().cast(), words.as_ptr()) }
}

/// [`format`] into a temporary game string, whose bytes are returned.
///
/// # Safety
/// As for [`format`], without the string.
pub unsafe fn format_bytes(format_text: &CStr, args: &[FormatArg]) -> alloc::vec::Vec<u8> {
    let mut s = core::mem::MaybeUninit::<GameString>::uninit();
    // SAFETY: `s` is writable and holds no string; the caller guarantees the build.
    unsafe { construct_empty(s.as_mut_ptr()) };
    // SAFETY: `s` is now constructed; the caller guarantees the arguments.
    unsafe { format(s.as_mut_ptr(), format_text, args) };
    // SAFETY: `s` is constructed and not mutated while the reference lives.
    let string = unsafe { &*s.as_ptr() };
    // SAFETY: a constructed string holds a valid buffer; the caller guarantees the build.
    let bytes = unsafe { string.as_bytes() }.to_vec();
    // SAFETY: a constructed string, not used again.
    unsafe { destroy(s.as_mut_ptr()) };
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_string_reads_its_length_from_the_header() {
        #[repr(C)]
        struct Block {
            data: StringData,
            chars: [u8; 8],
        }
        let mut block = Block {
            data: StringData {
                refs: 1,
                len: 5,
                capacity: 7,
            },
            chars: *b"hello\0\0\0",
        };
        let s = GameString {
            chars: block.chars.as_mut_ptr(),
        };
        // SAFETY: `s` points just past a header, as a game string does.
        assert_eq!(unsafe { s.as_bytes() }, b"hello");
        // SAFETY: as above.
        assert_eq!(unsafe { *s.data() }, block.data);
    }

    #[test]
    fn varargs_words_split_doubles_low_word_first() {
        let s = c"x".as_ptr().cast::<u8>();
        assert_eq!(
            va_words(&[
                FormatArg::Str(s),
                FormatArg::Int(-1),
                FormatArg::Double(1.5)
            ]),
            [s as u32, 0xFFFF_FFFF, 0, 0x3FF8_0000]
        );
    }

    #[test]
    fn an_array_shows_only_the_elements_in_use() {
        let mut items = [1u32, 2, 3, 0];
        let mut array = GameArray {
            vtable: core::ptr::null(),
            data: items.as_mut_ptr(),
            len: 3,
            capacity: 4,
            grow_by: 0,
        };
        // SAFETY: `data` holds `len` elements.
        assert_eq!(unsafe { array.as_slice() }, &[1, 2, 3]);
        array.len = 0;
        // SAFETY: as above.
        assert!(unsafe { array.as_slice() }.is_empty());
        array.len = -1;
        // SAFETY: a negative count reads as empty.
        assert!(unsafe { array.as_slice() }.is_empty());
    }
}
