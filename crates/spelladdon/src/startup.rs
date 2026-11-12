//! SpellAddon's start, at the moment the sound system would load the original module:
//! the INI, the fixed patch, then every feature in order.

use std::path::PathBuf;

use eix_core::spelladdon::{Profile, Values, duplicate_keys};
use eix_engine::ansi;
use eix_engine::group::{Outcome, SiteWrite};
use eix_engine::win::{from_ansi, to_ansi};
use eix_sites::Site;

use crate::apply::{self, Merge};
use crate::features::{Action, Ctx};
use crate::registry::{self, FEATURES};
use crate::{Earlier, Level, Log};

/// The INI's file name, in the mod folder or the game folder.
const INI: &[u8] = b"SPELLADDON.INI";

/// `BeginConstruct`'s `push 0x79583C` ("No valid spell prototype\n") at 0x684205, which the original replaces
/// with `push edx` and four NOPs whatever the INI says.
const FIXED_PATCH: Site = eix_sites::C_SPELL_CONSTRUCTOR_BEGIN_CONSTRUCT__25;
const FIXED_WAS: [u8; 5] = [0x68, 0x3C, 0x58, 0x79, 0x00];
const FIXED_NOW: [u8; 5] = [0x52, 0x90, 0x90, 0x90, 0x90];

/// The system's INI reads on one file.
struct OsProfile {
    file: Vec<u8>,
}

impl Profile for OsProfile {
    fn int(&self, section: &str, key: &str, default: i32) -> i32 {
        ansi::profile_int(section.as_bytes(), key.as_bytes(), default, &self.file)
    }

    fn text(&self, section: &str, key: &str, default: &str, size: usize) -> Vec<u8> {
        // 0xCC: the debug build's stack filler, which the digit parser reads past the string's NUL.
        ansi::profile_string(
            section.as_bytes(),
            key.as_bytes(),
            default.as_bytes(),
            size,
            0xCC,
            &self.file,
        )
    }
}

/// An INI without the key: every read gives its default, as the system's reads do.
struct Defaults;

impl Profile for Defaults {
    fn int(&self, _: &str, _: &str, default: i32) -> i32 {
        default
    }

    fn text(&self, _: &str, _: &str, default: &str, size: usize) -> Vec<u8> {
        let mut buffer = vec![0xCC; size];
        let n = default.len().min(size.saturating_sub(1));
        buffer[..n].copy_from_slice(&default.as_bytes()[..n]);
        if let Some(end) = buffer.get_mut(n) {
            *end = 0;
        }
        buffer
    }
}

/// Every write SpellAddon may make, each with what makes it: the fixed patch, then each feature at each positive
/// value of its flag that it builds and does not skip, every other key at its default.
pub(crate) fn writes() -> Vec<(String, SiteWrite)> {
    fn no_log(_: Level, _: &str, _: &str, _: &str) {}
    let values = Values::read(&Defaults);
    let ctx = Ctx::new(&values, no_log);
    // The fixed patch's two byte strings have one length, so it always builds.
    let mut out: Vec<(String, SiteWrite)> = SiteWrite::bytes(FIXED_PATCH, &FIXED_WAS, &FIXED_NOW)
        .map(|w| ("fixed patch".to_owned(), w))
        .into_iter()
        .collect();
    for f in &FEATURES {
        let Some(build) = f.build() else { continue };
        for value in 1..=i8::MAX {
            if registry::skipped(f, value, &values).is_some() {
                continue;
            }
            // A value the feature does not take builds nothing or fails; neither writes anything.
            let Ok(writes) = build(&ctx, value) else {
                continue;
            };
            let name = format!("[{}] {}={value}", f.section, f.key.trim_end());
            out.extend(writes.into_iter().map(|w| (name.clone(), w)));
        }
    }
    out
}

/// ANSI bytes as a path for std.
fn path(ansi: &[u8]) -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_wide(&from_ansi(ansi)))
}

/// The game's working folder in the ANSI code page, without a trailing `\`.
fn current_dir() -> Option<Vec<u8>> {
    use std::os::windows::ffi::OsStrExt;
    let dir = std::env::current_dir().ok()?;
    Some(to_ansi(
        &dir.as_os_str().encode_wide().collect::<Vec<u16>>(),
    ))
}

/// The INI to read: the mod folder's, else the game folder's (the built-in SpellAddon's choice); `None` when neither
/// exists, and SpellAddon then does nothing.
fn ini_file(mod_dir: &[u8], log: Log) -> Option<Vec<u8>> {
    let mut candidates = Vec::new();
    if !mod_dir.is_empty() {
        candidates.push([mod_dir, INI].concat());
    }
    match current_dir() {
        Some(dir) => candidates.push([&dir[..], b"\\", INI].concat()),
        None => log(
            Level::Warn,
            "spelladdon",
            "cannot read the working folder; its spelladdon.ini is not looked for",
            "",
        ),
    }
    candidates.into_iter().find(|c| path(c).is_file())
}

