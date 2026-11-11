//! MoviesPatch: every movie the game opens through its `_BinkOpen@8` import comes from the mod's `Movies\` folder
//! when the mod has a file of that name (the name after the path's last `\`).
#![expect(
    unsafe_code,
    reason = "game.exe's import slot, the name Bink is given, and calling the real BinkOpen"
)]

use std::ffi::{CString, c_void};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use eix_core::mod_files::from_last_slash;
use eix_engine::game::media::{BINK_OPEN_SLOT, BinkOpen};
use eix_engine::group::SiteWrite;

use super::arg_path::c_str;
use super::{BuildError, Labelled, Setup};
use crate::log;
use crate::setup::path;

static MOD_DIR: OnceLock<Vec<u8>> = OnceLock::new();
/// The game's `_BinkOpen@8`, which the slot held.
static REAL: AtomicUsize = AtomicUsize::new(0);

/// The mod's movie for `name`, when it has one.
fn mod_movie(name: &[u8]) -> Option<CString> {
    let candidate = [&MOD_DIR.get()?[..], b"Movies", from_last_slash(name)?].concat();
    if !path(&candidate).is_file() {
        return None;
    }
    CString::new(candidate).ok()
}

extern "system" fn bink_open(name: *const u8, flags: u32) -> *mut c_void {
    // SAFETY: Bink's callers pass the movie's file name, a NUL-terminated string that lives for the call.
    let game = unsafe { c_str(name.expose_provenance() as u32) };
    let replacement = game.and_then(|g| mod_movie(g.to_bytes()));
    if let Some(r) = &replacement {
        log::info("movies", "the mod's movie", &r.to_string_lossy());
    }
    let opened = replacement.as_ref().map_or(name, |r| r.as_ptr().cast());
    // SAFETY: REAL holds the game's `_BinkOpen@8`, read from its import slot before the slot pointed here.
    let real: BinkOpen = unsafe { eix_engine::hook::as_fn_at(REAL.load(Ordering::SeqCst)) };
    // SAFETY: the game's arguments, with a live NUL-terminated name in place of the first.
    unsafe { real(opened, flags) }
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let _ = MOD_DIR.set(setup.mod_dir.clone());
    // SAFETY: the build is a supported one, whose import slot is initialised by the Windows loader before the entry point;
    // this runs on the game's thread before WinMain.
    let real = unsafe { BINK_OPEN_SLOT.read() };
    REAL.store(real, Ordering::SeqCst);
    Ok(vec![Labelled {
        label: "Movies".to_owned(),
        write: SiteWrite::pointer(BINK_OPEN_SLOT.site(), real, bink_open as *const () as usize),
    }])
}
