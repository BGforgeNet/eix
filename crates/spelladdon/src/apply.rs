//! The writes SpellAddon makes when the sound system starts: game.exe's image made writable as the original leaves
//! it, the features' writes merged in order and applied, and the filter that keeps Miles from loading a
//! separate SpellAddon.
#![expect(
    unsafe_code,
    reason = "reading and writing game.exe's image and mss32.dll's import slot at the sound system's start"
)]

use std::collections::BTreeMap;
use std::ffi::{CStr, c_char, c_void};
use std::ops::Range;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use eix_engine::group::{self, Outcome, Report, SiteWrite};
use eix_engine::patch::{self, Kind};
use eix_engine::{hook, imports};
use eix_sites::Site;

use crate::{Level, Log};

/// game.exe RU 1.06 from its headers to the end of `.data`, where the original's added code and patches lie; `.rsrc`
/// is never written.
pub(crate) const IMAGE: Range<usize> = 0x40_0000..eix_sites::RSRC_SECTION.ru();

/// Makes [`IMAGE`] readable, writable and executable for good, as the original's start does: its run-time code
/// writes into game.exe's code (the speedrun timer's word, for one).
pub(crate) fn unprotect_image() -> Result<(), String> {
    // SAFETY: the range is game.exe's mapped image (the addon checked the build before installing the hook).
    unsafe { patch::unprotect(Site::new(IMAGE.start), IMAGE.end - IMAGE.start) }
        .map_err(|e| e.to_string())
}

/// The bytes of game.exe the features change: for each RU 1.06 address, what game.exe held and what the last
/// feature to write it wrote, in the running build's bytes. The merge's byte arithmetic runs on RU 1.06 addresses.
#[derive(Debug)]
pub(crate) struct Merge {
    bytes: BTreeMap<usize, (u8, u8)>,
    addon: crate::Earlier,
}

impl Default for Merge {
    fn default() -> Self {
        Self::new(|_| None)
    }
}

/// How far before a SpellAddon write an addon.dll write that reaches into it may start: longer than any of the
/// addon's site writes.
const ADDON_WRITE_REACH: usize = 64;

/// A byte range of game.exe as `(site, length)`.
type Span = (usize, usize);

/// The addon.dll writes that `site .. site + base.len()` meets, as `(site, length)` byte ranges the range may write
/// over: first those it takes over (each lies wholly inside the range), then those it covers only in part with the
/// very bytes the addon wrote there, which stay the addon's. Each still holds what the addon wrote (`base` is what
/// the range holds now, `new` what it writes). The original's memcpy lands over an addon.dll site its block shares;
/// a part covered with other bytes would leave a broken branch, and is refused.
fn addon_writes_covered(
    site: usize,
    base: &[u8],
    new: &[u8],
    addon: crate::Earlier,
) -> Result<(Vec<Span>, Vec<Span>), String> {
    let end = site + base.len();
    let (mut taken, mut shared) = (Vec::new(), Vec::new());
    for at in site.saturating_sub(ADDON_WRITE_REACH)..end {
        // addon.dll's writes the merge weighs lie in `.text`, where the section says code.
        let Some(written) =
            addon(Site::new(at)).map(|w| patch::as_built(Site::new(at), Kind::Section, &w))
        else {
            continue;
        };
        if at + written.len() <= site {
            continue;
        }
        let (from, to) = (at.max(site), (at + written.len()).min(end));
        let held = base[from - site..to - site] == written[from - at..to - at];
        if from == at && to == at + written.len() {
            if held {
                taken.push((at, written.len()));
            }
        } else if held && new[from - site..to - site] == written[from - at..to - at] {
            // aggrmod 6's rewrite ends in the NOPs DeagrPlayerOnLoose writes over the same `je` (0x5cacfc).
            shared.push((from, to - from));
        } else {
            return Err(format!(
                "patch at {site:#x}+{}: covers only part of addon.dll's write at {at:#x}+{}",
                base.len(),
                written.len()
            ));
        }
    }
    Ok((taken, shared))
}

/// One run of back-to-back writes: its RU 1.06 site, expected and new bytes, and their kind (code when a piece of it
/// is).
type Run = (usize, Vec<u8>, Vec<u8>, Kind);

/// The runs of back-to-back writes: a feature that NOPs the first bytes of a replaced instruction and jumps from the
/// rest writes two pieces, and addon.dll's write over the same instruction must be weighed against both together.
fn contiguous_runs(writes: &[SiteWrite]) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for w in writes {
        match runs.last_mut() {
            Some((site, expected, new, kind)) if *site + expected.len() == w.site().ru() => {
                expected.extend_from_slice(w.expected());
                new.extend_from_slice(w.new_bytes());
                if w.kind() == Kind::Code {
                    *kind = Kind::Code;
                }
            }
            _ => runs.push((
                w.site().ru(),
                w.expected().to_vec(),
                w.new_bytes().to_vec(),
                w.kind(),
            )),
        }
    }
    runs
}