/// Runs SpellAddon: from the hook on the sound system's start, on the game's main thread, after addon.dll's writes
/// (`addon` looks them up).
pub(crate) fn run(mod_dir: &[u8], log: Log, addon: Earlier) {
    crate::features::common::runtime_log(log);
    if let Err(e) = apply::block_separate_spelladdon(log) {
        log(
            Level::Error,
            "spelladdon",
            &format!("cannot keep a separate SpellAddon*.asi from loading: {e}"),
            "",
        );
    }
    let Some(file) = ini_file(mod_dir, log) else {
        log(
            Level::Info,
            "spelladdon",
            "no spelladdon.ini in the mod folder or the game folder: SpellAddon is off",
            "",
        );
        return;
    };
    let shown = String::from_utf8_lossy(&file).into_owned();
    log(
        Level::Info,
        "spelladdon",
        "built-in SpellAddon 20.6 reads",
        &shown,
    );
    // The original keeps its embedded documentation in the working folder; the built-in has no copy to write.
    log(
        Level::Info,
        "spelladdon",
        "SpellAddonInfo.txt is not written: the built-in SpellAddon carries no copy of the original's text",
        "",
    );
    if let Err(e) = apply::unprotect_image() {
        log(
            Level::Error,
            "spelladdon",
            &format!("game.exe's image stays protected: {e}"),
            "",
        );
    }
    let dir = file
        .iter()
        .rposition(|&c| c == b'\\')
        .map_or(&[][..], |i| &file[..i])
        .to_vec();
    let text = match std::fs::read(path(&file)) {
        Ok(text) => text,
        Err(e) => {
            log(
                Level::Warn,
                "spelladdon",
                &format!("cannot read INI text for duplicate diagnostics: {e}"),
                &shown,
            );
            Vec::new()
        }
    };
    let values = read_values(&OsProfile { file }, &text, log);
    let mut ctx = Ctx::new(&values, log);
    ctx.ini_dir = dir;
    let mut merge = Merge::new(addon);
    let mut tally = Tally::default();
    let fixed = SiteWrite::bytes(FIXED_PATCH, &FIXED_WAS, &FIXED_NOW).map_err(|e| e.to_string());
    keep(
        log,
        &mut merge,
        &mut tally,
        "fixed patch",
        fixed.map(|w| vec![w]),
    );
    for f in &FEATURES {
        let name = format!("[{}] {}", f.section, f.key.trim_end());
        // Every feature's key is a flag of the table (the registry's tests check it).
        let value = values.flag(f.section, f.key).unwrap_or(0);
        if let Some(reason) = registry::skipped(f, value, &values) {
            log(
                Level::Warn,
                "spelladdon",
                &format!("value {value} skipped: {reason}"),
                &name,
            );
            tally.skipped += 1;
            continue;
        }
        let Some(build) = f.build() else {
            if value != 0 {
                log(
                    Level::Warn,
                    "spelladdon",
                    &format!("value {value} not implemented yet"),
                    &name,
                );
                tally.missing += 1;
            }
            continue;
        };
        let owner = format!("SpellAddon {name}");
        let result = eix_engine::owners::building(&owner, || build(&ctx, value));
        let actions = ctx.actions.take();
        if result.is_ok() {
            for action in actions {
                perform(action, log);
            }
        }
        match result {
            Ok(writes) if writes.is_empty() => {
                if value != 0 {
                    log(
                        Level::Info,
                        "spelladdon",
                        &format!("value {value}: no change"),
                        &name,
                    );
                }
            }
            result => keep(
                log,
                &mut merge,
                &mut tally,
                &name,
                result.map_err(|e| e.to_string()),
            ),
        }
    }
    let report = merge.apply();
    match &report.outcome {
        Outcome::Applied => {
            // After the merged write, which names them all "SpellAddon": a later record wins.
            for (owner, site, len) in &tally.sites {
                eix_engine::owners::site(site.here(), *len, owner);
            }
            log(Level::Info, "spelladdon", "patches written", "");
        }
        Outcome::Disabled | Outcome::Failed { .. } => {
            log(Level::Error, "spelladdon", &report.to_string(), "");
            tally.failed += tally.applied;
            tally.applied = 0;
        }
    }
    log(
        Level::Info,
        "spelladdon",
        &format!(
            "{} applied, {} not implemented, {} skipped, {} failed",
            tally.applied, tally.missing, tally.skipped, tally.failed
        ),
        "",
    );
}

