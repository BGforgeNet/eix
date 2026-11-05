//! ConfigLightsIni: a zone's lights file comes from the mod when `[CONFIG]` names it (`lights<X>.ini=<path>`): the
//! call of the INI reader 0x695BF0 at 0x5639BE gets the mod's file for the name after the path's last `\`, lowered,
//! against the keys as written: as in the original, a key with an upper-case letter never matches.

use std::sync::OnceLock;

use eix_core::mod_files::{from_last_slash, lower};

use super::arg_path::rewrite_first_arg;
use super::{BuildError, Labelled, Setup};

/// By key name as written, the mod's file.
static LIGHTS: OnceLock<Vec<(Vec<u8>, Vec<u8>)>> = OnceLock::new();

fn rule(game: &[u8]) -> Option<Vec<u8>> {
    let name = lower(&from_last_slash(game)?[1..]);
    LIGHTS
        .get()?
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, path)| path.clone())
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let files = setup
        .options
        .config
        .config
        .lights_ini
        .iter()
        .map(|(key, value)| (key.clone(), [&setup.mod_dir[..], &value[..]].concat()))
        .collect();
    // The groups are built once per process; a second set would find the first's table in place.
    let _ = LIGHTS.set(files);
    Ok(vec![rewrite_first_arg(
        "Lights.ini",
        eix_sites::C_WORLD_CLIENT_LOAD_LIGHTS__8E,
        [0xE8, 0x2D, 0x22, 0x13, 0x00],
        rule,
    )?])
}
