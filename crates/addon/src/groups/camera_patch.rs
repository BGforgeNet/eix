//! CameraPatch: a camera path (`Camera::LoadPath`'s file open, the call of 0x7011E8 at 0x40ED42) comes from the mod's
//! `Camera\` folder when the mod has a file of that name.

use std::sync::OnceLock;

use eix_core::mod_files::from_last_slash;

use super::arg_path::rewrite_first_arg;
use super::{BuildError, Labelled, Setup};
use crate::setup::path;

static MOD_DIR: OnceLock<Vec<u8>> = OnceLock::new();

fn rule(game: &[u8]) -> Option<Vec<u8>> {
    let candidate = [&MOD_DIR.get()?[..], b"Camera", from_last_slash(game)?].concat();
    path(&candidate).is_file().then_some(candidate)
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    // The groups are built once per process; a second set would find the first's folder in place.
    let _ = MOD_DIR.set(setup.mod_dir.clone());
    Ok(vec![rewrite_first_arg(
        "CameraPatch",
        eix_sites::C_CAMERA_LOAD_PATH__72,
        [0xE8, 0xA1, 0x24, 0x2F, 0x00],
        rule,
    )?])
}
