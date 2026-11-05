//! SkipDiscCheck: the starter's game.exe asks for the game disc at start-up unless the current drive carries the
//! disc's label. The test's `je` (0x456BB5) becomes the `jmp` GOG's Russian exe has there, the one byte in which the
//! two exes differ; GOG's exe already holds it, so nothing is written there.

use eix_engine::group::SiteWrite;

use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// `je` over the disc prompt, then the `mov edi, [0x73B670]` after it, which pins the site.
const CHECK: [u8; 8] = [0x74, 0x59, 0x8B, 0x3D, 0x70, 0xB6, 0x73, 0x00];
/// The same with `jmp`, as in GOG's exe.
const SKIP: [u8; 8] = [0xEB, 0x59, 0x8B, 0x3D, 0x70, 0xB6, 0x73, 0x00];

/// Why the fix is off when game.exe needs no change: GOG's exe.
pub(super) fn not_needed() -> Option<String> {
    crate::game::holds(eix_sites::TEXT_456BB5, &SKIP)
        .then(|| "game.exe starts without the disc already".to_owned())
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "SkipDiscCheck".to_owned(),
        write: SiteWrite::bytes(eix_sites::TEXT_456BB5, &CHECK, &SKIP)?,
    }])
}
