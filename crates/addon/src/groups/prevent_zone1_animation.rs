//! PreventZone1Animation (`[MOD] PreventZone1Animation`): `CWorldServer::CreateUnits` 0x58EF20 always takes the
//! branch at 0x58FA13 (`je` becomes `jmp`), which skips zone1's start animation.

use super::{BuildError, Labelled, Setup, bytes};

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![bytes(
        "Prevent Zone1 Animation",
        eix_sites::C_WORLD_SERVER_CREATE_UNITS__AF3,
        &[0x74, 0x17],
        &[0xEB, 0x17],
    )?])
}
