//! SpellAddon's start, at the moment the sound system would load the original module:
//! the INI, the fixed patch, then every feature in order.

use std::path::PathBuf;

use eix_core::spelladdon::{Profile, Values, duplicate_keys};
use eix_engine::ansi;
use eix_engine::group::{Outcome, SiteWrite};
use eix_engine::win::{from_ansi, narrow, to_ansi};
use eix_sites::Site;

use crate::Write;
use crate::apply::{self, Merge};
use crate::features::{Action, Ctx};
use crate::registry::{self, FEATURES};
use crate::types::{Earlier, Level, Log};

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

/// Values that expose additional sites, alongside the default profile.
struct AdditionalSites;

impl Profile for AdditionalSites {
    fn int(&self, section: &str, key: &str, default: i32) -> i32 {
        match (section, key) {
            ("gameplay", "newoptions" | "newperks" | "follow" | "loseallyfast")
            | ("rebalanceengine", "meleesum_2") => 1,
            ("gameplay", "speedslowremaster") => 2,
            ("gameplay", "damageboost") => 5,
            ("gameplay", "saveall") => 32,
            ("gameplay", "maxcountofmerc") => 51,
            _ => default,
        }
    }

    fn text(&self, section: &str, key: &str, default: &str, size: usize) -> Vec<u8> {
        Defaults.text(section, key, default, size)
    }
}

