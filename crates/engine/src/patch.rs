//! Writes to game memory, checking what is there first.
#![expect(
    unsafe_code,
    reason = "changing page protection and writing game.exe's code"
)]

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::fmt;

use windows_sys::Win32::System::Diagnostics::Debug::{FlushInstructionCache, OutputDebugStringW};
use windows_sys::Win32::System::Memory::{
    PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualProtect,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use eix_sites::Site;

use crate::lock::SpinLock;
use crate::translate;

/// Why a patch was not written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchError {
    /// `expected` and `new` differ in length.
    Length {
        /// The patch site.
        site: Site,
        /// Length of the expected bytes.
        expected: usize,
        /// Length of the new bytes.
        new: usize,
    },
    /// The page protection could not be changed.
    Protect {
        /// The patch site.
        site: Site,
        /// The system's error.
        error: String,
    },
    /// The bytes at the site are not the expected ones; nothing was written.
    Mismatch {
        /// The patch site.
        site: Site,
        /// What the patch expects.
        expected: Vec<u8>,
        /// What is there.
        found: Vec<u8>,
    },
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length {
                site,
                expected,
                new,
            } => write!(
                f,
                "patch at {site:#x}: {expected} expected bytes but {new} new ones"
            ),
            Self::Protect { site, error } => {
                write!(f, "patch at {site:#x}: cannot make it writable: {error}")
            }
            Self::Mismatch {
                site,
                expected,
                found,
            } => write!(
                f,
                "patch at {site:#x}: expected {} but found {}",
                hex(expected),
                hex(found)
            ),
        }
    }
}

impl core::error::Error for PatchError {}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// What bytes written at a site are, for another build: the bytes there translate as their kind says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// What the site's section holds: code in `.text`, data elsewhere (a data site's 4-byte address translates).
    Section,
    /// Code wherever it lies: a branch the original put into `.data` is rebased as code.
    Code,
    /// Data wherever it lies: a pointer in a jump table of `.text` translates as an address.
    Data,
    /// Bytes that hold no address (text, a constant), the same in every build: nothing in them translates.
    Plain,
}

/// Replaces `expected` at `site` with `new`, only if `expected` is what is there.
///
/// # Safety
/// `site..site + expected.len()` must be mapped memory of this process that no other thread runs or writes while
/// the call lasts.
pub unsafe fn write(site: Site, expected: &[u8], new: &[u8]) -> Result<(), PatchError> {
    // SAFETY: the caller's guarantee.
    unsafe { write_as(site, Kind::Section, expected, new) }
}

/// [`write()`] of bytes of `kind`.
///
/// # Safety
/// As for [`write()`].
pub unsafe fn write_as(
    site: Site,
    kind: Kind,
    expected: &[u8],
    new: &[u8],
) -> Result<(), PatchError> {
    if expected.len() != new.len() {
        return Err(PatchError::Length {
            site,
            expected: expected.len(),
            new: new.len(),
        });
    }
    // SAFETY: the caller's guarantee, for the site in the running build.
    let (expected, new) = unsafe { for_build(site, kind, expected, new) };
    // SAFETY: as above.
    unsafe { write_at(site, &expected, &new) }
}

/// [`write()`] with `expected` and `new` already the running build's bytes ([`localize`] made them): only the site is
/// translated.
///
/// # Safety
/// As for [`write()`].
pub unsafe fn write_native(site: Site, expected: &[u8], new: &[u8]) -> Result<(), PatchError> {
    if expected.len() != new.len() {
        return Err(PatchError::Length {
            site,
            expected: expected.len(),
            new: new.len(),
        });
    }
    // SAFETY: the caller's guarantee.
    unsafe { write_at(site, expected, new) }
}

/// The bytes `expected` and `new` (of `kind`) for RU 1.06 address `site` as the running build has them: what the site
/// holds when it is `expected` translated (else `expected`), and `new` translated. Both unchanged on RU 1.06.
///
/// # Safety
/// As for [`verify`].
pub unsafe fn localize(site: Site, kind: Kind, expected: &[u8], new: &[u8]) -> (Vec<u8>, Vec<u8>) {
    // SAFETY: the caller's guarantee.
    let (expected, new) = unsafe { for_build(site, kind, expected, new) };
    (expected.into_owned(), new.into_owned())
}

