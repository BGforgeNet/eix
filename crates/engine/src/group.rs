//! Patch groups: named sets of site writes, applied all or nothing.
#![expect(unsafe_code, reason = "checking and writing game memory")]

use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use eix_sites::Site;

use crate::hook::HookError;
use crate::patch::{self, Kind, PatchError};

/// One write to game memory: the bytes a site must hold and what replaces them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteWrite {
    site: Site,
    expected: Vec<u8>,
    new: Vec<u8>,
    bytes: Space,
}

/// Whose bytes a write holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Space {
    /// RU 1.06's, of this kind, which another build translates.
    Ru(Kind),
    /// The running build's already ([`patch::localize`]); only the site is translated.
    Native,
}

impl SiteWrite {
    /// Raw bytes (code or data) of the same length.
    pub fn bytes(site: Site, expected: &[u8], new: &[u8]) -> Result<Self, PatchError> {
        if expected.len() != new.len() {
            return Err(PatchError::Length {
                site,
                expected: expected.len(),
                new: new.len(),
            });
        }
        Ok(Self {
            site,
            expected: expected.to_vec(),
            new: new.to_vec(),
            bytes: Space::Ru(Kind::Section),
        })
    }

    /// [`bytes`](Self::bytes) whose bytes are the running build's already, as [`patch::localize`] gives them.
    pub fn native(site: Site, expected: &[u8], new: &[u8]) -> Result<Self, PatchError> {
        Ok(Self {
            bytes: Space::Native,
            ..Self::bytes(site, expected, new)?
        })
    }

    /// NOPs over `expected`.
    pub fn nop(site: Site, expected: &[u8]) -> Self {
        Self {
            site,
            expected: expected.to_vec(),
            new: vec![0x90; expected.len()],
            bytes: Space::Ru(Kind::Section),
        }
    }

    /// A `jmp rel32` to `to` over `expected` (at least 5 bytes); the bytes after the jump become NOPs.
    pub fn jmp(site: Site, expected: &[u8], to: usize) -> Result<Self, HookError> {
        Self::branch(site, expected, patch::jmp_rel32(site, to))
    }

    /// A `call rel32` to `to` over `expected` (at least 5 bytes); the bytes after the call become NOPs.
    pub fn call(site: Site, expected: &[u8], to: usize) -> Result<Self, HookError> {
        Self::branch(site, expected, patch::call_rel32(site, to))
    }

    fn branch(site: Site, expected: &[u8], branch: [u8; 5]) -> Result<Self, HookError> {
        if expected.len() < 5 {
            return Err(HookError::TooShort {
                site,
                len: expected.len(),
            });
        }
        let mut new = vec![0x90; expected.len()];
        new[..5].copy_from_slice(&branch);
        Ok(Self {
            site,
            expected: expected.to_vec(),
            new,
            // A branch is code wherever it lies: the original put some into `.data`.
            bytes: Space::Ru(Kind::Code),
        })
    }

    /// A 4-byte pointer (a data pointer, a vtable entry, an import slot): `expected` becomes `new`. A pointer into
    /// game.exe is its RU 1.06 address ([`Site::ru`]), which the write translates.
    pub fn pointer(site: Site, expected: usize, new: usize) -> Self {
        Self {
            site,
            expected: (expected as u32).to_le_bytes().to_vec(),
            new: (new as u32).to_le_bytes().to_vec(),
            bytes: Space::Ru(Kind::Section),
        }
    }

    /// This branch write with every byte after its 5-byte branch set to `byte` instead of a NOP: `int3` (CC) marks
    /// code that no longer runs, and gives a site that ended in NOPs its whole length in a memory diff.
    pub fn padded(mut self, byte: u8) -> Self {
        for b in self.new.iter_mut().skip(5) {
            *b = byte;
        }
        self
    }

    /// The site written.
    pub fn site(&self) -> Site {
        self.site
    }

    /// How many bytes are written.
    pub fn size(&self) -> usize {
        self.new.len()
    }

    /// The bytes the site must hold.
    pub fn expected(&self) -> &[u8] {
        &self.expected
    }