/// Fixed image write ranges from all signed flag values and the profiles that add sites or extend a write.
/// Feature builds supply the ranges and the descriptions of run-time globals.
pub(crate) fn writes() -> Vec<(String, Write)> {
    fn no_log(_: Level, _: &str, _: &str, _: &str) {}
    let fixed = SiteWrite::bytes(FIXED_PATCH, &FIXED_WAS, &FIXED_NOW)
        .unwrap_or_else(|e| panic!("fixed patch: {e}"));
    let mut out = vec![("fixed patch".to_owned(), Write::Patch(fixed))];
    for (profile, values) in [
        ("", Values::read(&Defaults)),
        (" (additional sites)", Values::read(&AdditionalSites)),
    ] {
        let ctx = Ctx::inventory(&values, no_log);
        for f in &FEATURES {
            let Some(build) = f.build else { continue };
            for value in i8::MIN..=i8::MAX {
                if registry::skipped(f, value, &values).is_some() {
                    continue;
                }
                let name = format!("[{}] {}={value}{profile}", f.section, f.key.trim_end());
                let writes = build(&ctx, value).unwrap_or_else(|e| panic!("{name}: {e}"));
                out.extend(
                    writes
                        .into_iter()
                        .map(Write::Patch)
                        .chain(ctx.ranges.take())
                        .map(|w| (name.clone(), w)),
                );
            }
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
    let shown = narrow(&from_ansi(&file));
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
        let Some(build) = f.build else {
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
                        narrow(&from_ansi(&path))
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
    use std::collections::{BTreeMap, BTreeSet};

    use crate::features::testing::{no_log, values};

    /// Secondary flags at each branch boundary, including the signed-byte endpoints.
    const FLAGS: &[&str] = &[
        "-128", "-1", "0", "1", "2", "3", "4", "5", "6", "12", "13", "16", "31", "32", "33", "127",
    ];

    type Input = (&'static str, &'static str, &'static [&'static str]);

    /// Inputs read by a feature or its skip condition, including those that only change bytes within a site.
    fn inputs(key: &str) -> Vec<Input> {
        let flags: &[(&str, &str)] = match key {
            "rewritenpcvisionarc" => &[("gameplay", "removenpcsblock")],
            "autorunmobfile" => &[("gameplay", "newspells")],
            "genocideincvars" => &[("gameplay", "genocidedipvars")],
            "avgarmors" => &[("gameplay", "newperks")],
            "meleesum_low" => &[
                ("rebalanceengine", "meleesum_1"),
                ("rebalanceengine", "meleesum_2"),
            ],
            "possessioncharmlink" | "nightpower" => &[("gameplay", "damageboost")],
            "shopschanges" => &[("gameplay", "shopsnewnormalchanges")],
            "encumbrancemovespeed" => &[("gameplay", "speedslowremaster")],
            "encumbranceattackspeed" => &[
                ("animationsspeed", "rewriteactionsspeed"),
                ("gameplay", "actionspatch"),
            ],
            "camp" => &[("gameplay", "newoptions")],
            "newoptions" => &[("gameplay", "camp")],
            "visionfix" => &[("gameplay", "aggrmod")],
            "increasinghitchance" => &[("gameplay", "newattackdefence")],
            "newrangedweapontypes" => &[("gameplay", "rangedweaponbackstabs")],
            "extendedmerc" => &[("gameplay", "saveall")],
            _ => &[],
        };
        let mut inputs: Vec<Input> = flags.iter().map(|&(s, k)| (s, k, FLAGS)).collect();
        if key == "extendedmerc" {
            inputs.push((
                "gameplay",
                "maxcountofmerc",
                &["-2147483648", "-1", "0", "50", "51", "2147483647"],
            ));
        }
        if key == "aggrmod" {
            for key in ["follow", "loseallyfast", "improve"] {
                inputs.push(("gameplay", key, &["-1", "0", "1"]));
            }
        }
        inputs
    }

    fn combinations(inputs: &[Input]) -> Vec<Vec<(&'static str, &'static str, &'static str)>> {
        let mut out = vec![Vec::new()];
        for &(section, key, values) in inputs {
            out = out
                .into_iter()
                .flat_map(|pairs| {
                    values.iter().map(move |&value| {
                        let mut next = pairs.clone();
                        next.push((section, key, value));
                        next
                    })
                })
                .collect();
        }
        out
    }

    #[test]
    fn inventory_covers_feature_combinations() {
        let mut covered: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
        for (name, write) in writes() {
            let owner = name.split('=').next().unwrap().to_owned();
            covered
                .entry(owner)
                .or_default()
                .extend(write.site().ru()..write.site().ru() + write.size());
        }
        let mut missing = BTreeSet::new();
        for feature in &FEATURES {
            let Some(build) = feature.build else { continue };
            let owner = format!("[{}] {}", feature.section, feature.key.trim_end());
            let covered = covered.get(&owner).cloned().unwrap_or_default();
            let inputs = inputs(feature.key);
            for pairs in combinations(&inputs) {
                let config = values(&pairs);
                let ctx = Ctx::inventory(&config, no_log);
                for value in i8::MIN..=i8::MAX {
                    if registry::skipped(feature, value, &config).is_some() {
                        continue;
                    }
                    let built = match build(&ctx, value) {
                        Ok(writes) => writes,
                        Err(e) => {
                            missing.insert(format!("{owner}={value}: build failed: {e}"));
                            continue;
                        }
                    };
                    for write in built.into_iter().map(Write::Patch).chain(ctx.ranges.take()) {
                        if (write.site().ru()..write.site().ru() + write.size())
                            .any(|at| !covered.contains(&at))
                        {
                            missing.insert(format!(
                                "{owner}: {:#x}+{}",
                                write.site().ru(),
                                write.size()
                            ));
                        }
                    }
                }
                for (section, key) in ctx.values.flags.borrow().iter() {
                    assert!(
                        inputs.iter().any(|(s, k, _)| section == s && key == k),
                        "{owner}: [{section}] {key} has no combination domain"
                    );
                }
            }
        }
        assert!(missing.is_empty(), "unlisted feature writes: {missing:#?}");
    }

    #[test]
    fn inventory_marks_ranges_without_expected_bytes() {
        let inventory = crate::writes();
        for (owner, site, size) in [
            (
                "[speedrun] timervars=1",
                eix_sites::C_DIPLOMACY_GET_HOSTILE_MASK__1D,
                2,
            ),
            ("[gameplay] aggrmod=1", eix_sites::DATA_799CF8, 4),
            ("[speed] gssetvar=1", eix_sites::DATA_78FF34, 4),
            (
                "[gameplay] effectredraws=1",
                eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_SPELL_RECORD__2C0,
                736,
            ),
        ] {
            let (_, write) = inventory
                .iter()
                .find(|(name, w)| name == owner && w.site() == site)
                .unwrap_or_else(|| panic!("{owner}: missing range at {site:#x}"));
            assert_eq!(write, &crate::Write::Range { site, size }, "{owner}");
        }
        assert!(matches!(&inventory[0].1, crate::Write::Patch(_)));
    }

    #[test]
    fn inventory_covers_the_timer_word() {
        let inventory = writes();
        for mode in 1..=3 {
            let owner = format!("[speedrun] timervars={mode}");
            for at in eix_sites::C_DIPLOMACY_GET_HOSTILE_MASK__1D.ru()
                ..eix_sites::C_DIPLOMACY_GET_HOSTILE_MASK__1D.ru() + 2
            {
                assert!(
                    inventory.iter().any(|(name, w)| name == &owner
                        && (w.site().ru()..w.site().ru() + w.size()).contains(&at)),
                    "{owner}: timer byte {at:#x} is unlisted"
                );
            }
        }
    }

    #[test]
    fn inventory_covers_run_time_globals() {
        let inventory = writes();
        for (owner, site) in [
            ("[gameplay] aggrmod=1", eix_sites::DATA_799CF8),
            ("[speed] gssetvar=1", eix_sites::DATA_78FF34),
        ] {
            for at in site.ru()..site.ru() + 4 {
                assert!(
                    inventory.iter().any(|(name, w)| name == owner
                        && (w.site().ru()..w.site().ru() + w.size()).contains(&at)),
                    "{owner}: byte {at:#x} is unlisted"
                );
            }
        }
    }

    #[test]
    fn font_failure_logs_the_ansi_path() {
        static MESSAGE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
        fn log(_: Level, _: &str, message: &str, _: &str) {
            *MESSAGE.lock().unwrap() = message.to_owned();
        }
        let path = b"eix_missing_font_\xE9.ttf";
        perform(Action::AddFont(path.to_vec()), log);
        assert_eq!(
            *MESSAGE.lock().unwrap(),
            format!(
                "changefont: the font file {} was not added",
                eix_engine::win::narrow(&from_ansi(path))
            )
        );
    }

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
