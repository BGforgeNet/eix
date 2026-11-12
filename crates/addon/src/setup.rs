//! What the addon reads before it patches: the starter's registry values, the mod's config.reg and ai.reg,
//! `2utoexec.cfg`, and facts about the game folder and process, gathered into core's [`Options`].

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

use eix_core::ei_plugin::{self, AsiFile};
use eix_core::eix_ini::EixIni;
use eix_core::mod_files::{ResConfig, with_slash};
use eix_core::options::{
    AiReg, CommandLine, ConfigReg, Environment, Options, SETTING_DWORDS, SettingValue,
    SettingValues, Settings,
};
use eix_core::reg::{self, LoadError, Registry};
use eix_engine::ansi;
use eix_engine::dll::Dll;
use eix_engine::registry;
use eix_engine::win::{ansi_text, error_box, exit_process, from_ansi, narrow, to_ansi};

use crate::log;

/// `HKCU\<STARTER>`: the starter's own key; `path` is its folder.
pub(crate) const STARTER: &str = "Software\\Gipat.Ru\\EI_Starter";
/// The starter's options for the addon; `AddonPath` is the mod folder.
pub(crate) const SETTINGS: &str = "Software\\Gipat.Ru\\EI_Starter\\Settings";
const PATHS: &str = "Software\\Gipat.Ru\\EI_Starter\\EvilIslands\\Path Settings";
const GENERAL: &str = "Software\\Gipat.Ru\\EI_Starter\\EvilIslands\\General Settings";

/// The mod folder's `binkw32.dll`, as BinkPatch needs it.
#[derive(Debug, Default)]
pub(crate) enum ModBink {
    /// No such file, or it loads as the game's own module.
    #[default]
    None,
    /// Loaded as a module of its own.
    Separate(Dll),
    /// The file is there but does not load: the system's error code.
    LoadFailed(u32),
}

/// Everything read at start, for the patch groups.
#[derive(Debug, Default)]
pub(crate) struct Setup {
    /// The mod folder's `binkw32.dll`.
    pub(crate) mod_bink: ModBink,
    /// The predicates' inputs.
    pub(crate) options: Options,
    /// The mod folder in the ANSI code page, ending in `\`; empty when no mod is selected.
    pub(crate) mod_dir: Vec<u8>,
    /// The mod's archive table.
    pub(crate) res: ResConfig,
    /// The lines of `2utoexec.cfg`.
    pub(crate) autoexec_lines: Vec<Vec<u8>>,
    /// The `Console AutoExec` registry lines in the ANSI code page, which AutoExec runs before the file's.
    pub(crate) autoexec_registry_lines: Vec<Vec<u8>>,
    /// addon.log's level: the registry's `LogLevel`, clamped to 0..=4 (default 1).
    pub(crate) log_level: i32,
    /// The console logger's level: `ConsoleLogLevel`, clamped to 0..=4 (default 3).
    pub(crate) console_log_level: i32,
}

/// A log level under [`SETTINGS`]: a DWORD read as signed and clamped to 0..=4, else `default`.
fn log_level(name: &str, default: i32) -> i32 {
    registry::hkcu_dword(SETTINGS, name).map_or(default, |v| (v as i32).clamp(0, 4))
}

/// ANSI bytes as a path for std.
pub(crate) fn path(ansi: &[u8]) -> PathBuf {
    PathBuf::from(OsString::from_wide(&from_ansi(ansi)))
}

/// A file or folder name from std in the ANSI code page.
pub(crate) fn ansi(name: &std::ffi::OsStr) -> Vec<u8> {
    to_ansi(&name.encode_wide().collect::<Vec<u16>>())
}

/// `bytes` up to their first NUL as a C string, as a C string the game takes ends there.
pub(crate) fn c_string_until_nul(bytes: &[u8]) -> std::ffi::CString {
    let end = bytes.iter().position(|&c| c == 0).unwrap_or(bytes.len());
    // The slice holds no NUL, so `new` cannot fail.
    std::ffi::CString::new(&bytes[..end]).unwrap_or_default()
}

