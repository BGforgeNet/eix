//! MultipleLoadingMobs: a mob file loaded into a zone that already has objects skips the records whose ids the world
//! holds, and gives records without an id (-1) a free one from 100 up.
#![expect(
    unsafe_code,
    reason = "reading the mob record at the hooked site and asking the server world for ids"
)]

use eix_core::rules::free_id;
use eix_engine::game::mob::MobSpec;
use eix_engine::game::world::{SERVER_WORLD, World};
use eix_engine::hook::{self, Context, Mode};

use super::memory::word;
use super::{BuildError, Labelled, Setup};

/// Where `ReadMobChunks` goes on with the next record once one is filed (it clears ESI first).
const NEXT_RECORD: usize = eix_sites::READ_MOB_CHUNKS__2B8;

/// Whether the server world has an object with `id`.
fn taken(id: i32) -> bool {
    // SAFETY: mob files load on the game's thread into the constructed server world.
    !unsafe { World::find_object(SERVER_WORLD.as_ptr(), id) }.is_null()
}

/// At 0x48213A, after a record (ESI) was read and before it is filed in the table at [EBP+8].
extern "C" fn record(_: usize, ctx: &mut Context) {
    let spec = core::ptr::with_exposed_provenance_mut::<MobSpec>(ctx.esi as usize);
    // SAFETY: at 0x48213A ESI is the record just read, owned by the reader.
    let id = unsafe { (*spec).id };
    if id != -1 && taken(id) {
        // The original leaves the record allocated; nothing refers to it, so it is freed.
        // SAFETY: the reader drops its only pointer to the record at NEXT_RECORD.
        unsafe { MobSpec::delete(spec) };
        ctx.resume = NEXT_RECORD;
        return;
    }
    if id == -1 {
        let id = free_id(taken);
        // SAFETY: as above.
        unsafe { (*spec).id = id };
    }
    // The replaced `mov edi, [ebp+8]; mov eax, [esi+0x18]`.
    // SAFETY: EBP is ReadMobChunks' frame; its argument at +8 is the record table.
    ctx.edi = unsafe { word(ctx.ebp.wrapping_add(8) as usize) };
    // SAFETY: as for `id`.
    ctx.eax = unsafe { (*spec).id } as u32;
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "MultipleLoadingMobs #0".to_owned(),
        write: hook::context_hook(
            eix_sites::READ_MOB_CHUNKS__23A,
            &[0x8B, 0x7D, 0x08, 0x8B, 0x46, 0x18],
            Mode::Drop,
            record,
            0,
        )?,
    }])
}
