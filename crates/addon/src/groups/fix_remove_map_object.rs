//! FixRemoveMapObject: `CWorldServer::RemoveObject` 0x5935C0 reports an id it has no object for and returns, where
//! the game went on with the missing object and crashed.

use eix_engine::hook::{self, Context, Mode};

use super::{BuildError, Labelled, Setup};
use crate::report;

/// `mov dword ptr [esp + 0x4C], 0`: the path taken when the find found nothing; ESI holds the id.
const SITE: usize = eix_sites::C_WORLD_SERVER_REMOVE_OBJECT__55;
const ORIGINAL: [u8; 8] = [0xC7, 0x44, 0x24, 0x4C, 0x00, 0x00, 0x00, 0x00];
/// The function's epilogue, which restores the registers and the SEH frame and returns.
const EPILOGUE: usize = eix_sites::C_WORLD_SERVER_REMOVE_OBJECT__407;

extern "C" fn handler(_: usize, ctx: &mut Context) {
    let text = format!(
        "RemoveMapObject: attempt to remove non-existent object with ID {}",
        ctx.esi as i32
    );
    report::error(text.as_bytes());
    ctx.resume = EPILOGUE;
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    report::init(&setup.options);
    Ok(vec![Labelled {
        label: "FixRemoveMapObject #0".to_owned(),
        write: hook::context_hook(SITE, &ORIGINAL, Mode::Drop, handler, 0)?,
    }])
}