impl Merge {
    /// No writes yet; `addon` names addon.dll's writes, which SpellAddon's own may take over.
    pub(crate) fn new(addon: crate::Earlier) -> Self {
        Self {
            bytes: BTreeMap::new(),
            addon,
        }
    }

    /// Adds one feature's writes, in order, and returns the addon.dll sites they take over. Each byte must hold what
    /// it expects, either in game.exe or as an earlier write left it, so a later feature wins on shared bytes as in
    /// the original; a write that covers an addon.dll write wholly lands over it, as the original's copy does, and
    /// one that covers part of it with the addon's own bytes leaves that part to the addon; otherwise nothing of the
    /// feature is kept.
    pub(crate) fn add(&mut self, writes: &[SiteWrite]) -> Result<Vec<Site>, String> {
        let mut bytes = self.bytes.clone();
        let mut taken = Vec::new();
        for (site, expected_run, new_run, kind) in contiguous_runs(writes) {
            if site < IMAGE.start || site + expected_run.len() > IMAGE.end {
                return Err(format!(
                    "{site:#x}+{} is outside game.exe's patchable image",
                    expected_run.len()
                ));
            }
            // SAFETY: the range lies in game.exe's image (checked above), which is mapped and readable; the game
            // writes no code while its sound system starts.
            let base = unsafe { patch::read(Site::new(site), expected_run.len()) };
            // The merge compares and keeps the running build's bytes; each address stays RU 1.06's.
            // SAFETY: as above.
            let (expected_run, new_run) =
                unsafe { patch::localize(Site::new(site), kind, &expected_run, &new_run) };
            let mismatch = expected_run
                .iter()
                .enumerate()
                .any(|(i, &e)| e != base[i] && bytes.get(&(site + i)).map(|&(_, n)| n) != Some(e));
            let (covered, shared) = if mismatch {
                addon_writes_covered(site, &base, &new_run, self.addon)?
            } else {
                (Vec::new(), Vec::new())
            };
            taken.extend(covered.iter().map(|&(at, _)| Site::new(at)));
            for (i, (&expected, &new)) in expected_run.iter().zip(&new_run).enumerate() {
                let at = site + i;
                if shared.iter().any(|&(s, n)| (s..s + n).contains(&at)) {
                    // The addon's own bytes, which the original's copy writes again unchanged.
                    continue;
                }
                let now = bytes.get(&at).map(|&(_, n)| n);
                let addon = covered.iter().any(|&(s, n)| (s..s + n).contains(&at));
                if expected != base[i] && now != Some(expected) && !addon {
                    return Err(format!(
                        "patch at {at:#x}: expected {expected:02X} but found {:02X}",
                        now.unwrap_or(base[i])
                    ));
                }
                bytes
                    .entry(at)
                    .and_modify(|e| e.1 = new)
                    .or_insert((base[i], new));
            }
        }
        self.bytes = bytes;
        Ok(taken)
    }

    /// The merged writes: one per run of consecutive changed addresses, in the running build's bytes.
    fn writes(&self) -> Result<Vec<SiteWrite>, patch::PatchError> {
        let mut writes = Vec::new();
        let mut run: Option<(usize, Vec<u8>, Vec<u8>)> = None;
        for (&at, &(was, now)) in &self.bytes {
            match &mut run {
                Some((start, old, new)) if *start + old.len() == at => {
                    old.push(was);
                    new.push(now);
                }
                _ => {
                    if let Some((start, old, new)) = run.take() {
                        writes.push(SiteWrite::native(Site::new(start), &old, &new)?);
                    }
                    run = Some((at, vec![was], vec![now]));
                }
            }
        }
        if let Some((start, old, new)) = run {
            writes.push(SiteWrite::native(Site::new(start), &old, &new)?);
        }
        Ok(writes)
    }

    /// Writes everything, all or nothing.
    pub(crate) fn apply(&self) -> Report {
        // SAFETY: every write lies in game.exe's image (checked by `add`) and expects the bytes read there; this runs
        // on the game's main thread in the sound manager's constructor, where the original made the same writes, and
        // nothing else writes these sites meanwhile.
        unsafe {
            group::apply("SpellAddon", true, || {
                self.writes().map_err(hook::HookError::from)
            })
        }
    }
}

type LoadLibraryA = unsafe extern "system" fn(*const c_char) -> *mut c_void;

