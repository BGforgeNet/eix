//! SetDefaultRate (registry `Rate` = 2500): the game's default network rate 0x783E34 is written with 2500, the
//! value it already holds in RU 1.06's data, so the write changes nothing there.

use super::{BuildError, Labelled, Setup, bytes};

const RATE: [u8; 4] = 2500u32.to_le_bytes();

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![bytes(
        "SetDefaultRate #0",
        eix_sites::G_REMOTE_RATE,
        &RATE,
        &RATE,
    )?])
}