/// Performs the external actions at their feature's place in startup order.
fn perform(action: Action, log: Log) {
    match action {
        Action::ImportantAlerts => crate::features::startup::show_important_alerts(),
        Action::VersionAlerts => crate::features::startup::show_version_alerts(log),
        Action::AddFont(path) => {
            // The original ignores the result; a failure is logged, the four names still apply.
            if ansi::add_private_font(&path) == 0 {
                log(
                    Level::Warn,
                    "spelladdon",
                    &format!(
                        "changefont: the font file {} was not added",
                        String::from_utf8_lossy(&path)
                    ),
                    "",
                );
            }
        }
    }
}

fn read_values(profile: &impl Profile, text: &[u8], log: Log) -> Values {
    let emit = |d: eix_core::spelladdon::Diagnostic| {
        log(
            if d.warning { Level::Warn } else { Level::Info },
            "spelladdon",
            &d.message,
            &d.key,
        );
    };
    for d in duplicate_keys(text) {
        emit(d);
    }
    Values::read_with_diagnostics(profile, emit)
}

/// How the features went, for the summary line, and the sites of those kept, for crash records.
#[derive(Debug, Default)]
struct Tally {
    applied: usize,
    missing: usize,
    skipped: usize,
    failed: usize,
    /// `(owner, site, length)` of each kept write.
    sites: Vec<(String, Site, usize)>,
}

/// Merges a feature's writes, logging each site, or logs why none is kept.
fn keep(
    log: Log,
    merge: &mut Merge,
    tally: &mut Tally,
    name: &str,
    writes: Result<Vec<SiteWrite>, String>,
) {
    match writes.and_then(|w| merge.add(&w).map(|taken| (w, taken))) {
        Ok((writes, taken)) => {
            for at in taken {
                log(
                    Level::Warn,
                    "spelladdon",
                    "lands over addon.dll's write, which no longer runs",
                    &format!("0x{at:08X} ({name})"),
                );
            }
            for (i, w) in writes.iter().enumerate() {
                log(
                    Level::Info,
                    "site",
                    &format!("SpellAddon {name} #{}", i + 1),
                    &format!("0x{:08X}+{}", w.site(), w.size()),
                );
                tally
                    .sites
                    .push((format!("SpellAddon {name}"), w.site(), w.size()));
            }
            log(Level::Info, "spelladdon", "applied", name);
            tally.applied += 1;
        }
        Err(e) => {
            log(Level::Error, "spelladdon", &format!("failed: {e}"), name);
            tally.failed += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_ini_reads_and_diagnostics() {
        let dir = std::env::temp_dir().join(format!("eix-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("test.ini");
        let text = b"[speed]\r\ndisablepausekey=1\r\ndisablepausekey =2\r\n[gameplay]\r\navrgarmors=1\r\nnewspells=256\r\nnewspells=1\r\n";
        std::fs::write(&file, text).unwrap();
        let p = OsProfile {
            file: file.to_str().unwrap().as_bytes().to_vec(),
        };
        let plain = p.int("speed", "disablepausekey", -1);
        let spaced = p.int("speed", "disablepausekey ", -1);
        static MESSAGES: std::sync::Mutex<Vec<(Level, String, String)>> =
            std::sync::Mutex::new(Vec::new());
        fn log(level: Level, _: &str, message: &str, key: &str) {
            MESSAGES
                .lock()
                .unwrap()
                .push((level, message.to_owned(), key.to_owned()));
        }
        let v = read_values(&p, text, log);
        std::fs::remove_dir_all(dir).unwrap();
        // The native reader treats trailing key whitespace as insignificant and keeps the first value.
        assert_eq!((plain, spaced), (1, 1));
        assert_eq!(v.flag("gameplay", "newspells"), Ok(0));
        assert_eq!(v.flag("gameplay", "avgarmors"), Ok(1));
        let messages = MESSAGES.lock().unwrap();
        assert!(
            messages
                .iter()
                .any(|(level, msg, key)| *level == Level::Warn
                    && key == "[gameplay] \"newspells\""
                    && msg == "duplicate key: using \"256\", ignoring \"1\"")
        );
        assert!(
            messages
                .iter()
                .any(|(level, msg, key)| *level == Level::Warn
                    && key == "[gameplay] \"newspells\""
                    && msg.contains("256")
                    && msg.contains("using 0"))
        );
        assert!(
            messages
                .iter()
                .any(|(level, msg, _)| *level == Level::Info && msg.contains("avrgarmors"))
        );
    }
}
