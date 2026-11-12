//! Types shared by SpellAddon's hook, startup and patch application.

use eix_sites::Site;

/// How serious a log line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Normal progress.
    Info,
    /// Something is off, but SpellAddon goes on.
    Warn,
    /// Something failed.
    Error,
}

/// Writes one addon.log line: the level, the subsystem (`spelladdon` or `site`), the message, and the site or key
/// it concerns (empty for none).
pub type Log = fn(Level, &str, &str, &str);

/// The bytes addon.dll wrote at exactly a site, if any: a SpellAddon write that covers them lands over them, as the
/// original's copy does.
pub type Earlier = fn(Site) -> Option<Vec<u8>>;

/// What the hook needs when it runs.
#[derive(Debug)]
pub(crate) struct Setup {
    pub(crate) mod_dir: Vec<u8>,
    pub(crate) log: Log,
    pub(crate) addon: Earlier,
}
