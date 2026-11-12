//! INI values for scripts, read from the mod folder at the call: the one reader behind the script functions and the
//! Lua function.

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

use eix_core::ini;
use eix_engine::ansi;
use eix_engine::win::ansi_text;

use crate::{log, setup};

/// The longest value read; the API cuts a longer one.
const VALUE_BYTES: usize = 4096;

/// The calls already reported, by file, section and key.
static REPORTED: Mutex<BTreeSet<Vec<u8>>> = Mutex::new(BTreeSet::new());

/// The mod folder, from the registry at the first call.
fn mod_dir() -> &'static [u8] {
    static DIR: OnceLock<Vec<u8>> = OnceLock::new();
    DIR.get_or_init(setup::mod_dir)
}

/// Logs `what` for the call once. A script may read every tick, and a missing value or a refused path stays so for
/// the rest of the session, so a repeat would only fill addon.log.
fn report_once(what: &str, file: &[u8], section: &[u8], key: &[u8]) {
    let id = [file, b"\0", section, b"\0", key, b"\0", what.as_bytes()].concat();
    let first = REPORTED.lock().is_ok_and(|mut seen| seen.insert(id));
    if first {
        let at = format!(
            "{} [{}] {}",
            ansi_text(file),
            ansi_text(section),
            ansi_text(key)
        );
        log::warn("ini", what, &at);
    }
}

/// The value of `key` in `[section]` of `file` (a path below the mod folder); `None` when the path is refused or the
/// file, section or key is missing. A key with an empty value gives an empty value.
pub(crate) fn read(file: &[u8], section: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let Some(relative) = ini::mod_relative_path(file) else {
        report_once("not a path below the mod folder", file, section, key);
        return None;
    };
    let base = mod_dir();
    if base.is_empty() {
        report_once("there is no mod folder", file, section, key);
        return None;
    }
    let full = [base, &relative].concat();
    // Two reads with different defaults tell a missing key from a value equal to a default: only a present value
    // comes back the same.
    let value = |default: &[u8]| {
        let buf = ansi::profile_string(section, key, default, VALUE_BYTES, 0, &full);
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        buf[..end].to_vec()
    };
    let (first, second) = (value(b"\x01a"), value(b"\x01b"));
    if first != second {
        report_once("no such file, section or key", file, section, key);
        return None;
    }
    Some(first)
}

/// `read` as a number, `default` when it is missing or not a number.
pub(crate) fn number(file: &[u8], section: &[u8], key: &[u8], default: f32) -> f32 {
    let Some(value) = read(file, section, key) else {
        return default;
    };
    match ini::parse_number(&value) {
        Some(n) => n as f32,
        None => {
            report_once("the value is not a number", file, section, key);
            default
        }
    }
}
