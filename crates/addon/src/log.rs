//! `addon.log`: one line per event, tab-separated: time, level, subsystem, message, and the site or key it concerns
//! (`-` when none).
//!
//! Each line goes to the file in one write as it is logged, with nothing buffered in the process, so a crash or
//! `ExitProcess` loses no buffered line. A busy file lock drops the line, so a stopped thread cannot block logging.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

use eix_engine::group::SiteWrite;

/// Logs a site write as `site`, its label, `0x<address>+<length>`.
pub(crate) fn log_site(label: &str, write: &SiteWrite) {
    info(
        "site",
        label,
        &format!("0x{:08X}+{}", write.site(), write.size()),
    );
}

/// How serious an event is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    /// Detail only a debugging session wants.
    Debug,
    /// Normal progress.
    Info,
    /// Something is off, but the addon goes on as intended.
    Warn,
    /// Something failed.
    Error,
}

impl Level {
    /// Its rank against a level threshold, as the original's loggers number them: 0 debug to 3 error.
    pub(crate) fn rank(self) -> i32 {
        match self {
            Level::Debug => 0,
            Level::Info => 1,
            Level::Warn => 2,
            Level::Error => 3,
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        })
    }
}

static FILE: Mutex<Option<File>> = Mutex::new(None);

/// The threshold: an event whose [`Level::rank`] is below it is not written; 4 writes nothing. The registry's
/// `LogLevel` sets it at start (default 1, as the original's), the console's `addon debug log` later.
static THRESHOLD: AtomicI32 = AtomicI32::new(1);

/// The threshold, 0 to 4.
pub(crate) fn threshold() -> i32 {
    THRESHOLD.load(Ordering::Relaxed)
}

/// Sets the threshold, clamped to 0..=4.
pub(crate) fn set_threshold(level: i32) {
    THRESHOLD.store(level.clamp(0, 4), Ordering::Relaxed);
}

/// Opens `path` for appending; lines logged before this are lost, which only start-up's first lines can be.
pub(crate) fn open(path: &Path) -> std::io::Result<()> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    *FILE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(file);
    eix_engine::patch::set_warning_handler(|site, error| {
        warn(
            "patch",
            &format!("cannot restore page protection: {error}"),
            &format!("{site:#x}"),
        );
    });
    Ok(())
}

/// The line as it is written, without the line end.
pub(crate) fn format_line(
    time: [u16; 7],
    level: Level,
    subsystem: &str,
    message: &str,
    at: &str,
) -> String {
    let [y, mo, d, h, mi, s, ms] = time;
    let clean = |t: &str| t.replace(['\t', '\r', '\n'], " ");
    let at = if at.is_empty() {
        "-".to_owned()
    } else {
        clean(at)
    };
    format!(
        "{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}.{ms:03}\t{level}\t{subsystem}\t{}\t{at}",
        clean(message)
    )
}

/// Logs one event, unless its level is below the threshold or the file lock is busy.
pub(crate) fn write(level: Level, subsystem: &str, message: &str, at: &str) {
    if level.rank() < threshold() {
        return;
    }
    let mut guard = match FILE.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return,
    };
    if let Some(file) = guard.as_mut() {
        let line = format_line(eix_engine::win::local_time(), level, subsystem, message, at);
        // The log is the reporting channel itself; a failed write has nowhere else to go.
        let _ = file.write_all(format!("{line}\r\n").as_bytes());
    }
}

/// Logs at [`Level::Debug`].
pub(crate) fn debug(subsystem: &str, message: &str, at: &str) {
    write(Level::Debug, subsystem, message, at);
}

/// Logs at [`Level::Info`].
pub(crate) fn info(subsystem: &str, message: &str, at: &str) {
    write(Level::Info, subsystem, message, at);
}

/// Logs at [`Level::Warn`].
pub(crate) fn warn(subsystem: &str, message: &str, at: &str) {
    write(Level::Warn, subsystem, message, at);
}

/// Logs at [`Level::Error`].
pub(crate) fn error(subsystem: &str, message: &str, at: &str) {
    write(Level::Error, subsystem, message, at);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_has_five_tab_separated_fields() {
        let line = format_line(
            [2026, 11, 1, 8, 5, 9, 42],
            Level::Warn,
            "groups",
            "bad\tbytes\n",
            "",
        );
        assert_eq!(line, "2026-11-01 08:05:09.042\tWARN\tgroups\tbad bytes \t-");
        assert_eq!(
            format_line(
                [2026, 1, 2, 3, 4, 5, 6],
                Level::Info,
                "site",
                "Maps #1",
                "0x005954CE+5"
            ),
            "2026-01-02 03:04:05.006\tINFO\tsite\tMaps #1\t0x005954CE+5"
        );
    }

    #[test]
    fn lines_reach_the_file_at_once() {
        let dir = std::env::temp_dir().join(format!("eix-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("addon.log");
        open(&path).unwrap();
        info("main", "started", "");
        error("plugins", "cannot load", "x.dll");
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("\tINFO\tmain\tstarted\t-"));
        assert!(lines[1].ends_with("\tERROR\tplugins\tcannot load\tx.dll"));

        let guard = FILE.lock().unwrap();
        let (sent, received) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            info("main", "cleanup: the process is ending", "");
            sent.send(()).unwrap();
        });
        let completed = received.recv_timeout(std::time::Duration::from_secs(2));
        drop(guard);
        writer.join().unwrap();
        assert!(completed.is_ok(), "logging waited for the held file lock");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