    /// The bytes written over them: what a later group that takes over the site expects there.
    pub fn new_bytes(&self) -> &[u8] {
        &self.new
    }

    /// What the bytes are, for another build's translation (native bytes count as their section's).
    pub fn kind(&self) -> Kind {
        match self.bytes {
            Space::Ru(kind) => kind,
            Space::Native => Kind::Section,
        }
    }

    /// # Safety
    /// As for [`patch::verify`].
    unsafe fn verify(&self) -> Result<(), PatchError> {
        match self.bytes {
            // SAFETY: the caller's guarantee.
            Space::Native => unsafe { patch::verify_native(self.site, &self.expected) },
            // SAFETY: the caller's guarantee.
            Space::Ru(_) => unsafe { patch::verify(self.site, &self.expected) },
        }
    }

    /// Writes `new` over `expected` at the site, in this write's byte space.
    ///
    /// # Safety
    /// As for [`patch::write`].
    unsafe fn write(&self, expected: &[u8], new: &[u8]) -> Result<(), PatchError> {
        match self.bytes {
            // SAFETY: the caller's guarantee.
            Space::Native => unsafe { patch::write_native(self.site, expected, new) },
            // SAFETY: the caller's guarantee.
            Space::Ru(kind) => unsafe { patch::write_as(self.site, kind, expected, new) },
        }
    }
}

/// What applying a patch group did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Every write is in place.
    Applied,
    /// The group is turned off; nothing was prepared or written.
    Disabled,
    /// Nothing is in place, unless `unrestored` lists writes that could not be undone.
    Failed {
        /// Why the group failed.
        error: HookError,
        /// Undoing a write that was already made failed for these.
        unrestored: Vec<PatchError>,
    },
}

/// A patch group's name and outcome, for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The group's name.
    pub name: &'static str,
    /// What applying it did.
    pub outcome: Outcome,
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.outcome {
            Outcome::Applied => write!(f, "{}: applied", self.name),
            Outcome::Disabled => write!(f, "{}: disabled", self.name),
            Outcome::Failed { error, unrestored } => {
                write!(f, "{}: failed: {error}", self.name)?;
                for e in unrestored {
                    write!(f, "; cannot undo: {e}")?;
                }
                Ok(())
            }
        }
    }
}

/// Applies the group `name` if `enabled`: `build` prepares its writes (and their stubs), every site is checked,
/// then every write is made. A failed write undoes the ones before it.
///
/// # Safety
/// Every site must be mapped memory of this process that no other thread runs or writes while the call lasts, and
/// each write must leave code and data the game can run with.
pub unsafe fn apply(
    name: &'static str,
    enabled: bool,
    build: impl FnOnce() -> Result<Vec<SiteWrite>, HookError>,
) -> Report {
    if !enabled {
        return Report {
            name,
            outcome: Outcome::Disabled,
        };
    }
    let result = crate::owners::building(name, build)
        .map_err(|e| (e, Vec::new()))
        .and_then(|writes| {
            // SAFETY: the caller's guarantees, passed on.
            unsafe { write_all(&writes) }?;
            for w in &writes {
                crate::owners::site(w.site.here(), w.new.len(), name);
            }
            Ok(())
        });
    let outcome = match result {
        Ok(()) => Outcome::Applied,
        Err((error, unrestored)) => Outcome::Failed { error, unrestored },
    };
    Report { name, outcome }
}