/// [`localize`] against `found` instead of what the site holds now: a merge of writes not yet made weighs each one
/// against the bytes the earlier ones leave. `found` is the running build's bytes, as long as `expected`.
///
/// # Safety
/// As for [`verify`] (the bytes around the site are read for a field it cuts).
pub unsafe fn localize_against(
    site: Site,
    kind: Kind,
    expected: &[u8],
    new: &[u8],
    found: &[u8],
) -> (Vec<u8>, Vec<u8>) {
    if !eix_sites::translating() {
        return (expected.to_vec(), new.to_vec());
    }
    // SAFETY: the caller's guarantee.
    unsafe { against(site, kind, expected, new, found.to_vec()) }
}

/// `bytes` of `kind` written at RU 1.06 address `site`, as the running build holds them once written (code rebased,
/// a data address translated); reads nothing.
pub fn as_built(site: Site, kind: Kind, bytes: &[u8]) -> Vec<u8> {
    if eix_sites::translating() {
        translate::translated(site, bytes, kind)
    } else {
        bytes.to_vec()
    }
}

/// Whether bytes of `kind` at `site` are code, which another build gets rebased.
pub fn is_code(site: Site, kind: Kind) -> bool {
    translate::is_code(site.ru(), kind)
}

/// # Safety
/// As for [`write()`], for `site` itself.
unsafe fn write_at(site: Site, expected: &[u8], new: &[u8]) -> Result<(), PatchError> {
    let protect = |ptr, len, protection, old| {
        // SAFETY: the writer supplies the caller's mapped range and its local protection word.
        unsafe { VirtualProtect(ptr, len, protection, old) }
    };
    // SAFETY: the caller guarantees the range; `protect` forwards the protection change to VirtualProtect.
    unsafe { write_at_using(site, expected, new, protect, protection_warning) }
}

/// # Safety
/// As for [`write_at`]; `protect` must set the range's protection and return its old protection like VirtualProtect.
unsafe fn write_at_using(
    site: Site,
    expected: &[u8],
    new: &[u8],
    mut protect: impl FnMut(
        *const c_void,
        usize,
        PAGE_PROTECTION_FLAGS,
        *mut PAGE_PROTECTION_FLAGS,
    ) -> i32,
    warn: impl FnOnce(Site, &str),
) -> Result<(), PatchError> {
    let len = new.len();
    let ptr = site.ptr::<u8>();
    // Protection is per page: two writes to different sites on one page must not interleave, or one restores
    // read-only protection while the other is still writing.
    PROTECT.with(|()| {
        let mut old: PAGE_PROTECTION_FLAGS = 0;
        let ok = protect(ptr.cast(), len, PAGE_EXECUTE_READWRITE, &raw mut old);
        if ok == 0 {
            return Err(PatchError::Protect {
                site,
                error: crate::win::os_error("VirtualProtect"),
            });
        }
        // SAFETY: the range is mapped and now readable; no other thread writes it (caller's guarantee).
        let found = unsafe { core::slice::from_raw_parts(ptr, len) }.to_vec();
        let result = if found == expected {
            // SAFETY: the range is mapped, writable now, and `new` has its length.
            unsafe { core::ptr::copy_nonoverlapping(new.as_ptr(), ptr, len) };
            Ok(())
        } else {
            Err(PatchError::Mismatch {
                site,
                expected: expected.to_vec(),
                found,
            })
        };
        let mut ignored: PAGE_PROTECTION_FLAGS = 0;
        // Restoring the protection only narrows access again; the patch stands either way.
        if protect(ptr.cast(), len, old, &raw mut ignored) == 0 {
            warn(site, &crate::win::os_error("VirtualProtect restore"));
        }
        // SAFETY: returns a pseudo-handle to our own process; no preconditions.
        let process = unsafe { GetCurrentProcess() };
        // SAFETY: flushing a range of our own process is always allowed.
        unsafe { FlushInstructionCache(process, ptr.cast(), len) };
        result
    })
}

static PROTECT: SpinLock<()> = SpinLock::new(());

static WARNING: SpinLock<fn(Site, &str)> = SpinLock::new(debug_warning);

/// Sets where a failed page-protection restore is reported. The patch remains in place. Before a handler is set,
/// warnings go to the Windows debugger.
/// The handler must not call the patch writer: it runs while the page-protection lock is held.
pub fn set_warning_handler(handler: fn(Site, &str)) {
    WARNING.with(|current| *current = handler);
}

