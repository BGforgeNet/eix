//! CommandLine (`game.exe addon <command>`): WinMain skips its own loop over the command line's words (from 0x4565D4
//! straight to 0x4567D5), so the addon's arguments are not taken for the game's.

use eix_engine::group::SiteWrite;

use super::{BuildError, Labelled, Setup};

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "CommandLine #0".to_owned(),
        // jle 0x4567D5
        write: SiteWrite::jmp(
            eix_sites::TEXT_4565D4,
            &[0x0F, 0x8E, 0xFB, 0x01, 0x00, 0x00],
            eix_sites::TEXT_4567D5,
        )?,
    }])
}
