//! The built-in SpellAddon (crate `spelladdon`): its hook on the sound system's start, installed before WinMain,
//! and its log lines in addon.log.

use spelladdon::Level;

use crate::game::BeforeWinMain;
use crate::log;

fn write(level: Level, subsystem: &str, message: &str, at: &str) {
    let level = match level {
        Level::Info => log::Level::Info,
        Level::Warn => log::Level::Warn,
        Level::Error => log::Level::Error,
    };
    log::write(level, subsystem, message, at);
}

/// Installs the hook that runs SpellAddon when the sound system starts.
pub(crate) fn install(token: &BeforeWinMain, mod_dir: &[u8]) {
    let mut site = None;
    let report = token.apply("SpellAddon", || {
        let w = spelladdon::hook(mod_dir, write, crate::groups::applied_write)?;
        site = Some((w.site(), w.size()));
        Ok(vec![w])
    });
    if let Some((at, len)) = site {
        log::info("site", "SpellAddon start", &format!("0x{at:08X}+{len}"));
    }
    match report.outcome {
        eix_engine::group::Outcome::Applied => {
            log::info("groups", "applied", "SpellAddon");
        }
        eix_engine::group::Outcome::Disabled | eix_engine::group::Outcome::Failed { .. } => {
            log::error("groups", &report.to_string(), "SpellAddon");
        }
    }
}
