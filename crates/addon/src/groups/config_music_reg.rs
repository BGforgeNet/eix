//! ConfigMusicReg: the music player opens the mod's music.reg (`[CONFIG] music.reg`): the call of the registry file
//! reader 0x6945A0 at 0x6B477E gets the mod's path as its argument.

use super::{BuildError, Labelled, Setup, mod_file, path_as_first_arg};

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let path = mod_file(setup, &setup.options.config.config.music_reg)?;
    Ok(vec![path_as_first_arg(
        "Music.Reg",
        eix_sites::C_MUSIC_SYSTEM_LOAD_THEMES__6E,
        [0xE8, 0x1D, 0xFE, 0xFD, 0xFF],
        path,
    )?])
}