/// A registry string in the ANSI code page.
fn reg_ansi(key: &str, name: &str) -> Vec<u8> {
    to_ansi(&registry::hkcu_string(key, name))
}

/// The mod folder: `Settings\AddonPath` with a `\` added.
pub(crate) fn mod_dir() -> Vec<u8> {
    with_slash(reg_ansi(SETTINGS, "AddonPath"))
}

/// The starter's folder with a `\` added: `EI_Starter\path`, else the
/// folder of addon.dll's file. Empty only where the system cannot name that file; paths built on it are then relative
/// to the working folder, as the original's.
pub(crate) fn starter_folder() -> Vec<u8> {
    let mut starter = reg_ansi(STARTER, "path");
    if starter.is_empty() {
        match eix_engine::dll::own_path() {
            Ok(file) => {
                let file = path(&to_ansi(&file));
                if let Some(folder) = file.parent() {
                    starter = ansi(folder.as_os_str());
                }
            }
            Err(e) => log::error("options", &format!("the addon's own folder: {e}"), ""),
        }
    }
    with_slash(starter)
}

/// Where addon.log goes, and errors.log and `Dumps` beside it: the starter's folder.
pub(crate) fn log_path() -> PathBuf {
    path(&[&starter_folder()[..], b"addon.log"].concat())
}

/// The registry's `FullDump` (default 0): a crash dump holds the whole memory.
pub(crate) fn full_dump() -> bool {
    registry::hkcu_dword(SETTINGS, "FullDump").is_some_and(|v| v != 0)
}

fn setting_values() -> SettingValues {
    let mut values = SettingValues::new();
    for name in SETTING_DWORDS {
        if let Some(v) = registry::hkcu_dword(SETTINGS, name) {
            values.insert(name.to_owned(), SettingValue::Dword(v));
        }
    }
    if let Some(lines) = registry::hkcu_multi_string(SETTINGS, "Console AutoExec") {
        let lines = lines.iter().map(|l| narrow(l)).collect();
        values.insert(
            "Console AutoExec".to_owned(),
            SettingValue::MultiText(lines),
        );
    }
    let video = registry::hkcu_string(GENERAL, "VideoResolution");
    if !video.is_empty() {
        values.insert(
            "VideoResolution".to_owned(),
            SettingValue::Text(narrow(&video)),
        );
    }
    values
}

/// eix.ini of the mod folder `mod_dir` (ending in `\`, empty for no mod); the defaults without a mod or a file.
fn eix_ini(mod_dir: &[u8]) -> EixIni {
    if mod_dir.is_empty() {
        return EixIni::default();
    }
    let file = [mod_dir, b"eix.ini"].concat();
    let shown = ansi_text(&file);
    if !path(&file).is_file() {
        log::info("options", "no eix.ini: its defaults", &shown);
        return EixIni::default();
    }
    log::info("options", "eix.ini", &shown);
    let (ini, warnings) = EixIni::read(|section, key, default| {
        ansi::profile_int(section.as_bytes(), key.as_bytes(), default, &file)
    });
    for w in &warnings {
        log::warn("options", w, &shown);
    }
    ini
}

/// Reads a registry-format file. `None` where the original's loader fails (no such file, a folder, an empty file or
/// one without the format's header), which stops the game. A file with the header but a body that does not parse is
/// logged and reads as empty: the original reads it unchecked (garbage, or a crash on an offset outside the file).
fn read_reg(file: &[u8], what: &str) -> Option<Registry> {
    let shown = ansi_text(file);
    let data = match std::fs::read(path(file)) {
        Ok(data) => data,
        Err(e) => {
            log::error("options", &format!("cannot read {what}: {e}"), &shown);
            return None;
        }
    };
    match Registry::load(&data) {
        Ok(reg) => {
            log::info("options", &format!("read {what}"), &shown);
            Some(reg)
        }
        Err(LoadError::Unusable) => {
            log::error("options", &format!("{what}: no registry header"), &shown);
            None
        }
        Err(e @ LoadError::Malformed(_)) => {
            log::error("options", &format!("{what}: {e}"), &shown);
            Some(Registry::default())
        }
    }
}

