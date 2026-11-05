//! ConfigStreamSNReg: the streamed sound opens the mod's streamsn.reg (`[CONFIG] streamsn.reg`): the call of the
//! registry file reader 0x6945A0 at 0x6BAE8B gets the mod's path as its argument.

use super::{BuildError, Labelled, Setup, mod_file, path_as_first_arg};

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let path = mod_file(setup, &setup.options.config.config.streamsn_reg)?;
    Ok(vec![path_as_first_arg(
        "StreamSN.Reg",
        eix_sites::C_SOUND_MANAGER_SET_PATH__6B,
        [0xE8, 0x10, 0x97, 0xFD, 0xFF],
        path,
    )?])
}
