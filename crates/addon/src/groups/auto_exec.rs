//! AutoExec: before the game runs `exec autoexec.cfg` on the console, every `Console AutoExec` registry line, then
//! every line of `2utoexec.cfg`, runs as a console command.
//!
//! The three runs are `Execute` 0x5DC620 calls in `GameFlow::LoadGame` 0x57ABA0, `GameFlow::StartNewGame` 0x57BD10
//! and `GameFlow::ProcessRequest` 0x57C700. Both sources are read once, at start.
#![expect(unsafe_code, reason = "running console lines on the game's console")]

use std::ffi::CString;
use std::sync::OnceLock;

use eix_engine::game::console;
use eix_engine::hook::{self, Context};

use super::{BuildError, Labelled, Setup};

/// The three calls of `Execute` with `exec autoexec.cfg`.
const SITES: [(&str, usize, [u8; 5]); 3] = [
    (
        "AutoExec #1",
        eix_sites::GAME_FLOW_LOAD_GAME__41,
        [0xE8, 0x3A, 0x1A, 0x06, 0x00],
    ),
    (
        "AutoExec #2",
        eix_sites::GAME_FLOW_START_NEW_GAME__20,
        [0xE8, 0xEB, 0x08, 0x06, 0x00],
    ),
    (
        "AutoExec #3",
        eix_sites::GAME_FLOW_PROCESS_REQUEST__2A3,
        [0xE8, 0x78, 0xFC, 0x05, 0x00],
    ),
];

static LINES: OnceLock<Vec<CString>> = OnceLock::new();

extern "C" fn run_lines(_: usize, _: &mut Context) {
    for line in LINES.get().map(Vec::as_slice).unwrap_or_default() {
        // SAFETY: the game's thread, at a call of the executor, so the console's statics are constructed.
        unsafe { console::execute(line) };
    }
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    // A NUL inside a line ends it, as the C string the original passes does.
    let lines = setup
        .autoexec_registry_lines
        .iter()
        .chain(&setup.autoexec_lines)
        .map(|l| crate::setup::c_string_until_nul(l))
        .collect();
    let _ = LINES.set(lines);
    SITES
        .iter()
        .map(|(label, site, original)| {
            let (write, _) = hook::call_hook(*site, *original, run_lines, 0)?;
            Ok(Labelled {
                label: (*label).to_owned(),
                write,
            })
        })
        .collect()
}
