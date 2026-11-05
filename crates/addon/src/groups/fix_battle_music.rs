//! FixPlaingBattleMusicFromAntimagic: `CUnitServer::IsFighting` 0x54B580 no longer counts a unit whose spell in use
//! is of type 0x1A (antimagic) as fighting, so it starts no battle music: the `cmp eax, 0x1A` becomes a jump over
//! the test.

use eix_engine::group::SiteWrite;

use super::{BuildError, Labelled, Setup};

/// The first two bytes of `cmp eax, 0x1A; jne 0x54B600`; `jmp short +8` lands on 0x54B600, the not-casting path.
const SITE: usize = eix_sites::C_UNIT_SERVER_IS_FIGHTING__76;

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "FixPlaingBattleMusicFromAntimagic #0".to_owned(),
        write: SiteWrite::bytes(SITE, &[0x83, 0xF8], &[0xEB, 0x08])?,
    }])
}
