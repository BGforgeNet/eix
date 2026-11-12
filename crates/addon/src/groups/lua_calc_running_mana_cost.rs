//! LuaCalcRunningManaCost: the mana a running hero spends is the script's
//! `CalcRunningManaCost`, which lowers the mana itself; the game's subtraction is dropped.
#![expect(
    unsafe_code,
    reason = "reading the running unit's controller and vitals"
)]

use eix_core::encumbrance::rescaled;
use eix_core::lua_callbacks::Callback::CalcRunningManaCost;
use eix_engine::game::unit::{UnitServer, UnitVitals};
use eix_engine::hook::{Context, Mode};
use eix_sites::Site;

use super::lua_common::{context, require_host, wanted, with_host};
use super::{BuildError, Labelled, Setup};
use crate::lua::{addr, ptr};

/// Where `CUnitServer::TickMovement` stops running (mana below zero) and where it goes on.
const STOP_RUNNING: Site = eix_sites::UPDATE_MOTION__AA3;
const KEEP_RUNNING: Site = eix_sites::UPDATE_MOTION__AF8;

/// At 0x548315 ESI is the unit's controller and EDI its vitals.
extern "C" fn handler(_: usize, ctx: &mut Context) {
    let server = ptr::<UnitServer>(ctx.esi);
    let stats = ctx
        .esi
        .wrapping_add(core::mem::offset_of!(UnitServer, stats) as u32);
    // SAFETY: at this site ESI is a live CUnitServer, on the game's thread.
    let unit = unsafe { (*server).unit };
    let vitals = ptr::<UnitVitals>(ctx.edi);
    // SAFETY: EDI is the controller's vitals.
    let before = unsafe { (*vitals).mana };
    with_host((), |h| h.calc_running_mana_cost(stats, addr(unit), ctx.edi));
    // `[Encumbrance]` scales the mana the script took, as it does the game's drain. The statistics are read after the
    // script, which may change them.
    // SAFETY: as above.
    let s = unsafe { &(*server).stats };
    let m = crate::encumbrance_rules::drain_multiplier(s.load, s.capacity);
    if m != 1.0 {
        // SAFETY: as above.
        let after = unsafe { (*vitals).mana };
        // SAFETY: as above; the game's thread, which owns the vitals.
        unsafe { (*vitals).mana = rescaled(before, after, m) };
    }
    // SAFETY: EDI is the controller's vitals.
    let mana = unsafe { (*vitals).mana };
    // `0 > mana` stops; a NaN mana keeps running, as the original's compare.
    ctx.resume = if 0.0 > mana {
        STOP_RUNNING
    } else {
        KEEP_RUNNING
    };
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    require_host()?;
    // Without the callback the game's drain runs, and `[Encumbrance]` scales it at its own site.
    if !wanted(&[CalcRunningManaCost]) {
        return Ok(Vec::new());
    }
    Ok(vec![context(
        "CalcRunningManaCost #0",
        eix_sites::UPDATE_MOTION__A85,
        &[0xD9, 0x47, 0x14, 0xD9, 0x47, 0x18],
        Mode::Drop,
        handler,
    )?])
}
