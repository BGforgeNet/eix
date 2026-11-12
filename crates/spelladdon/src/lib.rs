#![warn(missing_docs, unreachable_pub, clippy::missing_panics_doc)]
//! The built-in SpellAddon 20.6: what SpellAddonXa.asi does to game.exe RU 1.06, from spelladdon.ini.
//!
//! The addon installs [`hook()`] before WinMain. The hook runs when the sound system starts (`AIL_startup`, where Miles
//! loads `*.asi` files and the original module did its work): it reads the INI, applies the fixed patch and the
//! features in the original's order, and keeps any separate `SpellAddon*.asi` from loading.

use std::sync::OnceLock;

use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, HookError, Mode};
use eix_sites::Site;

mod apply;
mod events;
mod features;
mod registry;
mod startup;
mod types;

pub use events::string_event_hook;
use types::Setup;
pub use types::{Earlier, Level, Log};

/// The call of `AIL_startup` in `CSoundManager`'s constructor: `call [0x73B7D4]`.
const AIL_STARTUP_CALL: Site = eix_sites::C_SOUND_MANAGER_CONSTRUCT__22A;
const AIL_STARTUP_BYTES: [u8; 6] = [0xFF, 0x15, 0xD4, 0xB7, 0x73, 0x00];

static SETUP: OnceLock<Setup> = OnceLock::new();

extern "C" fn on_sound_start(_: usize, _: &mut Context) {
    if let Some(setup) = SETUP.get() {
        startup::run(&setup.mod_dir, setup.log, setup.addon);
    }
}

/// Every write SpellAddon may make, each with the patch or `[section] key=value` that makes it, every other key at
/// its default. addon.dll's own fixes keep clear of these sites: SpellAddon applies later, and a feature that found
/// their bytes would be left out.
pub fn writes() -> Vec<(String, SiteWrite)> {
    startup::writes()
}

/// The write that runs SpellAddon just before the sound system starts; the caller applies it before WinMain.
///
/// `mod_dir` is the mod folder in the ANSI code page, ending in `\`, or empty without a mod; `addon` looks up
/// addon.dll's writes, made before.
pub fn hook(mod_dir: &[u8], log: Log, addon: Earlier) -> Result<SiteWrite, HookError> {
    let _ = SETUP.set(Setup {
        mod_dir: mod_dir.to_vec(),
        log,
        addon,
    });
    hook::context_hook(
        AIL_STARTUP_CALL,
        &AIL_STARTUP_BYTES,
        Mode::After,
        on_sound_start,
        0,
    )
}
