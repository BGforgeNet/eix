//! Start-up's game-specific calls: which exe this is, the loader contract `Apply` checks, the WinMain hook, and the
//! token that lets the patch groups write while the game's own threads have not started.
#![expect(
    unsafe_code,
    reason = "reading game.exe's bytes, hooking its WinMain and applying patch groups before it runs"
)]

use std::sync::atomic::{AtomicUsize, Ordering};

use eix_engine::exe;
use eix_engine::group::{self, Report, SiteWrite};
use eix_engine::hook::{self, HookError};
use eix_engine::patch;

/// game.exe's call of WinMain in the CRT's start-up code: `call 0x456440`.
const WIN_MAIN_CALL: usize = eix_sites::ENTRY__DB;
const WIN_MAIN_CALL_BYTES: [u8; 5] = [0xE8, 0x3A, 0xA1, 0xD6, 0xFF];

/// The words the original loader writes for an addon without `Apply` (a `jmp [0x73A762]` at 0x73A670 and the
/// target at 0x73A762); a new loader leaves them zero.
const OLD_LOADER_JUMP: usize = eix_sites::TEXT_73A670;
const OLD_LOADER_TARGET: usize = eix_sites::TEXT_73A762;

/// The operand of WinMain's `push` that names the game's registry root, unchanged until NivalRegistry runs.
const REGISTRY_ROOT_OPERAND: usize = eix_sites::TEXT_4568EA;
const NIVAL_REGISTRY_ROOT: u32 = eix_sites::DATA_7848A8 as u32;

/// Whether the process is the game.exe build eix knows.
pub(crate) fn known_exe() -> Result<(), String> {
    let id = exe::current()?;
    match exe::identify(&id) {
        Some(build) => {
            eix_sites::select(build);
            Ok(())
        }
        None => Err(format!(
            "unknown game.exe: entry point RVA {:#x}, SizeOfImage {:#x}, TimeDateStamp {:#x}; known: Russian 1.06, \
             English and German 1.06",
            id.entry_rva, id.size_of_image, id.time_date_stamp
        )),
    }
}

/// Whether game.exe still holds `expected` at `site`.
///
/// Only called once [`known_exe`] passed: every site read here lies in RU 1.06's mapped image.
pub(crate) fn holds(site: usize, expected: &[u8]) -> bool {
    // SAFETY: the site lies in game.exe's image (the build was checked), which is mapped and readable; before WinMain
    // no other thread writes it.
    unsafe { patch::verify(site, expected) }.is_ok()
}

/// What `Apply` checks: the old loader's jump is absent, and the registry root is still the game's own (no addon
/// has patched this exe yet).
pub(crate) fn loader_contract_holds() -> bool {
    holds(OLD_LOADER_JUMP, &[0; 6]) && holds(OLD_LOADER_TARGET, &[0; 4]) && exe_enables_t2()
}

/// Table T2's check: WinMain still pushes the game's registry root.
pub(crate) fn exe_enables_t2() -> bool {
    holds(REGISTRY_ROOT_OPERAND, &NIVAL_REGISTRY_ROOT.to_le_bytes())
}

/// Proof that the game's code has not started its own threads: patch groups apply through it.
#[derive(Debug)]
pub(crate) struct BeforeWinMain(());

impl BeforeWinMain {
    /// Applies the group `name`: `build` prepares its writes, then all are checked and made, or none.
    pub(crate) fn apply(
        &self,
        name: &'static str,
        build: impl FnOnce() -> Result<Vec<SiteWrite>, HookError>,
    ) -> Report {
        // SAFETY: this token exists only inside the WinMain hook, before the original WinMain runs, so the game
        // runs on this thread alone and nothing else writes its code; every group's sites are RU 1.06 sites (the
        // build was checked before the hook was installed), and each group's hooks match the code at its sites.
        unsafe { group::apply(name, true, build) }
    }
}

/// The original WinMain, called once the addon's work is done.
static WIN_MAIN: AtomicUsize = AtomicUsize::new(0);

type WinMain = unsafe extern "system" fn(usize, usize, *const u8, i32) -> i32;

/// Stands in for WinMain: runs `work` with the token, then the game's WinMain.
extern "system" fn win_main(
    instance: usize,
    previous: usize,
    command: *const u8,
    show: i32,
) -> i32 {
    crate::addon_main(&BeforeWinMain(()));
    // SAFETY: WIN_MAIN holds the CRT's call target, WinMain 0x456440, a stdcall function of this type, stored
    // before the call site was redirected here.
    let original: WinMain = unsafe { hook::as_fn(WIN_MAIN.load(Ordering::SeqCst)) };
    // SAFETY: the arguments are the CRT's, passed on unchanged.
    unsafe { original(instance, previous, command, show) }
}

/// Points the CRT's call of WinMain at [`win_main`]; it runs on the thread that called `Apply`, before WinMain.
pub(crate) fn hook_win_main() -> Result<(), String> {
    let (write, old) = hook::redirect_call(
        WIN_MAIN_CALL,
        WIN_MAIN_CALL_BYTES,
        win_main as *const () as usize,
    )
    .map_err(|e| e.to_string())?;
    WIN_MAIN.store(old, Ordering::SeqCst);
    crate::groups::log_site("HookWinMain", &write);
    // SAFETY: the call site is game.exe's (the build was checked) and has not run yet: `Apply` runs before the
    // entry point goes on, on the only thread.
    let report = unsafe { group::apply("HookWinMain", true, || Ok(vec![write])) };
    match report.outcome {
        group::Outcome::Applied => Ok(()),
        group::Outcome::Disabled | group::Outcome::Failed { .. } => Err(report.to_string()),
    }
}
