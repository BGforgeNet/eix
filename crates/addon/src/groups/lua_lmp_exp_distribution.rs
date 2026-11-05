//! LuaLmpExpDistribution: in an LMP game each player's share of a kill's experience is the
//! script's `DistributeLmpExp(exp, players)`.
//!
//! Fixed: the original drops the site's `fst` / `fstp` pair and so leaves the game's own share on the x87
//! stack at every call. Here the pair runs first (mode "before") and pops it; the handler then overwrites both
//! stores with the script's share, so memory ends as under the original.
#![expect(
    unsafe_code,
    reason = "writing the game's experience share and reading the server world"
)]

use eix_engine::game::world::{SERVER_WORLD, World};
use eix_engine::hook::{Context, Mode};

use super::lua_common::{context, require_host, with_host};
use super::{BuildError, Labelled, Setup};
use crate::log;
use crate::lua::ptr;

/// At 0x591327 (`CWorldServer::AwardExperience+0x167`) EBP is the frame, `[EBP+0xC]` the kill's experience; the share
/// goes to `[EBP+8]` and `[EAX+0xC]`.
extern "C" fn handler(_: usize, ctx: &mut Context) {
    let exp_at = (ctx.ebp as usize).wrapping_add(0xC);
    let Some(exp) = ctx.stack_at(exp_at).map(f32::from_bits) else {
        log::error(
            "lua",
            "the experience argument is off the stack",
            "0x00591327",
        );
        return;
    };
    // SAFETY: the server's world is constructed; on the game's thread.
    let manager = unsafe { World::players(SERVER_WORLD.as_ptr()) };
    // SAFETY: the world's player manager is live.
    let players = unsafe { (*manager).player_count };
    let share = with_host(0.0, |h| h.distribute_lmp_exp(exp, players));
    if !ctx.set_stack_at((ctx.ebp as usize).wrapping_add(8), share.to_bits()) {
        log::error("lua", "the share is off the stack", "0x00591327");
    }
    // SAFETY: EAX is the award record the dropped `fstp DWORD PTR [eax+0xc]` writes, on the game's thread.
    unsafe { ptr::<f32>(ctx.eax.wrapping_add(0xC)).write(share) };
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    require_host()?;
    Ok(vec![context(
        "LuaLmpExpDistribution #0",
        eix_sites::C_WORLD_SERVER_AWARD_EXPERIENCE__167,
        &[0xD9, 0x55, 0x08, 0xD9, 0x58, 0x0C],
        Mode::Before,
        handler,
    )?])
}