/// The lines of `2utoexec.cfg` in the working folder, where the game runs; none when it is missing.
fn autoexec_lines() -> Vec<Vec<u8>> {
    match std::fs::read("2utoexec.cfg") {
        Ok(data) => {
            log::info("options", "read 2utoexec.cfg", "2utoexec.cfg");
            let mut lines: Vec<Vec<u8>> = data
                .split(|&c| c == b'\n')
                .map(|l| l.strip_suffix(b"\r").unwrap_or(l).to_vec())
                .collect();
            // A line end at the end of the file starts no line, as for getline.
            if lines.last().is_some_and(Vec::is_empty) {
                lines.pop();
            }
            lines
        }
        Err(_) => Vec::new(),
    }
}

/// The original's `*.asi` scan of the working folder: whether the "moved patches" (table T3) run. An
/// EI_Plugin build the addon does not support, or an unreadable file named like one, turns them off.
fn asi_scan_passes() -> bool {
    let Ok(entries) = std::fs::read_dir(".") else {
        return true;
    };
    let mut passes = true;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.to_ascii_lowercase().ends_with(".asi") {
            continue;
        }
        match std::fs::read(e.path()) {
            Err(err) => {
                if ei_plugin::name_is_plugin(&name) {
                    log::warn(
                        "options",
                        &format!("Failed to read ei plugin ({err}). Moved patches are disabled"),
                        &name,
                    );
                    passes = false;
                }
            }
            Ok(data) => match ei_plugin::classify(&data) {
                AsiFile::Other => {}
                AsiFile::Supported => log::info("options", "supported EI_Plugin.asi", &name),
                AsiFile::Unsupported => {
                    log::warn(
                        "options",
                        "Not supported EI_Plugin.asi version. Moved patches are disabled",
                        &name,
                    );
                    passes = false;
                }
            },
        }
    }
    passes
}

/// Stops the game with `text` in a box, as the original's fatal errors do.
fn fatal(text: &str) -> ! {
    log::error("options", text, "");
    error_box("addon.dll", text);
    exit_process(1)
}

/// The mod's `binkw32.dll` when it loads as a module other than `game`'s.
fn mod_bink(file: &std::path::Path, game: Option<&Dll>) -> ModBink {
    if !file.is_file() {
        return ModBink::None;
    }
    let wide: Vec<u16> = file.as_os_str().encode_wide().collect();
    match Dll::load(&wide) {
        Ok(dll) if game.is_some_and(|g| g.handle() == dll.handle()) => ModBink::None,
        Ok(dll) => ModBink::Separate(dll),
        Err(code) => ModBink::LoadFailed(code),
    }
}

/// The folder `--moddir <name>` selects: the starter's folder (`EI_Starter\path`, else the
/// folder of addon.dll), `Mods\`, the name, and a `\` when missing.
pub(crate) fn named_mod_dir(name: &[u8]) -> Vec<u8> {
    with_slash([&starter_folder()[..], b"Mods\\", name].concat())
}

/// Reads everything for the registry's mod folder. `exe_enables_t2` is the game.exe check of table T2, made by the
/// caller.
pub(crate) fn read(exe_enables_t2: bool) -> Setup {
    read_for(exe_enables_t2, mod_dir())
}