/// mss32.dll's `LoadLibraryA`, which the filter calls for every other file.
static REAL_LOAD_LIBRARY: AtomicUsize = AtomicUsize::new(0);
static LOG: OnceLock<Log> = OnceLock::new();

/// mss32.dll's `LoadLibraryA` with `SpellAddon*.asi` left out: those fail as the original module's own load does
/// (its `DllMain` returns FALSE), and Miles goes on with the next file.
extern "system" fn load_library_a(name: *const c_char) -> *mut c_void {
    if !name.is_null() {
        // SAFETY: LoadLibraryA's argument is a NUL-terminated file name.
        let path = unsafe { CStr::from_ptr(name) }.to_bytes();
        if crate::is_separate_spelladdon(path) {
            if let Some(log) = LOG.get() {
                log(
                    Level::Warn,
                    "spelladdon",
                    "kept from loading: the built-in SpellAddon replaces it",
                    &String::from_utf8_lossy(path),
                );
            }
            return core::ptr::null_mut();
        }
    }
    // SAFETY: the slot held kernel32's LoadLibraryA (or a hook of it) of this type before it was redirected here.
    let real: LoadLibraryA = unsafe { hook::as_fn_at(REAL_LOAD_LIBRARY.load(Ordering::SeqCst)) };
    // SAFETY: the caller's argument, passed on unchanged.
    unsafe { real(name) }
}