fn protection_warning(site: Site, error: &str) {
    let handler = WARNING.with(|current| *current);
    handler(site, error);
}

fn debug_warning(site: Site, error: &str) {
    let text = crate::win::wide(&format!(
        "patch at {site:#x}: cannot restore page protection: {error}"
    ));
    // SAFETY: the text is NUL-terminated and lives through the call.
    unsafe { OutputDebugStringW(text.as_ptr()) };
}

/// An RU 1.06 site and the bytes expected and written there, for the running build: the site translated, the
/// expected bytes those found when they are the RU bytes translated, the written bytes translated.
///
/// # Safety
/// The translated range must be mapped, readable memory that no other thread writes meanwhile.
unsafe fn for_build<'a>(
    site: Site,
    kind: Kind,
    expected: &'a [u8],
    new: &'a [u8],
) -> (Cow<'a, [u8]>, Cow<'a, [u8]>) {
    if !eix_sites::translating() {
        return (Cow::Borrowed(expected), Cow::Borrowed(new));
    }
    // SAFETY: the caller's guarantee.
    let found = unsafe { core::slice::from_raw_parts(site.ptr::<u8>(), expected.len()) }.to_vec();
    // SAFETY: as above.
    let (expected, new) = unsafe { against(site, kind, expected, new, found) };
    (Cow::Owned(expected), Cow::Owned(new))
}

/// [`for_build`] with `found` given: what the site holds, or will once earlier writes not yet made land.
///
/// # Safety
/// As for [`translate::matches`].
unsafe fn against(
    site: Site,
    kind: Kind,
    expected: &[u8],
    new: &[u8],
    found: Vec<u8>,
) -> (Vec<u8>, Vec<u8>) {
    // SAFETY: the caller's guarantee; a site in the image has its neighbours mapped.
    let matched = unsafe { translate::matches(site, expected, &found) };
    let new = translate::written(site, expected, matched.then_some(&found[..]), new, kind);
    // Where the bytes match only in part (an earlier write holds the rest), the fields that translate are the build's;
    // the rest stays RU's, for the caller to judge.
    let expected = if matched {
        found
    } else {
        translate::localized(site, expected, &found)
    };
    (expected, new)
}

/// Checks that `expected` is what is at `site`, without writing.
///
/// # Safety
/// `site..site + expected.len()` must be mapped, readable memory of this process that no other thread writes while
/// the call lasts.
pub unsafe fn verify(site: Site, expected: &[u8]) -> Result<(), PatchError> {
    // SAFETY: the caller's guarantee, for the site in the running build.
    // The expected bytes are matched without decoding, so their kind does not matter.
    let (expected, _) = unsafe { for_build(site, Kind::Section, expected, expected) };
    // SAFETY: as above.
    unsafe { verify_at(site, &expected) }
}

/// [`verify`] with `expected` already the running build's bytes: only the site is translated.
///
/// # Safety
/// As for [`verify`].
pub unsafe fn verify_native(site: Site, expected: &[u8]) -> Result<(), PatchError> {
    // SAFETY: the caller's guarantee.
    unsafe { verify_at(site, expected) }
}

/// # Safety
/// As for [`verify`], for `site` itself.
unsafe fn verify_at(site: Site, expected: &[u8]) -> Result<(), PatchError> {
    // SAFETY: the caller guarantees the range is mapped, readable and not written meanwhile.
    let found = unsafe { core::slice::from_raw_parts(site.ptr::<u8>(), expected.len()) };
    if found == expected {
        Ok(())
    } else {
        Err(PatchError::Mismatch {
            site,
            expected: expected.to_vec(),
            found: found.to_vec(),
        })
    }
}

/// The `len` bytes at RU 1.06 address `site`, in the running build.
///
/// # Safety
/// `site..site + len` must be mapped, readable memory of this process that no other thread writes while the call
/// lasts.
pub unsafe fn read(site: Site, len: usize) -> Vec<u8> {
    // SAFETY: the caller guarantees the range is mapped, readable and not written meanwhile.
    unsafe { core::slice::from_raw_parts(site.ptr::<u8>(), len) }.to_vec()
}

