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

/// A fixed image write, with expected bytes only when they are known without reading the running game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Write {
    /// A patch with real expected and replacement bytes.
    Patch(SiteWrite),
    /// A live-code copy or run-time global whose bytes depend on the running game.
    Range {
        /// The first byte written.
        site: Site,
        /// The number of bytes written.
        size: usize,
    },
}

impl Write {
    /// The first byte written, for either kind of entry.
    pub fn site(&self) -> Site {
        match self {
            Self::Patch(write) => write.site(),
            Self::Range { site, .. } => *site,
        }
    }

    /// The number of bytes written, for either kind of entry.
    pub fn size(&self) -> usize {
        match self {
            Self::Patch(write) => write.size(),
            Self::Range { size, .. } => *size,
        }
    }
}

/// Every fixed game-image range SpellAddon can write, including flag combinations and run-time globals, labelled
/// by feature. Values are representative, so the entries are not an installable patch list. Live-code copies and
/// run-time globals are [`Write::Range`] entries. Heap objects and stacks are not image sites.
/// Engine fixes use these ranges to keep clear of SpellAddon's writes.
///
/// # Panics
/// A feature cannot build its range description; returning a partial list would hide overlaps.
pub fn writes() -> Vec<(String, Write)> {
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
