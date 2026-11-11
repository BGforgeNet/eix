//! DisableExitOnCrashReport (AntiCrash): the game's `FatalError` 0x480080 reports its text through the addon's error
//! report instead of the message box (its call at 0x4800A7 is dropped), and no longer exits (9 NOPs over the error
//! counter test and `exit(-1)`). `CWorldClient::Segment` 0x5641F0 no longer calls 0x665A10 before its own
//! `FatalError`.

use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use super::{BuildError, Labelled, Setup, applied_write};
use crate::{log, report};

/// `FatalError`'s message buffer, at its ESP when it calls the message box.
const BUFFER: usize = 0x400;

extern "C" fn handler(_: usize, ctx: &mut Context) {
    let start = ctx.stack();
    let mut text = Vec::new();
    for i in 0..BUFFER / 4 {
        let Some(word) = ctx.stack_at(start + 4 * i) else {
            break;
        };
        let bytes = word.to_le_bytes();
        if let Some(end) = bytes.iter().position(|&c| c == 0) {
            text.extend_from_slice(&bytes[..end]);
            break;
        }
        text.extend_from_slice(&bytes);
    }
    report::error(&text);
}

/// The message box call in `FatalError`.
const BOX_CALL: Site = eix_sites::FATAL_ERROR__27;

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    report::init(&setup.options);
    // The hook drops the site's instructions, so it may stand on GraphGenPatches #1 as well as on the original call.
    let site = match applied_write(BOX_CALL) {
        Some(earlier) => {
            log::warn(
                "groups",
                "0x004800A7 holds an earlier group's patch (GraphGenPatches #1); this group's hook replaces it, as in the original",
                "DisableExitOnCrashReport #0",
            );
            earlier
        }
        // call 0x4560D0, the message box
        None => vec![0xE8, 0x24, 0x60, 0xFD, 0xFF],
    };
    Ok(vec![
        Labelled {
            label: "DisableExitOnCrashReport #0".to_owned(),
            write: hook::context_hook(BOX_CALL, &site, Mode::Drop, handler, 0)?,
        },
        Labelled {
            label: "DisableExitOnCrashReport #1".to_owned(),
            // jl 0x4800D2; push -1; call exit
            write: SiteWrite::nop(
                eix_sites::FATAL_ERROR__49,
                &[0x7C, 0x07, 0x6A, 0xFF, 0xE8, 0xD5, 0x8C, 0x26, 0x00],
            ),
        },
        Labelled {
            label: "DisableExitOnCrashReport #2".to_owned(),
            // call 0x665A10
            write: SiteWrite::nop(
                eix_sites::C_WORLD_CLIENT_SEGMENT__140,
                &[0xE8, 0xDB, 0x16, 0x10, 0x00],
            ),
        },
    ])
}
