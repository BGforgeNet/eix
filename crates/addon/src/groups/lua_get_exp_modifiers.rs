//! LuaGetExpModifiers: the mana cost factors, the experience multiplier and the numbers
//! shown for experience come from the script's `GetManaBonusOn*` and `GetExpMultiplierOn*`.
//!
//! Site #2 is also ExpLimit #1's (0x523960). This group comes later in the table and takes the site as the original
//! does: its jump goes over the first five bytes of whatever ExpLimit wrote there.
#![expect(
    unsafe_code,
    reason = "reading the game's experience frames, units and statistics"
)]

use eix_core::stats::Experience;
use eix_engine::float::ftol;
use eix_engine::game::player::{PlayerClient, UnitMapNode};
use eix_engine::game::stl::HashTable;
use eix_engine::game::unit::{UnitParams, UnitStats};
use eix_engine::game::world::{CLIENT_WORLD, World};
use eix_engine::hook::{Context, Mode};

use super::lua_common::{context, replace, require_host, with_host};
use super::{BuildError, Labelled, Setup, applied_write};
use crate::log;
use crate::lua::{addr, ptr};

/// AddScaledExperience's site and its first five bytes in game.exe.
const ADD_SCALED_EXPERIENCE: usize = eix_sites::UNIT_STATS_ADD_SCALED_EXPERIENCE;
const ADD_SCALED_EXPERIENCE_ENTRY: [u8; 5] = [0xDD, 0x05, 0x28, 0xE1, 0x73];

/// The experience style of `CWorldClient::ShowNumber` (`CPlayerClient::OnExperience` passes 8).
const SHOW_EXPERIENCE: i32 = 8;

/// `UnitStats::Period20` 0x523CD0: `GetManaBonusOnServer(stats)`.
extern "thiscall" fn period20(stats: *mut UnitStats) -> f32 {
    with_host(0.0, |h| h.get_mana_bonus_on_server(addr(stats)))
}

/// `UnitParams::Period` 0x5271C0: `GetManaBonusOnClient(params)`.
extern "thiscall" fn params_period(params: *mut UnitParams) -> f32 {
    with_host(0.0, |h| h.get_mana_bonus_on_client(addr(params)))
}

/// `UnitStats::AddScaledExperience` 0x523960: `GetExpMultiplierOnServer(stats) * amount` joins the
/// pending experience.
extern "thiscall" fn add_scaled_experience(stats: *mut UnitStats, amount: f32) {
    let multiplier = with_host(0.0, |h| h.get_exp_multiplier_on_server(addr(stats)));
    // SAFETY: the game passes a unit's live statistics, on its thread; read after the script ran.
    let s = unsafe { &mut *stats };
    let mut e = Experience {
        experience: s.experience,
        spent: s.spent_experience,
        pending: s.pending_experience,
    };
    e.add_scaled(multiplier, amount);
    (s.experience, s.pending_experience) = (e.experience, e.pending);
}

/// The client's multiplier for the unit at `unit` times `amount`, as the experience number shows it; without the unit,
/// `amount` as it is (the original passes `unit + 0x330` to the script without checking the unit).
fn client_number(unit: u32, amount: f32) -> i32 {
    if unit == 0 {
        return ftol(f64::from(amount));
    }
    let params =
        unit.wrapping_add(core::mem::offset_of!(eix_engine::game::unit::Unit, params) as u32);
    let multiplier = with_host(0.0, |h| h.get_exp_multiplier_on_client(params));
    ftol(f64::from(multiplier) * f64::from(amount))
}

/// A word of the game's frame at `ebp + offset`.
fn frame_word(ctx: &Context, offset: i32) -> u32 {
    let at = (ctx.ebp as usize).wrapping_add_signed(offset as isize);
    ctx.stack_at(at).unwrap_or_else(|| {
        log::error(
            "lua",
            "an experience frame word is off the stack",
            &format!("{at:#x}"),
        );
        0
    })
}

/// `CPlayerClient::OnExperience` at 0x66A603: the shown number gets the client multiplier.
extern "C" fn on_experience(_: usize, ctx: &mut Context) {
    let id = frame_word(ctx, -4).cast_signed();
    let amount = f32::from_bits(frame_word(ctx, -8));
    // SAFETY: the client's world is constructed and a zone is loaded while experience arrives; on the game's thread.
    let unit = unsafe { World::find_unit(CLIENT_WORLD.as_ptr(), id) };
    ctx.eax = client_number(addr(unit), amount).cast_unsigned();
    ctx.ecx = id.cast_unsigned();
    ctx.resume = eix_sites::C_PLAYER_CLIENT_ON_EXPERIENCE__69;
}

