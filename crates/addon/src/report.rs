//! The original's reports about game data and script calls, and its console logger.
//!
//! - The bug fixes' report: ai.reg's `[Addon]` keys choose where it goes:
//!   `LogErrorsInFile` to `errors.log` beside addon.log, `LogErrorsInConsole` to the game's console as
//!   `[12:00:01][ERR] <message>`, `LogErrorsInChat` to every player as `[ERR] <message>` when this game is a network
//!   client with a server.
//! - The script commands' reports (and two warnings): with `EnableShowingScriptErrors`, to the console as
//!   `[12:00:01][ERR] <command>: <message>` or `[12:00:01][WRN] <message>`.
//!
//! The console lines go through the console logger, which drops a line below its level: the registry's
//! `ConsoleLogLevel` (default 3; 0 debug to 3 error, 4 nothing) or the console's `addon debug console`. The console
//! and errors.log lines take the original's form.
#![expect(
    unsafe_code,
    reason = "printing to the game's console and announcing to the game's players"
)]

use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Mutex, OnceLock};

use eix_core::options::Options;
use eix_engine::game::{console, net};
use eix_engine::win::local_time;

use crate::log::{self, Level};

/// The console logger's tag for `level`, in its short form.
fn tag(level: Level) -> &'static str {
    match level {
        Level::Debug => "DBG",
        Level::Info => "INF",
        Level::Warn => "WRN",
        Level::Error => "ERR",
    }
}

/// The console line for `message` at `time` (`local_time`'s fields): the time, the level's tag, a space.
fn console_line(time: [u16; 7], level: Level, message: &[u8]) -> Vec<u8> {
    let head = format!(
        "[{:02}:{:02}:{:02}][{}] ",
        time[3],
        time[4],
        time[5],
        tag(level)
    );
    [head.as_bytes(), message].concat()
}

/// The bytes errors.log gets for `message` at `time`: the line and its end as the original's text-mode file writes
/// them, every `\n` (the message's own included, as in a `FatalError` text) becoming CRLF.
fn file_line(time: [u16; 7], message: &[u8]) -> Vec<u8> {
    let [y, mo, d, h, mi, s, ms] = time;
    let head = format!("[{d:02}.{mo:02}.{y} {h:02}:{mi:02}:{s:02}.{ms:03}]\t");
    eix_core::cli::crt_text(&[head.as_bytes(), message, b"\n"].concat())
}

/// Where a report goes, from ai.reg.
#[derive(Debug, Clone, Copy, Default)]
struct Targets {
    file: bool,
    console: bool,
    chat: bool,
    scripts: bool,
}

static TARGETS: OnceLock<Targets> = OnceLock::new();
static ERRORS_LOG: OnceLock<PathBuf> = OnceLock::new();
static ERRORS_FILE: Mutex<Option<File>> = Mutex::new(None);
/// The console logger's level: a line whose level ranks below it is dropped.
static CONSOLE_THRESHOLD: AtomicI32 = AtomicI32::new(3);

/// The console logger's level, 0 to 4.
pub(crate) fn console_threshold() -> i32 {
    CONSOLE_THRESHOLD.load(Ordering::Relaxed)
}

/// Sets the console logger's level, clamped to 0..=4.
pub(crate) fn set_console_threshold(level: i32) {
    CONSOLE_THRESHOLD.store(level.clamp(0, 4), Ordering::Relaxed);
}

/// Takes the reports' targets from the options; a group that reports calls it when it is built.
pub(crate) fn init(options: &Options) {
    let a = &options.ai.addon;
    let _ = TARGETS.set(Targets {
        file: a.log_errors_in_file != 0,
        console: a.log_errors_in_console != 0,
        chat: a.log_errors_in_chat != 0,
        scripts: a.enable_showing_script_errors != 0,
    });
    let _ = ERRORS_LOG.set(crate::setup::log_path().with_file_name("errors.log"));
}