/// Reads everything for `mod_dir` (ending in `\`, or empty for no mod): options from their defaults, as the
/// original's config read does whether the folder comes from the registry or from `--moddir`.
pub(crate) fn read_for(exe_enables_t2: bool, mod_dir: Vec<u8>) -> Setup {
    let settings = Settings::from_values(&setting_values());
    let mut setup = Setup {
        mod_dir: mod_dir.clone(),
        ..Setup::default()
    };
    let mut ai_failure = None;
    if mod_dir.is_empty() {
        log::info("options", "no mod selected", "Settings\\AddonPath");
    } else {
        log::info("options", "mod folder", &ansi_text(&mod_dir));
        let config_file = [&mod_dir[..], b"config.reg"].concat();
        let Some(config_reg) = read_reg(&config_file, "config.reg") else {
            fatal(&reg::config_load_error(&ansi_text(&config_file)));
        };
        setup.options.config = ConfigReg::from_registry(&config_reg);
        let ai = &setup.options.config.config.ai_reg;
        if !ai.is_empty() {
            let ai_file = [&mod_dir[..], &ai[..]].concat();
            match read_reg(&ai_file, "ai.reg") {
                Some(ai_reg) => setup.options.ai = AiReg::from_registry(&ai_reg),
                // The original reads ai.reg after the checks below, so their boxes come first.
                None => ai_failure = Some(reg::ai_load_error(&ansi_text(&ai_file))),
            }
        }
        setup.res = ResConfig::new(
            &setup.options.config.res,
            &mod_dir,
            &reg_ansi(PATHS, "RESOURCES PATH"),
            &reg_ansi(PATHS, "CD-ROM PATH"),
        );
    }
    setup.options.settings = settings;
    setup.options.eix = eix_ini(&mod_dir);
    setup.autoexec_lines = autoexec_lines();
    setup.autoexec_registry_lines = registry::hkcu_multi_string(SETTINGS, "Console AutoExec")
        .map(|lines| lines.iter().map(|l| to_ansi(l)).collect())
        .unwrap_or_default();
    setup.log_level = log_level("LogLevel", 1);
    setup.console_log_level = log_level("ConsoleLogLevel", 3);
    if !mod_dir.is_empty()
        && let Err(text) = ei_plugin::check_addon_version(&setup.options.config.mod_.addon_version)
    {
        fatal(&text);
    }
    let zone = &setup.options.config.mp_zone;
    if !mod_dir.is_empty() && zone.duplicate_name().is_some() {
        let file = ansi_text(&[&mod_dir[..], b"config.reg"].concat());
        fatal(&format!("Duplicate zone name found in file: {file}"));
    }
    if let Some(text) = ai_failure {
        fatal(&text);
    }
    let asi_scan_passes = asi_scan_passes();
    if !asi_scan_passes {
        let needs = &setup.options.config.mod_.addon_version;
        // A version that does not parse has already stopped the game above.
        if ei_plugin::parse_version(needs).is_some_and(|v| v >= ei_plugin::PLUGIN_STRICT_SINCE) {
            fatal(
                "Incompatible (old) version of EI_Plugin found! Please, update it and try again.",
            );
        }
    }
    let bink = Dll::open(&"binkw32.dll".encode_utf16().collect::<Vec<u16>>()).ok();
    if !mod_dir.is_empty() {
        setup.mod_bink = mod_bink(
            &path(&[&mod_dir[..], b"binkw32.dll"].concat()),
            bink.as_ref(),
        );
    }
    setup.options.env = Environment {
        exe_enables_t2,
        asi_scan_passes,
        mod_loaded: !mod_dir.is_empty(),
        bink_dll_in_mod: !matches!(setup.mod_bink, ModBink::None),
        game_bink_loaded: bink.is_some(),
        bink_open_exported: bink
            .as_ref()
            .is_some_and(|d| d.address(c"_BinkOpen@8").is_some()),
        autoexec_file_commands: setup.autoexec_lines.len(),
        // The command line's parse sets it (`cli::early`), after this read.
        command_line: CommandLine::None,
    };
    setup
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_paths_round_trip() {
        let p = path(b"C:\\Mods\\inv\\config.reg");
        assert_eq!(p.to_str(), Some("C:\\Mods\\inv\\config.reg"));
        assert_eq!(ansi(p.as_os_str()), b"C:\\Mods\\inv\\config.reg");
    }
}
