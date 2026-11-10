//! The C runtime game.exe links statically, where a patch group must share its state with the game.
#![expect(unsafe_code, reason = "calls into game.exe's C runtime")]

use core::ffi::c_void;

use super::{Global, code};

/// `int __cdecl _tolower_lk(int)`: `tolower` without the locale guard, which its callers (`tolower`, `_stricmp`,
/// `_strnicmp`, `_memicmp`) hold. Under a set LC_CTYPE locale it lowers a byte through `__crtLCMapStringA`.
pub const TOLOWER_LK: usize = eix_sites::TEXT_6EBAC2;

/// The C runtime's LC_CTYPE locale, which `setlocale` 0x6EA234 alone sets. The rest of the state [`TOLOWER_LK`]
/// reads derives from these two: `__init_ctype` 0x6F47F0 builds `_pctype` 0x797DD8 and `__mb_cur_max` 0x797FE4
/// from them (into a fresh allocation on each call, so `_pctype`'s address is no key).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CtypeLocale {
    /// `__lc_handle[LC_CTYPE]` 0x7CABF0: the locale's LCID, 0 in the "C" locale.
    pub handle: u32,
    /// `__lc_codepage` 0x7CAC00: the ANSI code page `__crtLCMapStringA` converts through.
    pub code_page: u32,
}

/// The LC_CTYPE state now.
///
/// # Safety
/// The process must be game.exe RU 1.06, on a thread inside the CRT's locale guard (as a caller of `_tolower_lk`
/// is), so that no `setlocale` changes the state meanwhile.
pub unsafe fn ctype_locale() -> CtypeLocale {
    CtypeLocale {
        // SAFETY: the caller guarantees the build, so the address is that CRT global, and the guard.
        handle: unsafe { Global::<u32>::at(eix_sites::DATA_7CABF0).read() },
        // SAFETY: as above.
        code_page: unsafe { Global::<u32>::at(eix_sites::DATA_7CAC00).read() },
    }
}

/// kernel32's `HeapAlloc` (stdcall `(heap, flags, size)`).
pub type HeapAlloc = unsafe extern "system" fn(*mut c_void, u32, usize) -> *mut c_void;
/// kernel32's `HeapReAlloc` (stdcall `(heap, flags, block, size)`).
pub type HeapReAlloc =
    unsafe extern "system" fn(*mut c_void, u32, *mut c_void, usize) -> *mut c_void;

/// game.exe's import slot of `HeapAlloc` 0x73B360. With the system heap the C runtime calls it for every new block
/// on its heap `_crtheap` 0x7CAF08: `_heap_alloc` 0x6EACF6 (malloc, operator new), calloc 0x6F040D, realloc of a
/// small-block heap's block 0x6ED792 and 0x6ED8FA; the small-block heaps take their own tables from it.
pub const HEAP_ALLOC_SLOT: Global<usize> = Global::at(eix_sites::RDATA_73B360);
/// game.exe's import slot of `HeapReAlloc` 0x73B374: realloc 0x6ED7E9, 0x6ED93B and 0x6ED99B, `_expand` 0x6EEEB2
/// (in place only), the small-block heap 0x6F5D8A.
pub const HEAP_RE_ALLOC_SLOT: Global<usize> = Global::at(eix_sites::RDATA_73B374);
/// `__active_heap` 0x7CAF0C, set by `_heap_init` 0x6F54E6 from `__heap_select` 0x6F539E: 1 the system heap (chosen on
/// Windows NT 5 and later), 2 and 3 the C runtime's own small-block heaps, which serve small blocks without
/// `HeapAlloc`.
pub const ACTIVE_HEAP: Global<u32> = Global::at(eix_sites::DATA_7CAF0C);
/// [`ACTIVE_HEAP`]'s value for the system heap.
pub const SYSTEM_HEAP: u32 = 1;

/// The game's `rand` 0x6E77F7 (cdecl): the next number of the CRT generator the game draws from, 0..=0x7FFF.
///
/// # Safety
/// The process must be game.exe RU 1.06, on the game's thread.
pub unsafe fn rand() -> i32 {
    // SAFETY: 0x6E77F7 is `int __cdecl rand(void)`: it steps the generator and returns bits 16..30.
    let f = unsafe { code::<unsafe extern "C" fn() -> i32>(eix_sites::TEXT_6E77F7) };
    // SAFETY: the caller guarantees the build and the thread.
    unsafe { f() }
}

/// `base ^ exponent` by the game's `_CIpow` 0x6E81A0 (base in ST1, exponent in ST0), under the control word in
/// effect; the result as an `f64` (exact unless the precision is 64 bits).
///
/// # Safety
/// The process must be game.exe RU 1.06, with at most five x87 registers in use.
pub unsafe fn ci_pow(base: f64, exponent: f64) -> f64 {
    let mut out = 0f64;
    // SAFETY: `_CIpow` pops its two operands and leaves the power in ST0; it keeps EDI (the CRT's callee-saved
    // registers), so `out` survives the call.
    unsafe {
        core::arch::asm!(
            "fld qword ptr [{b}]",
            "fld qword ptr [{e}]",
            "call {pow}",
            "fstp qword ptr [edi]",
            b = in(reg) &base,
            e = in(reg) &exponent,
            pow = in(reg) eix_sites::here(eix_sites::TEXT_6E81A0),
            in("edi") &mut out,
            clobber_abi("C"),
        );
    }
    out
}

/// The game's `atof` 0x6E911A (cdecl): the longest leading number of `text`, 0 when there is none.
///
/// # Safety
/// The process must be game.exe RU 1.06, on the game's thread.
pub unsafe fn atof(text: &core::ffi::CStr) -> f64 {
    // SAFETY: 0x6E911A is `double __cdecl atof(const char*)`, the result in ST0.
    let f = unsafe { code::<unsafe extern "C" fn(*const u8) -> f64>(eix_sites::LIBCRT_ATOF) };
    // SAFETY: the caller guarantees the build and the thread; `text` is NUL-terminated.
    unsafe { f(text.as_ptr().cast()) }
}