/// `CPlayerClient::OnExperienceAll` at 0x66A4C6: a number over each of the player's units.
extern "C" fn on_experience_all(_: usize, ctx: &mut Context) {
    let amount = f32::from_bits(frame_word(ctx, -0x10));
    // SAFETY: ESI is the live CPlayerClient handling the message, on the game's thread.
    let player = unsafe { (*ptr::<PlayerClient>(ctx.esi)).player };
    // SAFETY: the client's player is live; only the table's address is taken.
    let units = unsafe { &raw const (*player).units };
    let show = |node: *mut UnitMapNode| {
        // SAFETY: `for_each` passes live nodes.
        let n = unsafe { &*node };
        let value = client_number(addr(n.unit), amount);
        // SAFETY: the client's world, on the game's thread.
        unsafe { World::show_number(CLIENT_WORLD.as_ptr(), n.id, value, SHOW_EXPERIENCE) };
    };
    // SAFETY: the player's unit table; showing a number leaves it unchanged.
    unsafe { HashTable::for_each(units, show) };
    ctx.resume = eix_sites::C_PLAYER_CLIENT_ON_EXPERIENCE_ALL__104;
}

/// `CItemInfoPanel::DescribeStats` at 0x5F17B8, after its `push 0x792FB8` (format "%s %d%%"): the percentage
/// argument becomes the server multiplier of the hovered unit's statistics ([EBX+0x10]) times 100.
extern "C" fn describe_stats(_: usize, ctx: &mut Context) {
    // SAFETY: at this site EBX is the hover info, whose +0x10 is the unit's statistics (the game reads it at 0x5F1787).
    let stats = unsafe { ptr::<u32>(ctx.ebx.wrapping_add(0x10)).read() };
    let multiplier = with_host(0.0, |h| h.get_exp_multiplier_on_server(stats));
    let percent = ftol(f64::from(multiplier) * 100.0);
    if !ctx.set_stack_word(2, percent.cast_unsigned()) {
        log::error(
            "lua",
            "the percentage argument is off the stack",
            "0x005F17B8",
        );
    }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    require_host()?;
    let entry = match applied_write(ADD_SCALED_EXPERIENCE) {
        Some(earlier) if earlier.len() >= ADD_SCALED_EXPERIENCE_ENTRY.len() => {
            log::warn(
                "lua",
                "0x00523960 holds an earlier group's patch (ExpLimit #1); its first 5 bytes become this group's jump, as in the original",
                "LuaGetExpModifiers #2",
            );
            earlier[..ADD_SCALED_EXPERIENCE_ENTRY.len()].to_vec()
        }
        Some(_) | None => ADD_SCALED_EXPERIENCE_ENTRY.to_vec(),
    };
    Ok(vec![
        replace(
            "LuaGetExpModifiers #0",
            eix_sites::UNIT_STATS_PERIOD_20,
            &[0xD9, 0x05, 0x24, 0xE1, 0x73, 0x00],
            period20 as *const () as usize,
        )?,
        replace(
            "LuaGetExpModifiers #1",
            eix_sites::UNIT_PARAMS_PERIOD,
            &[0x51, 0x33, 0xC0, 0x8A, 0x41, 0x0E],
            params_period as *const () as usize,
        )?,
        replace(
            "LuaGetExpModifiers #2",
            ADD_SCALED_EXPERIENCE,
            &entry,
            add_scaled_experience as *const () as usize,
        )?,
        context(
            "LuaGetExpModifiers #3",
            eix_sites::C_PLAYER_CLIENT_ON_EXPERIENCE__63,
            &[0x8B, 0x45, 0xF0, 0x8B, 0x4D, 0xFC],
            Mode::Drop,
            on_experience,
        )?,
        context(
            "LuaGetExpModifiers #4",
            eix_sites::C_PLAYER_CLIENT_ON_EXPERIENCE_ALL__46,
            &[0x8B, 0x4D, 0xE4, 0x8B, 0x45, 0xD0],
            Mode::Drop,
            on_experience_all,
        )?,
        context(
            "LuaGetExpModifiers #5",
            eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_STATS__228,
            &[0x68, 0xB8, 0x2F, 0x79, 0x00],
            Mode::Before,
            describe_stats,
        )?,
    ])
}
