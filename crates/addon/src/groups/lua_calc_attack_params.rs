//! LuaCalcAttackParams: the attack parameters of server and client go through the script's
//! `CalcAttackParamsOnServer` / `OnClient`; a hit on a unit is ignored in god mode, and from another player's unit
//! as the game decides.
//!
//! Kept from the original: the difficulty scaling skips text-key units (the game passes a local flag). A hit from
//! another player's unit is still ignored unless `enablekillsinmulti` is on (the original drops that test, so
//! players' units could always hurt each other).
#![expect(
    unsafe_code,
    reason = "reading the attacking unit, its controller and its player"
)]

use eix_engine::game::combat::AttackParams;
use eix_engine::game::player::Player;
use eix_engine::game::unit::{UnitController, UnitServer};
use eix_engine::hook::{Context, Mode};

use super::lua_common::{context, has_text_key, require_host, unit_effect_maxima, with_host};
use super::{BuildError, Labelled, Setup};
use crate::lua::{addr, ptr};

/// `CUnitServer::GetAttackParams` at 0x555BA8 (EBP the controller, ESI the parameters): resumes at its epilogue.
extern "C" fn server(_: usize, ctx: &mut Context) {
    let server = ptr::<UnitServer>(ctx.ebp);
    let params = ptr::<AttackParams>(ctx.esi);
    // SAFETY: at this site EBP is a live CUnitServer, on the game's thread.
    let s = unsafe { &*server };
    // The dropped instruction's work: the attack spread from the statistics.
    // SAFETY: ESI is the parameters being filled.
    unsafe { (*params).attack_spread = s.stats.combat[2] };
    let unit = s.unit;
    // SAFETY: the controller's unit is live.
    let keyed = unsafe { has_text_key(unit) };
    // SAFETY: the parameters, on the game's thread.
    unsafe { AttackParams::scale_for_difficulty(params, keyed) };
    // SAFETY: as above.
    let mut maxima = unsafe { unit_effect_maxima(unit) };
    let stats = addr(&raw const s.stats);
    let m = addr(maxima.as_mut_ptr());
    with_host((), |h| {
        h.calc_attack_params_on_server(stats, addr(unit), ctx.esi, m)
    });
    ctx.resume = eix_sites::C_UNIT_SERVER_GET_ATTACK_PARAMS__1BF;
}

/// `CUnitClientSpecific::GetAttackParams` at 0x538B74 (EDI the controller, ESI the parameters).
extern "C" fn client(_: usize, ctx: &mut Context) {
    let params = ptr::<AttackParams>(ctx.esi);
    // SAFETY: at this site EDI is a live CUnitClientSpecific, on the game's thread.
    let unit = unsafe { (*ptr::<UnitController>(ctx.edi)).unit };
    // SAFETY: the controller's unit is live.
    let spread = unsafe { (*unit).params.combat[2] };
    // SAFETY: ESI is the parameters being filled.
    unsafe { (*params).attack_spread = f32::from(spread) };
    // SAFETY: as above.
    let keyed = unsafe { has_text_key(unit) };
    // SAFETY: the parameters, on the game's thread.
    unsafe { AttackParams::scale_for_difficulty(params, keyed) };
    // SAFETY: as above.
    let mut maxima = unsafe { unit_effect_maxima(unit) };
    // SAFETY: the unit is live; only its address is taken.
    let unit_params = addr(unsafe { &raw const (*unit).params });
    let m = addr(maxima.as_mut_ptr());
    with_host((), |h| {
        h.calc_attack_params_on_client(unit_params, addr(unit), ctx.esi, m)
    });
    ctx.resume = eix_sites::C_UNIT_CLIENT_SPECIFIC_GET_ATTACK_PARAMS__23C;
}

/// `CUnitServer::TakeHit` at 0x554139 (ESI the controller): the hit is ignored when the unit's player is in god mode;
/// otherwise a unit with a player goes on to the game's test of a hit from another player's unit (EAX that player),
/// which `enablekillsinmulti` lifts.
extern "C" fn take_hit(_: usize, ctx: &mut Context) {
    // SAFETY: at this site ESI is a live CUnitServer, on the game's thread.
    let unit = unsafe { (*ptr::<UnitServer>(ctx.esi)).unit };
    let player = if unit.is_null() {
        core::ptr::null_mut()
    } else {
        // SAFETY: a live unit.
        unsafe { (*unit).player }.cast::<Player>()
    };
    ctx.resume = if player.is_null() {
        eix_sites::C_UNIT_SERVER_TAKE_HIT__22D
    // SAFETY: a unit's non-null player is live.
    } else if unsafe { (*player).god_mode } != 0 {
        eix_sites::C_UNIT_SERVER_TAKE_HIT__43E
    } else {
        ctx.eax = addr(player);
        eix_sites::C_UNIT_SERVER_TAKE_HIT__214
    };
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    require_host()?;
    Ok(vec![
        context(
            "CalcAttackParams #0",
            eix_sites::C_UNIT_SERVER_GET_ATTACK_PARAMS__1A8,
            &[0x8B, 0x8D, 0x58, 0x02, 0x00, 0x00],
            Mode::Drop,
            server,
        )?,
        context(
            "CalcAttackParams #1",
            eix_sites::C_UNIT_CLIENT_SPECIFIC_GET_ATTACK_PARAMS__214,
            &[0x8B, 0x47, 0x1C, 0x8B, 0x54, 0x24, 0x0C],
            Mode::Drop,
            client,
        )?,
        context(
            "CalcAttackParams #2",
            eix_sites::C_UNIT_SERVER_TAKE_HIT__1F9,
            &[0x8B, 0x46, 0x1C, 0x8B, 0x80, 0x4C, 0x02, 0x00, 0x00],
            Mode::Drop,
            take_hit,
        )?,
    ])
}