/// Opens errors.log beside `addon_log` for appending, creating it empty: the original opens both logs at start-up,
/// whatever `LogErrorsInFile` says.
pub(crate) fn open_errors_log(addon_log: &Path) {
    let path = addon_log.with_file_name("errors.log");
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(f) => {
            *ERRORS_FILE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(f);
        }
        Err(e) => log::error(
            "report",
            &format!("cannot open errors.log: {e}"),
            &path.display().to_string(),
        ),
    }
}

fn targets() -> Targets {
    TARGETS.get().copied().unwrap_or_default()
}

/// Reports `message` (ANSI) where ai.reg says; from the game's thread, in play.
pub(crate) fn error(message: &[u8]) {
    let targets = targets();
    if targets.file {
        to_file(message);
    }
    if targets.console {
        to_console(Level::Error, message);
    }
    if targets.chat {
        to_chat(message);
    }
}

/// A script command's problem, `<command>: <message>` (ANSI), as an error; from the game's thread. The command
/// counts as done afterwards, so this answers true, as the original's report does.
pub(crate) fn script_error(command: &str, message: &[u8]) -> bool {
    if targets().scripts {
        to_console(Level::Error, &[command.as_bytes(), b": ", message].concat());
    }
    true
}

/// A script command's warning (ANSI); from the game's thread.
pub(crate) fn script_warning(message: &[u8]) {
    if targets().scripts {
        to_console(Level::Warn, message);
    }
}

fn to_file(message: &[u8]) {
    let Some(path) = ERRORS_LOG.get() else {
        return;
    };
    let mut file = ERRORS_FILE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if file.is_none() {
        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(f) => *file = Some(f),
            Err(e) => {
                log::error("report", &format!("cannot open errors.log: {e}"), "");
                return;
            }
        }
    }
    let line = file_line(local_time(), message);
    if let Some(f) = file.as_mut()
        && let Err(e) = f.write_all(&line)
    {
        log::error("report", &format!("cannot write errors.log: {e}"), "");
    }
}

/// Prints `message` through the console logger at `level`.
fn to_console(level: Level, message: &[u8]) {
    if level.rank() < console_threshold() {
        return;
    }
    // A message cut at a NUL, as the original's C string is.
    let end = message
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(message.len());
    let Ok(line) = CString::new(console_line(local_time(), level, &message[..end])) else {
        return;
    };
    // SAFETY: the build was checked before WinMain; reports come from the game's thread in play, console built.
    unsafe { console::print_line(&line) };
}

fn to_chat(message: &[u8]) {
    // SAFETY: the build was checked before WinMain; reports come from the game's thread.
    if !unsafe { net::is_multiplayer_client() } {
        return;
    }
    // SAFETY: as above.
    let Some(server) = (unsafe { net::live_server() }) else {
        return;
    };
    let Ok(text) = CString::new([b"[ERR] ", message].concat()) else {
        return;
    };
    // SAFETY: the live game server, on the game's thread.
    unsafe { net::announce(server, &text) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_take_the_originals_form() {
        let time = [2026, 11, 1, 7, 5, 9, 42];
        assert_eq!(
            console_line(time, Level::Error, b"text"),
            b"[07:05:09][ERR] text"
        );
        assert_eq!(
            console_line(time, Level::Warn, b"text"),
            b"[07:05:09][WRN] text"
        );
        assert_eq!(
            file_line(time, b"text"),
            b"[01.11.2026 07:05:09.042]\ttext\r\n"
        );
    }

    #[test]
    fn a_message_line_feed_becomes_crlf_in_errors_log() {
        // The original writes errors.log in text mode: a FatalError text's own "\n" ends in CRLF, then the line's.
        let time = [2026, 11, 4, 16, 29, 9, 713];
        assert_eq!(
            file_line(time, b"Can't begin\n"),
            b"[04.11.2026 16:29:09.713]\tCan't begin\r\n\r\n"
        );
    }
}