/// # Safety
/// As [`apply`].
unsafe fn write_all(writes: &[SiteWrite]) -> Result<(), (HookError, Vec<PatchError>)> {
    for w in writes {
        // SAFETY: the caller guarantees the site is mapped and not written meanwhile.
        unsafe { w.verify() }.map_err(|e| (e.into(), Vec::new()))?;
    }
    for (i, w) in writes.iter().enumerate() {
        // SAFETY: as above.
        if let Err(e) = unsafe { w.write(&w.expected, &w.new) } {
            let mut unrestored = Vec::new();
            for done in writes[..i].iter().rev() {
                // SAFETY: as above; this site holds `new` now.
                if let Err(u) = unsafe { done.write(&done.new, &done.expected) } {
                    unrestored.push(u);
                }
            }
            return Err((e.into(), unrestored));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_is_all_or_nothing() {
        let mut a = vec![1u8, 2, 3, 4];
        let mut b = vec![5u8, 6, 7, 8];
        let (sa, sb) = (
            Site::of(a.as_mut_ptr() as usize),
            Site::of(b.as_mut_ptr() as usize),
        );
        // SAFETY: both buffers are ours, mapped and not shared.
        let report = unsafe {
            apply("Mixed", true, || {
                Ok(vec![
                    SiteWrite::nop(sa, &[1, 2, 3, 4]),
                    SiteWrite::bytes(sb, &[5, 6, 0, 8], &[0; 4])?,
                ])
            })
        };
        assert!(matches!(
            report.outcome,
            Outcome::Failed {
                error: HookError::Patch(PatchError::Mismatch { site, .. }),
                ..
            } if site == sb
        ));
        assert_eq!((&a[..], &b[..]), (&[1, 2, 3, 4][..], &[5, 6, 7, 8][..]));

        // SAFETY: as above.
        let report = unsafe {
            apply("Both", true, || {
                Ok(vec![
                    SiteWrite::nop(sa, &[1, 2, 3, 4]),
                    SiteWrite::pointer(sb, 0x0807_0605, 0x7654_3210),
                ])
            })
        };
        assert_eq!(report.to_string(), "Both: applied");
        assert_eq!(
            (&a[..], &b[..]),
            (&[0x90; 4][..], &[0x10, 0x32, 0x54, 0x76][..])
        );
    }

    #[test]
    fn a_failed_write_undoes_the_earlier_ones() {
        let mut a = vec![1u8, 2, 3, 4];
        let site = Site::of(a.as_mut_ptr() as usize);
        // Both match before anything is written, but the first changes what the second expects.
        // SAFETY: the buffer is ours, mapped and not shared.
        let report = unsafe {
            apply("Overlap", true, || {
                Ok(vec![
                    SiteWrite::bytes(site, &[1, 2], &[9, 9])?,
                    SiteWrite::bytes(site.offset(1), &[2, 3], &[0, 0])?,
                ])
            })
        };
        match report.outcome {
            Outcome::Failed {
                error: HookError::Patch(PatchError::Mismatch { found, .. }),
                unrestored,
            } => {
                assert_eq!(found, [9, 3]);
                assert!(unrestored.is_empty());
            }
            other @ (Outcome::Applied | Outcome::Disabled | Outcome::Failed { .. }) => {
                panic!("{other:?}")
            }
        }
        assert_eq!(a, [1, 2, 3, 4]);
    }

    #[test]
    fn a_disabled_group_prepares_nothing() {
        // SAFETY: nothing is written.
        let report = unsafe { apply("Off", false, || panic!("built a disabled group")) };
        assert_eq!(report.to_string(), "Off: disabled");
        let site = Site::new(0x1000);
        assert_eq!(
            SiteWrite::jmp(site, &[0x90; 4], 0x2000),
            Err(HookError::TooShort { site, len: 4 })
        );
        assert_eq!(
            SiteWrite::call(site, &[0; 6], 0x1000).map(|w| w.new),
            Ok(vec![0xE8, 0xFB, 0xFF, 0xFF, 0xFF, 0x90])
        );
    }

    #[test]
    fn a_branch_is_code_wherever_it_lies() {
        let site = eix_sites::DATA_79A5E0;
        assert_eq!(
            SiteWrite::jmp(site, &[0; 5], 0x1000).map(|w| w.kind()),
            Ok(Kind::Code)
        );
        assert_eq!(
            SiteWrite::call(site, &[0; 5], 0x1000).map(|w| w.kind()),
            Ok(Kind::Code)
        );
        assert_eq!(SiteWrite::pointer(site, 0, 1).kind(), Kind::Section);
        assert_eq!(SiteWrite::nop(site, &[0]).kind(), Kind::Section);
    }
}
