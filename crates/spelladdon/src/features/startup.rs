//! The start-up diagnostics. `fhrunetrans` is in the registry's skip list, so it has no build.

use eix_engine::ansi;
use eix_engine::group::SiteWrite;
use eix_engine::registry::hkcu_value;
use eix_engine::win::to_ansi;

use super::{Action, BuildError, Ctx};
use crate::types::{Level, Log};

const MB_OK: u32 = 0;
const MB_HELP: u32 = 0x4000;
const REG_SZ: u32 = 1;
const REG_EXPAND_SZ: u32 = 2;
/// The size of the buffer the original reads `AddonPath` into.
const PATH_BUFFER: usize = 0x104;

/// `importantalerts` >= 1: a modal box with the starter's `AddonPath` and the caption "2"; without the value, the
/// caption "3" over the unwritten buffer, a run of 0xCC bytes. The game waits until it is closed.
pub(crate) fn important_alerts(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value >= 1 {
        ctx.actions.borrow_mut().push(Action::ImportantAlerts);
    }
    Ok(Vec::new())
}

pub(crate) fn show_important_alerts() {
    let read = hkcu_value("Software\\Gipat.Ru\\EI_Starter\\Settings", "AddonPath").filter(
        |(ty, bytes)| matches!(*ty, REG_SZ | REG_EXPAND_SZ) && bytes.len() <= 2 * PATH_BUFFER,
    );
    match read {
        Some((_, bytes)) => {
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&c| u16::from_le_bytes(c))
                .take_while(|&u| u != 0)
                .collect();
            ansi::message_box(&to_ansi(&units), b"2", MB_HELP);
        }
        // The stack filler up to the buffer's end; past it the original shows whatever the stack holds.
        None => {
            ansi::message_box(&[0xCC; PATH_BUFFER], b"3", MB_HELP);
        }
    }
}

/// The `.asi` files `versionalerts` does not warn about. The original's list (with its own name rewritten to
/// "EI_Plugin.asi") plus SpellAddonXa.asi, the file the built-in replaces.
const KNOWN_ASI: [&[u8]; 4] = [
    b"EI_Plugin.asi",
    b"mp3dec.asi",
    b"SpellAddonX.asi",
    b"SpellAddonXa.asi",
];

/// The box text for one file.
fn version_alert_text(name: &[u8]) -> Vec<u8> {
    [
        &b"Probably old version found with name:\n"[..],
        name,
        b"\nDelete it if it is older than newest version, site:\n",
        b"https://evilislandsaddon.forumotion.com/",
    ]
    .concat()
}

/// Whether `versionalerts` warns about the file `name`: a `.asi` in exactly that case, not on the known list.
fn version_alert_for(name: &[u8]) -> bool {
    name.ends_with(b".asi") && !KNOWN_ASI.contains(&name)
}

/// `versionalerts` = 1: one box per `.asi` file of the working folder that is not on the known list.
pub(crate) fn version_alerts(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value == 1 {
        ctx.actions.borrow_mut().push(Action::VersionAlerts);
    }
    Ok(Vec::new())
}

pub(crate) fn show_version_alerts(log: Log) {
    use std::os::windows::ffi::OsStrExt;
    // The original skips the first entry FindFirstFile returns, which in a game folder is ".", and std lists
    // neither "." nor "..": every file is looked at in both.
    let entries = match std::fs::read_dir(".") {
        Ok(e) => e,
        Err(e) => {
            log(
                Level::Warn,
                "spelladdon",
                &format!("versionalerts: cannot list the working folder: {e}"),
                "",
            );
            return;
        }
    };
    for entry in entries.flatten() {
        let name = to_ansi(&entry.file_name().encode_wide().collect::<Vec<u16>>());
        if version_alert_for(&name) {
            ansi::message_box(
                &version_alert_text(&name),
                b"Probably old version found",
                MB_OK,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_build_actions_without_showing_them() {
        use super::super::Action;
        use super::super::testing::{no_log, values};
        let values = values(&[]);
        let ctx = Ctx::new(&values, no_log);
        assert!(important_alerts(&ctx, 1).unwrap().is_empty());
        assert!(version_alerts(&ctx, 1).unwrap().is_empty());
        assert_eq!(
            ctx.actions.take(),
            [Action::ImportantAlerts, Action::VersionAlerts]
        );
    }

    #[test]
    fn version_alerts_pick_unknown_lowercase_asi_files() {
        assert!(version_alert_for(b"SpellAddonX20.asi"));
        assert!(version_alert_for(b"um-window.asi"));
        assert!(!version_alert_for(b"SpellAddonXa.asi"));
        assert!(!version_alert_for(b"SpellAddonX.asi"));
        assert!(!version_alert_for(b"EI_Plugin.asi"));
        assert!(!version_alert_for(b"mp3dec.asi"));
        assert!(!version_alert_for(b"MP3DEC.ASI"));
        assert!(version_alert_for(b"Mp3dec.asi"));
        assert!(!version_alert_for(b"asi"));
        assert_eq!(
            version_alert_text(b"x.asi"),
            b"Probably old version found with name:\nx.asi\nDelete it if it is older than newest version, \
              site:\nhttps://evilislandsaddon.forumotion.com/"
        );
    }
}
