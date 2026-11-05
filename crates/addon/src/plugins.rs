//! Mod plugins (`[DEV] Plugin`): DLLs with `int __cdecl EntryBeforePatch(const char* mod_path)`, called before the
//! patch tables, and `EntryAfterPatch`, called after them; 0 means success.
//!
//! The value, relative to the mod folder, names a DLL, a pattern of them (`*` and `?` in the file name), or a folder
//! whose `*.plg` files are all loaded; an empty value names the mod folder itself. A plugin that does not load, or
//! whose entry point returns a non-zero code, stops the game with a message, as in the original addon.
#![expect(unsafe_code, reason = "calling the plugins' entry points")]

use std::ffi::{CString, c_char};
use std::path::Path;

use eix_engine::dll::Dll;
use eix_engine::win::{error_box, exit_process, from_ansi};

use crate::log;
use crate::setup::{ansi, path};

type Entry = unsafe extern "C" fn(*const c_char) -> i32;

/// The loaded plugins, in load order.
#[derive(Debug, Default)]
pub(crate) struct Plugins(Vec<Dll>);

/// Whether `name` matches `pattern` with `*` and `?`, ignoring ASCII case, as FindFirstFile matches a file name.
pub(crate) fn matches(pattern: &[u8], name: &[u8]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some((b'*', rest)) => (0..=name.len()).any(|i| matches(rest, &name[i..])),
        Some((&p, rest)) => name.split_first().is_some_and(|(&c, tail)| {
            (p == b'?' || p.eq_ignore_ascii_case(&c)) && matches(rest, tail)
        }),
    }
}

/// The box text of a plugin that fails: `code` is the system's error code of a failed load, or what an entry point
/// returned.
fn failure_text(path: &str, code: impl std::fmt::Display) -> String {
    format!("Failed to load plugin {path}, error code: {code}")
}

/// The plugin files `value` names under `mod_dir`, in name order.
fn files(mod_dir: &[u8], value: &[u8]) -> Vec<Vec<u8>> {
    let full = [mod_dir, value].concat();
    let (dir, pattern) = if path(&full).is_dir() {
        (eix_core::mod_files::with_slash(full), b"*.plg".to_vec())
    } else {
        match full.iter().rposition(|&c| c == b'\\') {
            Some(i) => (full[..=i].to_vec(), full[i + 1..].to_vec()),
            None => (Vec::new(), full),
        }
    };
    let Ok(entries) = std::fs::read_dir(if dir.is_empty() {
        Path::new(".").to_path_buf()
    } else {
        path(&dir)
    }) else {
        return Vec::new();
    };
    let mut found: Vec<Vec<u8>> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| !t.is_dir()))
        .map(|e| ansi(&e.file_name()))
        .filter(|name| matches(&pattern, name))
        .map(|name| [&dir[..], &name[..]].concat())
        .collect();
    found.sort();
    found
}

impl Plugins {
    /// Loads the plugins `value` names under `mod_dir`.
    pub(crate) fn load(mod_dir: &[u8], value: &[u8]) -> Self {
        let mut loaded = Vec::new();
        let found = files(mod_dir, value);
        // A mod without plugins leaves the value empty: nothing to report then.
        if found.is_empty() && !value.is_empty() {
            log::warn(
                "plugins",
                "no plugin file found",
                &String::from_utf8_lossy(value),
            );
        }
        for file in found {
            let shown = String::from_utf8_lossy(&file).into_owned();
            log::info("plugins", "loading", &shown);
            match Dll::open(&from_ansi(&file)) {
                Ok(dll) => loaded.push(dll),
                Err(code) => {
                    log::error("plugins", &format!("cannot load: OS error {code}"), &shown);
                    error_box("addon.dll", failure_text(&shown, code));
                    exit_process(1);
                }
            }
        }
        Self(loaded)
    }

    /// Calls `export` (`EntryBeforePatch` or `EntryAfterPatch`) of every plugin that has it, with the mod folder.
    pub(crate) fn call(&self, export: &std::ffi::CStr, mod_dir: &[u8]) {
        let Ok(arg) = CString::new(mod_dir.to_vec()) else {
            log::error("plugins", "the mod path holds a NUL", "");
            return;
        };
        let name = export.to_string_lossy();
        for dll in &self.0 {
            // SAFETY: plugins export their entry points as `int __cdecl (const char*)`, which is this type.
            let Some(entry) = (unsafe { dll.proc::<Entry>(export) }) else {
                log::info("plugins", &format!("has no {name}"), dll.path());
                continue;
            };
            // SAFETY: the argument is a NUL-terminated string that outlives the call, as the entry point expects.
            let result = unsafe { entry(arg.as_ptr()) };
            let message = format!("{name} returned {result}");
            if result == 0 {
                log::info("plugins", &message, dll.path());
            } else {
                // The original reports a failing entry point with its load-failure text and ends the process with
                // the entry's code.
                log::error("plugins", &message, dll.path());
                error_box("addon.dll", failure_text(dll.path(), result));
                exit_process(result as u32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_failure_text_names_the_path_and_the_code() {
        assert_eq!(
            failure_text("C:\\mod\\a.dll", 126),
            "Failed to load plugin C:\\mod\\a.dll, error code: 126"
        );
        assert_eq!(
            failure_text("C:\\mod\\a.dll", -3),
            "Failed to load plugin C:\\mod\\a.dll, error code: -3"
        );
    }

    #[test]
    fn an_empty_value_names_the_mod_folder_itself() {
        let dir = std::env::temp_dir().join(format!("eix-plugins-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub.plg")).unwrap();
        for name in ["b.PLG", "a.plg", "c.dll"] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        let mut mod_dir = dir.to_string_lossy().into_owned().into_bytes();
        mod_dir.push(b'\\');
        let found = files(&mod_dir, b"");
        std::fs::remove_dir_all(&dir).unwrap();
        let names: Vec<&[u8]> = found.iter().map(|f| &f[mod_dir.len()..]).collect();
        assert_eq!(names, [b"a.plg".as_slice(), b"b.PLG".as_slice()]);
    }

    #[test]
    fn patterns_match_like_find_first_file() {
        assert!(matches(b"*.plg", b"a.PLG"));
        assert!(matches(b"eitweaks.dll", b"EItweaks.dll"));
        assert!(matches(b"e?t*.dll", b"eitweaks.dll"));
        assert!(!matches(b"*.plg", b"a.dll"));
        assert!(!matches(b"a", b"ab"));
        assert!(matches(b"*", b""));
    }
}
