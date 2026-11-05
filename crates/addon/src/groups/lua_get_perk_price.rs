//! LuaGetPerkPrice: skill and perk prices come from the script's `GetSkillPriceDiff` and
//! `GetPerkPrice`.

use eix_engine::game::unit::UnitStats;

use super::lua_common::{replace, require_host, with_host};
use super::{BuildError, Labelled, Setup};
use crate::lua::addr;

/// `SchoolPrice` 0x527FF0 (fastcall: ECX from, EDX to): `GetSkillPriceDiff(from, to)`.
extern "fastcall" fn school_price(from: i32, to: i32) -> i32 {
    with_host(0, |h| h.get_skill_price_diff(from, to))
}

/// `UnitStats::SkillPrice` 0x528050 (thiscall, the perk record on the stack): `GetPerkPrice(stats, record)`.
extern "thiscall" fn skill_price(stats: *mut UnitStats, record: u32) -> i32 {
    with_host(0, |h| h.get_perk_price(addr(stats), record))
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    require_host()?;
    Ok(vec![
        replace(
            "LuaGetPerkPrice #0",
            eix_sites::SCHOOL_PRICE,
            &[0x83, 0xEC, 0x08, 0xDD, 0x05, 0x40, 0xE1, 0x73, 0x00],
            school_price as *const () as usize,
        )?,
        replace(
            "LuaGetPerkPrice #1",
            eix_sites::UNIT_STATS_SKILL_PRICE,
            &[0x55, 0x8B, 0xEC, 0x83, 0xE4, 0xF8],
            skill_price as *const () as usize,
        )?,
    ])
}
