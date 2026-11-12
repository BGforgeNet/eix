//! The crash handler the original installs at start-up : an unhandled exception stops the
//! game's other threads, writes a minidump to `Dumps\ei-<DD.MM.YYYY>-<n>.dmp` in the starter's folder, backs up the
//! hosting player's party ([`crate::auto_recover`]), shows the exception's code and address, and ends the process
//! with the exception code.
//!
//! The registry's `FullDump` (default 0) dumps the whole memory. Not reproduced: `Tracer` (default off).
//!
//! Ours also writes a text record of the crash to addon.log first ([`eix_core::crash_record`]), with the owner of
//! each address and the loaded modules; its box asks for a report at eix's issue tracker, naming addon.log and the
//! dump; and it keeps its filter the process's one when a plugin sets its own, running the plugin's first
//! ([`crash::keep_first`]).

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use eix_core::auto_recover::{box_line, crash_text};
use eix_core::crash_record::{self, MAX_RETURNS, Place};
use eix_engine::crash::{self, Crash};
use eix_engine::dll::module_at;
use eix_engine::win::{error_box, exit_process, from_ansi, narrow};

use crate::{auto_recover, log};

/// How often the watcher puts our filter back first. A crash in the time between a plugin setting its filter and
/// the next look goes to the plugin's alone.
const WATCH: Duration = Duration::from_millis(250);

/// How much of the crashing thread's stack the record searches for return addresses, in 4-byte words.
const STACK_WORDS: usize = 1024;

/// addon.log's path, the dump's path (empty when every name was taken), and whether to dump the whole memory.
static FILES: OnceLock<(PathBuf, PathBuf, bool)> = OnceLock::new();

/// The dump's file name for a day and a number: `ei-DD.MM.YYYY-n.dmp`.
fn dump_name(day: u16, month: u16, year: u16, n: u32) -> String {
    format!("ei-{day:02}.{month:02}.{year}-{n}.dmp")
}

/// The first `ei-<today>-<n>.dmp` in `folder` that does not exist yet, n from 0 to 999; none when all
/// are taken or the folder cannot be made.
fn dump_path(folder: &Path) -> Option<PathBuf> {
    if let Err(e) = std::fs::create_dir(folder)
        && e.kind() != std::io::ErrorKind::AlreadyExists
    {
        log::warn(
            "crash",
            &format!("cannot create the dump folder: {e}"),
            &folder.display().to_string(),
        );
        return None;
    }
    let [year, month, day, ..] = eix_engine::win::local_time();
    (0..1000)
        .map(|n| folder.join(dump_name(day, month, year, n)))
        .find(|p| !p.exists())
}

/// `address` with the module it lies in and what of ours put code there.
fn place(address: usize) -> Place {
    place_owned_at(address, address)
}

/// A return address: its owner is that of the call before it, which ends where the return address begins (a call
/// a patch group redirected returns just past its site).
fn return_place(address: usize) -> Place {
    place_owned_at(address, address.wrapping_sub(1))
}

fn place_owned_at(address: usize, owned: usize) -> Place {
    Place {
        address: address as u32,
        module: module_at(address).map(|(path, base)| {
            let path = narrow(&path);
            let name = path.rsplit(['\\', '/']).next().unwrap_or(&path).to_owned();
            (name, (address - base) as u32)
        }),
        owner: eix_engine::owners::of(owned),
    }
}

/// Writes a new dump and returns its attachment path only when the write succeeds.
fn write_dump(dump: &Path, write: impl FnOnce(&[u16]) -> Result<(), String>) -> String {
    if dump.as_os_str().is_empty() || dump.exists() {
        return String::new();
    }
    let wide: Vec<u16> = dump.as_os_str().encode_wide().collect();
    match write(&wide) {
        Ok(()) => {
            let path = dump.display().to_string();
            log::info("crash", "minidump written", &path);
            path
        }
        Err(e) => {
            log::error(
                "crash",
                &format!("the minidump was not written: {e}"),
                &dump.display().to_string(),
            );
            String::new()
        }
    }
}

fn handle(c: &Crash) {
    // Resolve modules while the other threads still run: a stopped thread could hold the loader lock.
    let returns: Vec<Place> = c
        .code_pointers_on_stack(STACK_WORDS)
        .into_iter()
        .filter(|&(_, before)| crash_record::follows_call(before))
        .take(MAX_RETURNS)
        .map(|(value, _)| return_place(value as usize))
        .collect();
    let registers = c.registers().map(|r| {
        [
            r.eax, r.ebx, r.ecx, r.edx, r.esi, r.edi, r.ebp, r.esp, r.eip, r.eflags,
        ]
    });
    for line in crash_record::lines(c.code, &place(c.address), c.access(), registers, &returns) {
        log::error("crash", &line, "");
    }
    // The renderer, its wrappers and 3DfpFPU.dll load after start-up: the crash's own list shows them.
    crate::environment::log_modules("crash");
    if let Some((code, at)) = c.chained_fault {
        log::error(
            "crash",
            &format!(
                "another module's exception filter faulted with 0x{code:08X} at {}: it was cut short",
                place(at)
            ),
            "",
        );
    }
    log::use_nonblocking_writes();
    c.stop_other_threads();
    let (log_path, dump, full) = FILES.get().cloned().unwrap_or_default();
    // The original writes no dump over a file of that name.
    let dump = write_dump(&dump, |wide| c.write_minidump(wide, full));
    // After the dump and before the box, as the original.
    let backup = box_line(&narrow(&from_ansi(&auto_recover::run())));
    error_box(
        "addon.dll",
        crash_text(
            c.code,
            c.address,
            &backup,
            &log_path.display().to_string(),
            &dump,
        ),
    );
    exit_process(c.code)
}

/// Installs the handler and logs where its dump goes: `Dumps` beside addon.log (`log_path`), in the starter's folder.
pub(crate) fn install(log_path: &Path, full_dump: bool) {
    let folder = log_path.parent().unwrap_or(log_path);
    let dump = dump_path(&folder.join("Dumps")).unwrap_or_default();
    log::info(
        "crash",
        "If game crashes, dump will be here",
        &dump.display().to_string(),
    );
    let _ = FILES.set((log_path.to_path_buf(), dump, full_dump));
    crash::install(handle);
    let watcher = std::thread::Builder::new()
        .name("eix crash filter".to_owned())
        .spawn(|| {
            loop {
                std::thread::sleep(WATCH);
                match crash::keep_first() {
                    Some(Ok(other)) => log::info(
                        "crash",
                        "another module set an exception filter: it runs before ours",
                        &place(other).to_string(),
                    ),
                    Some(Err(other)) => log::warn(
                        "crash",
                        "another module set an exception filter, one too many to keep: it does not run",
                        &place(other).to_string(),
                    ),
                    None => {}
                }
            }
        });
    if let Err(e) = watcher {
        log::warn(
            "crash",
            &format!("a later exception filter will replace ours: no watcher thread: {e}"),
            "",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dump_names_are_the_originals() {
        assert_eq!(dump_name(1, 11, 2026, 0), "ei-01.11.2026-0.dmp");
    }

    #[test]
    fn a_failed_dump_is_not_named_as_an_attachment() {
        let dump = std::env::temp_dir().join(format!("eix-missing-{}.dmp", std::process::id()));
        let attachment = write_dump(&dump, |_| Err("write refused".to_owned()));
        assert_eq!(attachment, "");
        let attachment = write_dump(&dump, |_| Ok(()));
        assert_eq!(attachment, dump.display().to_string());
    }
}
