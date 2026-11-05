//! DeagrPlayerOnLoose: in `AIViewer::Update`, a unit no longer keeps seeing another just because that one is on guard
//! and targets it (#1) or holds it as its aggression enemy (#0); and when a player's unit loses sight of another
//! player's unit it targets, its action is cleared.
#![expect(
    unsafe_code,
    reason = "reading the units at the hooked site and clearing a unit's action"
)]

use eix_engine::game::object::EngineObject;
use eix_engine::game::unit::{Unit, UnitServer};
use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, Mode};

use super::{BuildError, Labelled, Setup};

/// The "lost from sight" path at 0x5CAD02: EBX is the seeing unit, ESI the unit it lost.
extern "C" fn lost(_: usize, ctx: &mut Context) {
    let unit = core::ptr::with_exposed_provenance_mut::<Unit>(ctx.ebx as usize);
    let other = core::ptr::with_exposed_provenance_mut::<EngineObject>(ctx.esi as usize);
    // SAFETY: at 0x5CAD02 EBX and ESI are live units (the game reads their ids next) on the game's thread.
    let unit = unsafe { &*unit };
    let (player, controller) = (unit.player, unit.controller.cast::<UnitServer>());
    // SAFETY: as above; a unit the AI viewer updates has its server controller.
    let action = unsafe { &mut (*controller).action };
    // SAFETY: ESI is a live unit, as above.
    let other_player = unsafe { (*other.cast::<Unit>()).player };
    if action.target != other || player.is_null() || other_player.is_null() {
        return;
    }
    let target = core::mem::replace(&mut action.target, core::ptr::null_mut());
    action.kind = 0;
    action.source = 0;
    // The original stores 0 over the counted target without releasing it; dropping the reference keeps the count
    // right.
    // SAFETY: the action held one reference to `target`, non-null since it equals ESI, and gives it up here.
    unsafe { EngineObject::release(target) };
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    // #0: the `je` at 0x5CACFC becomes NOPs, and the two moves after it a call into the hook (which runs them first).
    let moves = [0x8B, 0x46, 0x0C, 0x8B, 0x4B, 0x0C];
    let call = hook::context_call(
        eix_sites::AI_VIEWER_UPDATE__302,
        &moves,
        Mode::Before,
        lost,
        0,
    )?;
    let original = [[0x0F, 0x84, 0x01, 0x01, 0x00, 0x00], moves].concat();
    let new = [&[0x90; 6][..], call.new_bytes()].concat();
    Ok(vec![
        Labelled {
            label: "DeagrPlayerOnLoose #0".to_owned(),
            write: SiteWrite::bytes(eix_sites::AI_VIEWER_UPDATE__2FC, &original, &new)?,
        },
        Labelled {
            label: "DeagrPlayerOnLoose #1".to_owned(),
            write: SiteWrite::nop(
                eix_sites::AI_VIEWER_UPDATE__2A0,
                &[0x0F, 0x84, 0x5D, 0x01, 0x00, 0x00],
            ),
        },
    ])
}
