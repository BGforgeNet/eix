//! BinkPatch (a `binkw32.dll` in the mod folder): game.exe's Bink imports are bound to the mod's copy. Every import
//! slot of `binkw32.dll` gets the mod DLL's export of the same name; `_BinkSetVolume@8` gets a wrapper that calls the
//! mod DLL's `_BinkSetVolume@12` with track 0, the newer Bink's signature.
//!
//! The original fails the group when the game's binkw32 is not loaded, lacks `_BinkOpen@8`, or the mod's cannot be
//! loaded or lacks an import's name.
#![expect(
    unsafe_code,
    reason = "reading game.exe's import slots and calling the mod's Bink"
)]

use std::collections::BTreeMap;
use std::ffi::{CStr, c_void};
use std::sync::atomic::{AtomicUsize, Ordering};

use eix_engine::game::Global;
use eix_engine::game::media::{BINK_IMPORTS, BinkSetVolume12};

use super::{BuildError, Labelled, Setup, pointer};
use crate::log;
use crate::setup::ModBink;

/// The mod DLL's `_BinkSetVolume@12`, or 0.
static SET_VOLUME_12: AtomicUsize = AtomicUsize::new(0);

/// Stands in for `_BinkSetVolume@8` (stdcall `(movie, volume)`).
extern "system" fn set_volume(movie: *mut c_void, volume: i32) {
    let f = SET_VOLUME_12.load(Ordering::SeqCst);
    if f == 0 {
        // The original calls the missing export and crashes; the volume stays as it is here.
        return;
    }
    // SAFETY: `f` is the mod DLL's `_BinkSetVolume@12`, which stays loaded.
    let f: BinkSetVolume12 = unsafe { eix_engine::hook::as_fn(f) };
    // SAFETY: the game's movie and volume, with the track the older signature lacks.
    unsafe { f(movie, 0, volume) }
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let env = &setup.options.env;
    if !env.game_bink_loaded {
        return Err("the game's binkw32.dll is not loaded".to_owned().into());
    }
    if !env.bink_open_exported {
        return Err("the game's binkw32.dll has no _BinkOpen@8"
            .to_owned()
            .into());
    }
    let dll = match &setup.mod_bink {
        ModBink::Separate(dll) => dll,
        ModBink::LoadFailed(code) => {
            return Err(format!("cannot load the mod's binkw32.dll: error {code}").into());
        }
        ModBink::None => return Err("the mod has no separate binkw32.dll".to_owned().into()),
    };
    let set_volume_12 = dll.address(c"_BinkSetVolume@12").unwrap_or(0);
    if set_volume_12 == 0 {
        log::warn(
            "groups",
            "BinkPatch: the mod's binkw32.dll has no _BinkSetVolume@12; movie volume changes are ignored",
            dll.path(),
        );
    }
    SET_VOLUME_12.store(set_volume_12, Ordering::SeqCst);
    // The original walks the imports in name order (a map keyed by name).
    let by_name: BTreeMap<&[u8], (&CStr, Global<usize>)> = BINK_IMPORTS
        .iter()
        .map(|&(name, slot)| (name.to_bytes(), (name, slot)))
        .collect();
    let mut writes = Vec::with_capacity(by_name.len());
    for (name, slot) in by_name.into_values() {
        let new = if name == c"_BinkSetVolume@8" {
            set_volume as *const () as usize
        } else {
            dll.address(name)
                .ok_or_else(|| format!("the mod's binkw32.dll has no {}", name.to_string_lossy()))?
        };
        // SAFETY: the build is RU 1.06, whose import slots the Windows loader filled before the entry point; this
        // runs on the game's thread before WinMain.
        let old = unsafe { slot.read() };
        writes.push(pointer(
            &format!("BinkPatch {}", name.to_string_lossy()),
            slot.address(),
            old,
            new,
        ));
    }
    Ok(writes)
}
