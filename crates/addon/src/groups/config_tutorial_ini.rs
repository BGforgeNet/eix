//! ConfigTutorialIni: both readers of tutorial.ini in the interface's set-up open the mod's (`[CONFIG]
//! tutorial.ini`): each hook keeps its `push` of the flags and leaves the path in EAX, which the next `push eax`
//! passes as the file name.

use super::{BuildError, Labelled, Setup, mod_file, path_in_eax};

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let path = mod_file(setup, &setup.options.config.config.tutorial_ini)?;
    Ok(vec![
        path_in_eax(
            "Tutorial.ini #1",
            eix_sites::C_INTERFACE_3D_CREATE__40C,
            &[0x68, 0x00, 0x40, 0x00, 0x00],
            path,
        )?,
        path_in_eax(
            "Tutorial.ini #2",
            eix_sites::C_INTERFACE_3D_SHUTDOWN__B8,
            &[0x68, 0x01, 0x10, 0x00, 0x00],
            path,
        )?,
    ])
}