/// Points mss32.dll's import of `LoadLibraryA` at [`load_library_a`].
pub(crate) fn block_separate_spelladdon(log: Log) -> Result<(), String> {
    let _ = LOG.set(log);
    let (slot, real) = imports::slot("mss32.dll", "kernel32.dll", c"LoadLibraryA")?
        .ok_or("mss32.dll imports no LoadLibraryA")?;
    REAL_LOAD_LIBRARY.store(real, Ordering::SeqCst);
    let write = SiteWrite::pointer(Site::of(slot), real, load_library_a as *const () as usize);
    // SAFETY: the slot is mss32.dll's import slot, mapped and holding `real`; it is read by mss32.dll's own code on
    // this thread, which is about to call AIL_startup and has not yet.
    let report = unsafe { group::apply("SpellAddon .asi filter", true, || Ok(vec![write])) };
    match report.outcome {
        Outcome::Applied => Ok(()),
        Outcome::Disabled | Outcome::Failed { .. } => Err(report.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// game.exe is not loaded in a test, so merging is checked on the run-forming step alone.
    #[test]
    fn merged_bytes_form_one_write_per_run() {
        let mut m = Merge::default();
        m.bytes.insert(eix_sites::TEXT_500000.ru(), (1, 2));
        m.bytes.insert(eix_sites::TEXT_500001.ru(), (3, 4));
        m.bytes.insert(eix_sites::TEXT_500005.ru(), (5, 6));
        let writes = m.writes().unwrap();
        assert_eq!(writes.len(), 2);
        assert_eq!(
            (
                writes[0].site(),
                writes[0].expected(),
                writes[0].new_bytes()
            ),
            (eix_sites::TEXT_500000, &[1, 3][..], &[2, 4][..])
        );
        assert_eq!(
            (writes[1].site(), writes[1].size()),
            (eix_sites::TEXT_500005, 1)
        );
    }

    /// addon.dll's 6-byte hook at 0x548315, as `groups::applied_write` reports it.
    fn addon_hook(at: Site) -> Option<Vec<u8>> {
        (at == eix_sites::UPDATE_MOTION__A85).then(|| vec![0xE9, 0x52, 0x87, 0x4F, 0x01, 0x90])
    }

    #[test]
    fn a_write_covering_an_addon_write_takes_it_over() {
        let mut base = vec![0xD9, 0x47, 0x14, 0xD9, 0x47, 0x18, 0xDC, 0x0D];
        base[..6].copy_from_slice(&addon_hook(eix_sites::UPDATE_MOTION__A85).unwrap());
        assert_eq!(
            addon_writes_covered(
                eix_sites::UPDATE_MOTION__A85.ru(),
                &base,
                &[0; 8],
                addon_hook
            ),
            Ok((vec![(eix_sites::UPDATE_MOTION__A85.ru(), 6)], vec![]))
        );
        assert_eq!(
            addon_writes_covered(
                eix_sites::UPDATE_MOTION__A82.ru(),
                &[0; 12],
                &[0; 12],
                addon_hook
            ),
            Ok((vec![], vec![])),
            "bytes the addon no longer holds there are not its write"
        );
        assert!(
            addon_writes_covered(
                eix_sites::UPDATE_MOTION__A88.ru(),
                &base[3..],
                &[0; 5],
                addon_hook
            )
            .is_err()
        );
        assert!(
            addon_writes_covered(
                eix_sites::UPDATE_MOTION__A80.ru(),
                &[0; 8],
                &[0; 8],
                addon_hook
            )
            .is_err()
        );
        assert_eq!(
            addon_writes_covered(
                eix_sites::UPDATE_MOTION__A8B.ru(),
                &[0; 6],
                &[0; 6],
                addon_hook
            ),
            Ok((vec![], vec![]))
        );
        assert_eq!(
            addon_writes_covered(
                eix_sites::UPDATE_MOTION__A70.ru(),
                &[0; 0x15],
                &[0; 0x15],
                addon_hook
            ),
            Ok((vec![], vec![]))
        );
    }

    /// DeagrPlayerOnLoose's 12-byte write at 0x5cacfc: the `je` made NOPs, then its call (the displacement made up).
    fn addon_deagr(at: Site) -> Option<Vec<u8>> {
        (at == eix_sites::AI_VIEWER_UPDATE__2FC)
            .then(|| vec![0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0xE8, 1, 2, 3, 4, 0x90])
    }

    #[test]
    fn a_write_ending_in_an_addon_writes_own_bytes_leaves_that_part_to_it() {
        // aggrmod 6's rewrite of 0x5caccc ends in six NOPs over the `je` the addon has already made NOPs.
        let mut base = vec![0xEE; 54];
        base[48..].copy_from_slice(&[0x90; 6]);
        let mut new = vec![0x74; 54];
        new[48..].copy_from_slice(&[0x90; 6]);
        assert_eq!(
            addon_writes_covered(
                eix_sites::AI_VIEWER_UPDATE__2CC.ru(),
                &base,
                &new,
                addon_deagr
            ),
            Ok((vec![], vec![(eix_sites::AI_VIEWER_UPDATE__2FC.ru(), 6)]))
        );
        new[53] = 0xCC;
        assert!(
            addon_writes_covered(
                eix_sites::AI_VIEWER_UPDATE__2CC.ru(),
                &base,
                &new,
                addon_deagr
            )
            .is_err(),
            "other bytes over part of the addon's write would break it"
        );
    }

    /// addon.dll's 7-byte write at 0x626a1e (the perk limit's compare), as the registry reports it.
    fn addon_compare(at: Site) -> Option<Vec<u8>> {
        (at == eix_sites::TEXT_626A1E).then(|| vec![0x8B, 0x54, 0x02, 0x10, 0x83, 0xFA, 0x64])
    }

    #[test]
    fn pieces_of_one_instruction_are_weighed_against_addon_writes_together() {
        // `extendedskills`' second cap: two NOPs, then the jump to its hook.
        let nops = SiteWrite::nop(eix_sites::TEXT_626A1E, &[0x8B, 0x54]);
        let jump = SiteWrite::bytes(
            eix_sites::TEXT_626A20,
            &[0x02, 0x10, 0x83, 0xFA, 0x64],
            &[0xE9, 1, 2, 3, 4],
        )
        .unwrap();
        let apart = SiteWrite::nop(eix_sites::TEXT_626B00, &[0x90]);
        let runs = contiguous_runs(&[nops, jump, apart]);
        assert_eq!(runs.len(), 2);
        assert_eq!(
            (runs[0].0, runs[0].1.len(), runs[0].2.len()),
            (eix_sites::TEXT_626A1E.ru(), 7, 7)
        );
        assert_eq!(runs[0].2, [0x90, 0x90, 0xE9, 1, 2, 3, 4]);
        // Held whole, the range takes over the addon's write; its first piece alone covers only part of it.
        let held = addon_compare(eix_sites::TEXT_626A1E).unwrap();
        assert_eq!(
            addon_writes_covered(
                eix_sites::TEXT_626A1E.ru(),
                &held,
                &runs[0].2,
                addon_compare
            ),
            Ok((vec![(eix_sites::TEXT_626A1E.ru(), 7)], vec![]))
        );
        assert!(
            addon_writes_covered(
                eix_sites::TEXT_626A1E.ru(),
                &held[..2],
                &runs[0].2[..2],
                addon_compare
            )
            .is_err()
        );
    }

    #[test]
    fn writes_outside_the_image_are_refused() {
        let mut m = Merge::default();
        let w = SiteWrite::bytes(eix_sites::DATA_7CCFFF, &[0, 0], &[1, 1]).unwrap();
        assert!(m.add(&[w]).is_err());
        let w = SiteWrite::bytes(Site::new(0x1000), &[0], &[1]).unwrap();
        assert!(m.add(&[w]).is_err());
        assert!(m.bytes.is_empty());
    }
}
