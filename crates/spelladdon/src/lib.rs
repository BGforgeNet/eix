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

pub use events::string_event_hook;

/// How serious a log line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Normal progress.
    Info,
    /// Something is off, but SpellAddon goes on.
    Warn,
    /// Something failed.
    Error,
}

/// Writes one addon.log line: the level, the subsystem (`spelladdon` or `site`), the message, and the site or key
/// it concerns (empty for none).
pub type Log = fn(Level, &str, &str, &str);

/// The bytes addon.dll wrote at exactly a site, if any: a SpellAddon write that covers them lands over them, as the
/// original's copy does.
pub type Earlier = fn(Site) -> Option<Vec<u8>>;

/// The call of `AIL_startup` in `CSoundManager`'s constructor: `call [0x73B7D4]`.
const AIL_STARTUP_CALL: Site = eix_sites::C_SOUND_MANAGER_CONSTRUCT__22A;
const AIL_STARTUP_BYTES: [u8; 6] = [0xFF, 0x15, 0xD4, 0xB7, 0x73, 0x00];

/// What the hook needs when it runs.
#[derive(Debug)]
struct Setup {
    mod_dir: Vec<u8>,
    log: Log,
    addon: Earlier,
}

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

/// Whether Miles loading `path` would load a separate SpellAddon: a file `SpellAddon*.asi`, in any case.
fn is_separate_spelladdon(path: &[u8]) -> bool {
    let name = path
        .rsplit(|&c| c == b'\\' || c == b'/')
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    name.starts_with(b"spelladdon") && name.ends_with(b".asi")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_a_separate_spelladdon_by_its_file_name() {
        assert!(is_separate_spelladdon(
            b"C:\\GOG\\EvilIslands\\SpellAddonXa.asi"
        ));
        assert!(is_separate_spelladdon(b".\\SPELLADDONX.ASI"));
        assert!(is_separate_spelladdon(b"spelladdon.asi"));
        assert!(!is_separate_spelladdon(b"C:\\Game\\mp3dec.asi"));
        assert!(!is_separate_spelladdon(b"C:\\SpellAddon\\EI_Plugin.asi"));
        assert!(!is_separate_spelladdon(b"SpellAddonXa.asi.bak"));
    }
}
