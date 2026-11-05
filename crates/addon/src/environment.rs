//! The lines of addon.log that a bug report needs beyond the events: which build of eix runs, on which system, and
//! the modules loaded beside it (DirectDraw wrappers, plugins, other mods' DLLs).

use eix_engine::env;
use eix_engine::win::narrow;

use crate::log;

/// eix's version and the commit it was built from.
pub(crate) fn build() -> String {
    format!(
        "eix {} (commit {})",
        env!("CARGO_PKG_VERSION"),
        env!("EIX_COMMIT")
    )
}

/// Logs the system's version.
pub(crate) fn log_system() {
    log::info("main", &env::system(), "system");
}

/// Logs every loaded module, one line each, under `area`: its file, where it is mapped, and its link time stamp.
pub(crate) fn log_modules(area: &str) {
    match env::modules() {
        Ok(modules) => {
            for m in modules {
                let stamp = m.time_stamp.map_or_else(
                    || "no PE header".to_owned(),
                    |t| format!("linked 0x{t:08X}"),
                );
                log::info(
                    area,
                    &format!("0x{:08X}+0x{:X}, {stamp}", m.base, m.size),
                    &narrow(&m.path),
                );
            }
        }
        Err(e) => log::warn(area, &format!("cannot list the loaded modules: {e}"), ""),
    }
}