/// Makes `start..start + len` readable, writable and executable, and leaves it so; nothing is written.
///
/// # Safety
/// The range must be mapped memory of this process.
pub unsafe fn unprotect(start: Site, len: usize) -> Result<(), PatchError> {
    let mut old: PAGE_PROTECTION_FLAGS = 0;
    // SAFETY: the caller guarantees the range is mapped; `old` receives the previous protection.
    let ok = unsafe { VirtualProtect(start.ptr(), len, PAGE_EXECUTE_READWRITE, &mut old) };
    if ok == 0 {
        return Err(PatchError::Protect {
            site: start,
            error: crate::win::os_error("VirtualProtect"),
        });
    }
    Ok(())
}

/// The bytes of a 5-byte `jmp rel32` at `from` to `to`.
pub fn jmp_rel32(from: Site, to: usize) -> [u8; 5] {
    rel32(0xE9, from, to)
}

/// The bytes of a 5-byte `call rel32` at `from` to `to`.
pub fn call_rel32(from: Site, to: usize) -> [u8; 5] {
    rel32(0xE8, from, to)
}

fn rel32(opcode: u8, from: Site, to: usize) -> [u8; 5] {
    let rel = (to as u32).wrapping_sub(from.ru32()).wrapping_sub(5);
    let mut b = [opcode, 0, 0, 0, 0];
    b[1..].copy_from_slice(&rel.to_le_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restore_failure_is_reported_without_undoing_the_write() {
        let mut buf = [1u8];
        let site = Site::of(buf.as_mut_ptr() as usize);
        let mut calls = 0;
        let mut warning = None;
        let protect = |_, _, _, old: *mut PAGE_PROTECTION_FLAGS| {
            calls += 1;
            // SAFETY: the writer passes its local protection word.
            unsafe { old.write(PAGE_EXECUTE_READWRITE) };
            i32::from(calls == 1)
        };
        // SAFETY: the buffer belongs to this test and is writable; the substitute changes no page protection.
        let result = unsafe {
            write_at_using(site, &[1], &[2], protect, |at, error| {
                warning = Some((at, error.to_owned()))
            })
        };
        assert_eq!(result, Ok(()));
        assert_eq!(buf, [2]);
        assert_eq!(warning.as_ref().map(|(at, _)| *at), Some(site));
        assert!(
            warning
                .unwrap()
                .1
                .starts_with("VirtualProtect restore: OS error ")
        );
    }

    #[test]
    fn jmp_forward_and_back() {
        assert_eq!(
            jmp_rel32(eix_sites::ENTRY, eix_sites::ENTRY.offset(5).ru()),
            [0xE9, 0, 0, 0, 0]
        );
        let from = Site::new(0x1000);
        assert_eq!(jmp_rel32(from, 0x800), [0xE9, 0xFB, 0xF7, 0xFF, 0xFF]);
        assert_eq!(call_rel32(from, 0x800), [0xE8, 0xFB, 0xF7, 0xFF, 0xFF]);
    }

    #[test]
    fn verifies_without_writing() {
        let buf = [0x55u8, 0x8B, 0xEC];
        let site = Site::of(buf.as_ptr() as usize);
        // SAFETY: `buf` is ours, mapped and not shared.
        assert_eq!(unsafe { verify(site, &[0x55, 0x8B, 0xEC]) }, Ok(()));
        // SAFETY: as above.
        let wrong = unsafe { verify(site, &[0x55, 0x8B, 0xED]) };
        assert_eq!(
            wrong,
            Err(PatchError::Mismatch {
                site,
                expected: vec![0x55, 0x8B, 0xED],
                found: vec![0x55, 0x8B, 0xEC]
            })
        );
    }

    #[test]
    fn writes_only_over_expected_bytes() {
        let mut buf = vec![0x55u8, 0x8B, 0xEC, 0x6A, 0xFF];
        let site = Site::of(buf.as_mut_ptr() as usize);
        // SAFETY: `buf` is ours, mapped and not shared.
        let wrong = unsafe { write(site, &[1, 2, 3, 4, 5], &[9; 5]) };
        assert!(matches!(wrong, Err(PatchError::Mismatch { .. })));
        assert_eq!(buf, [0x55, 0x8B, 0xEC, 0x6A, 0xFF]);
        // SAFETY: as above.
        unsafe { write(site, &[0x55, 0x8B, 0xEC, 0x6A, 0xFF], &[0x90; 5]) }.unwrap();
        assert_eq!(buf, [0x90; 5]);
    }
}
