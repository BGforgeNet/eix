//! AllowScriptsInMainMenu: the main menu zone's script runs.
//!
//! `GameFlow::StartZone` 0x57C460, for the menu (0x57C69F, right after the server world is set up), also turns on
//! the server world's first script queue ([`SCRIPTS_ACTIVE`]), which `CWorldServer::Update` otherwise leaves off
//! there.
#![expect(unsafe_code, reason = "setting the server world's script switch")]

use eix_engine::game::world::SCRIPTS_ACTIVE;
use eix_engine::hook::{self, Context, Mode};

use super::{BuildError, Labelled, Setup};

/// `mov edi, 4`.
const SITE: usize = eix_sites::GAME_FLOW_START_ZONE__23F;
const ORIGINAL: [u8; 5] = [0xBF, 0x04, 0x00, 0x00, 0x00];

extern "C" fn activate(_: usize, _: &mut Context) {
    // SAFETY: the hook runs on the game's thread inside StartZone, where the server world is constructed.
    unsafe { SCRIPTS_ACTIVE.write(1) };
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "AllowScriptsInMainMenu #0".to_owned(),
        write: hook::context_hook(SITE, &ORIGINAL, Mode::Before, activate, 0)?,
    }])
}
