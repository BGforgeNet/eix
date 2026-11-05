//! ShowTutorialDefault (`[MOD] ShowTutorialDefault`): the "show tutorial" option takes the mod's value while the
//! registry has none stored. `GameOptions::LoadFromRegistry` passes the option's current value (on) as the default
//! of its registry read (0x45C010); that load becomes `mov ecx, <value>`. The game writes the option back at exit,
//! so from then on the stored value, the player's choice included, wins.

use eix_engine::group::SiteWrite;

use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// `mov ecx, [esi + 0x208]`: the option's current value, pushed as the read's default.
const LOAD_DEFAULT: [u8; 6] = [0x8B, 0x8E, 0x08, 0x02, 0x00, 0x00];

/// Off while the mod's config.reg does not set the key: the game's default stands.
pub(super) fn off(setup: &Setup) -> Option<String> {
    setup
        .options
        .config
        .mod_
        .show_tutorial_default
        .is_none()
        .then(|| "[MOD] ShowTutorialDefault is not set".to_owned())
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let Some(value) = setup.options.config.mod_.show_tutorial_default else {
        return Err("[MOD] ShowTutorialDefault is not set".to_owned().into());
    };
    let mut new = [0x90; 6];
    new[0] = 0xB9;
    new[1..5].copy_from_slice(&value.to_le_bytes());
    Ok(vec![Labelled {
        label: "ShowTutorialDefault".to_owned(),
        write: SiteWrite::bytes(
            eix_sites::GAME_OPTIONS_LOAD_FROM_REGISTRY__E00,
            &LOAD_DEFAULT,
            &new,
        )?,
    }])
}
