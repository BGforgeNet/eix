//! Writes to game memory, checking what is there first.
#![expect(
    unsafe_code,
    reason = "changing page protection and writing game.exe's code"
)]

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::Memory::{
    PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualProtect,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use crate::lock::SpinLock;
use crate::translate;

/// Why a patch was not written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchError {
    /// `expected` and `new` differ in length.
    Length {
        /// The patch site.
        site: usize,
        /// Length of the expected bytes.
        expected: usize,
        /// Length of the new bytes.
        new: usize,
    },
    /// The page protection could not be changed.
    Protect {
        /// The patch site.
        site: usize,
        /// The system's error.
        error: String,
    },
    /// The bytes at the site are not the expected ones; nothing was written.
    Mismatch {
        /// The patch site.
        site: usize,
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

/// Replaces `expected` at `site` with `new`, only if `expected` is what is there.
///
/// # Safety
/// `site..site + expected.len()` must be mapped memory of this process that no other thread runs or writes while
/// the call lasts.
pub unsafe fn write(site: usize, expected: &[u8], new: &[u8]) -> Result<(), PatchError> {
    if expected.len() != new.len() {
        return Err(PatchError::Length {
            site,
            expected: expected.len(),
            new: new.len(),
        });
    }
    // SAFETY: the caller's guarantee, for the site in the running build.
    let (site, expected, new) = unsafe { for_build(site, expected, new) };
    // SAFETY: as above.
    unsafe { write_at(site, &expected, &new) }
}

/// [`write()`] with `expected` and `new` already the running build's bytes ([`localize`] made them): only the site is
/// translated.
///
/// # Safety
/// As for [`write()`].
pub unsafe fn write_native(site: usize, expected: &[u8], new: &[u8]) -> Result<(), PatchError> {
    if expected.len() != new.len() {
        return Err(PatchError::Length {
            site,
            expected: expected.len(),
            new: new.len(),
        });
    }
    // SAFETY: the caller's guarantee.
    unsafe { write_at(eix_sites::here(site), expected, new) }
}

/// The bytes `expected` and `new` for RU 1.06 address `site` as the running build has them: what the site holds when
/// it is `expected` translated (else `expected`), and `new` translated. Both unchanged on RU 1.06.
///
/// # Safety
/// As for [`verify`].
pub unsafe fn localize(site: usize, expected: &[u8], new: &[u8]) -> (Vec<u8>, Vec<u8>) {
    // SAFETY: the caller's guarantee.
    let (_, expected, new) = unsafe { for_build(site, expected, new) };
    (expected.into_owned(), new.into_owned())
}

/// `bytes` written at RU 1.06 address `site`, as the running build holds them once written (code rebased, a data
/// address translated); reads nothing.
pub fn as_built(site: usize, bytes: &[u8]) -> Vec<u8> {
    if eix_sites::translating() {
        translate::translated(site, bytes)
    } else {
        bytes.to_vec()
    }
}

/// # Safety
/// As for [`write()`], for `site` itself.
unsafe fn write_at(site: usize, expected: &[u8], new: &[u8]) -> Result<(), PatchError> {
    let len = new.len();
    let ptr = site as *mut u8;
    // Protection is per page: two writes to different sites on one page must not interleave, or one restores
    // read-only protection while the other is still writing.
    PROTECT.with(|()| {
        let mut old: PAGE_PROTECTION_FLAGS = 0;
        // SAFETY: the caller guarantees the range is mapped; `old` receives the previous protection.
        let ok = unsafe { VirtualProtect(ptr.cast(), len, PAGE_EXECUTE_READWRITE, &mut old) };
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
        // SAFETY: same range; `old` is the protection it had.
        unsafe { VirtualProtect(ptr.cast(), len, old, &mut ignored) };
        // SAFETY: returns a pseudo-handle to our own process; no preconditions.
        let process = unsafe { GetCurrentProcess() };
        // SAFETY: flushing a range of our own process is always allowed.
        unsafe { FlushInstructionCache(process, ptr.cast(), len) };
        result
    })
}

static PROTECT: SpinLock<()> = SpinLock::new(());

/// An RU 1.06 site and the bytes expected and written there, for the running build: the site translated, the
/// expected bytes those found when they are the RU bytes translated, the written bytes translated.
///
/// # Safety
/// The translated range must be mapped, readable memory that no other thread writes meanwhile.
unsafe fn for_build<'a>(
    site: usize,
    expected: &'a [u8],
    new: &'a [u8],
) -> (usize, Cow<'a, [u8]>, Cow<'a, [u8]>) {
    if !eix_sites::translating() {
        return (site, Cow::Borrowed(expected), Cow::Borrowed(new));
    }
    let at = eix_sites::here(site);
    // SAFETY: the caller's guarantee.
    let found = unsafe { core::slice::from_raw_parts(at as *const u8, expected.len()) }.to_vec();
    // SAFETY: the caller's guarantee; a site in the image has its neighbours mapped.
    let matched = unsafe { translate::matches(site, expected, &found) };
    let new = translate::written(site, expected, matched.then_some(&found[..]), new);
    let expected = if matched { found } else { expected.to_vec() };
    (at, Cow::Owned(expected), Cow::Owned(new))
}

/// Checks that `expected` is what is at `site`, without writing.
///
/// # Safety
/// `site..site + expected.len()` must be mapped, readable memory of this process that no other thread writes while
/// the call lasts.
pub unsafe fn verify(site: usize, expected: &[u8]) -> Result<(), PatchError> {
    // SAFETY: the caller's guarantee, for the site in the running build.
    let (site, expected, _) = unsafe { for_build(site, expected, expected) };
    // SAFETY: as above.
    unsafe { verify_at(site, &expected) }
}

/// [`verify`] with `expected` already the running build's bytes: only the site is translated.
///
/// # Safety
/// As for [`verify`].
pub unsafe fn verify_native(site: usize, expected: &[u8]) -> Result<(), PatchError> {
    // SAFETY: the caller's guarantee.
    unsafe { verify_at(eix_sites::here(site), expected) }
}

/// # Safety
/// As for [`verify`], for `site` itself.
unsafe fn verify_at(site: usize, expected: &[u8]) -> Result<(), PatchError> {
    // SAFETY: the caller guarantees the range is mapped, readable and not written meanwhile.
    let found = unsafe { core::slice::from_raw_parts(site as *const u8, expected.len()) };
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
pub unsafe fn read(site: usize, len: usize) -> Vec<u8> {
    // SAFETY: the caller guarantees the range is mapped, readable and not written meanwhile.
    unsafe { core::slice::from_raw_parts(eix_sites::here(site) as *const u8, len) }.to_vec()
}

/// Makes `start..start + len` readable, writable and executable, and leaves it so; nothing is written.
///
/// # Safety
/// The range must be mapped memory of this process.
pub unsafe fn unprotect(start: usize, len: usize) -> Result<(), PatchError> {
    let mut old: PAGE_PROTECTION_FLAGS = 0;
    // SAFETY: the caller guarantees the range is mapped; `old` receives the previous protection.
    let ok = unsafe { VirtualProtect(start as *mut _, len, PAGE_EXECUTE_READWRITE, &mut old) };
    if ok == 0 {
        return Err(PatchError::Protect {
            site: start,
            error: crate::win::os_error("VirtualProtect"),
        });
    }
    Ok(())
}

/// The bytes of a 5-byte `jmp rel32` at `from` to `to`.
pub fn jmp_rel32(from: usize, to: usize) -> [u8; 5] {
    rel32(0xE9, from, to)
}

/// The bytes of a 5-byte `call rel32` at `from` to `to`.
pub fn call_rel32(from: usize, to: usize) -> [u8; 5] {
    rel32(0xE8, from, to)
}

fn rel32(opcode: u8, from: usize, to: usize) -> [u8; 5] {
    let rel = (to as u32).wrapping_sub(from as u32).wrapping_sub(5);
    let mut b = [opcode, 0, 0, 0, 0];
    b[1..].copy_from_slice(&rel.to_le_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jmp_forward_and_back() {
        assert_eq!(
            jmp_rel32(eix_sites::ENTRY, eix_sites::ENTRY + 5),
            [0xE9, 0, 0, 0, 0]
        );
        assert_eq!(jmp_rel32(0x1000, 0x800), [0xE9, 0xFB, 0xF7, 0xFF, 0xFF]);
        assert_eq!(call_rel32(0x1000, 0x800), [0xE8, 0xFB, 0xF7, 0xFF, 0xFF]);
    }

    #[test]
    fn verifies_without_writing() {
        let buf = [0x55u8, 0x8B, 0xEC];
        let site = buf.as_ptr() as usize;
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
        let site = buf.as_mut_ptr() as usize;
        // SAFETY: `buf` is ours, mapped and not shared.
        let wrong = unsafe { write(site, &[1, 2, 3, 4, 5], &[9; 5]) };
        assert!(matches!(wrong, Err(PatchError::Mismatch { .. })));
        assert_eq!(buf, [0x55, 0x8B, 0xEC, 0x6A, 0xFF]);
        // SAFETY: as above.
        unsafe { write(site, &[0x55, 0x8B, 0xEC, 0x6A, 0xFF], &[0x90; 5]) }.unwrap();
        assert_eq!(buf, [0x90; 5]);
    }
}
