//! ConfigAiReg: WinMain opens the mod's ai.reg (`[CONFIG] ai.reg`) instead of the game's: at 0x456912 the hook keeps
//! `mov ecx, 0x7C53F8` and leaves the path in EAX, which the next `push eax` passes to the reader 0x5CE0F0.

use super::{BuildError, Labelled, Setup, mod_file, path_in_eax};

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let path = mod_file(setup, &setup.options.config.config.ai_reg)?;
    Ok(vec![path_in_eax(
        "Ai.Reg",
        eix_sites::TEXT_456912,
        &[0xB9, 0xF8, 0x53, 0x7C, 0x00],
        path,
    )?])
}
