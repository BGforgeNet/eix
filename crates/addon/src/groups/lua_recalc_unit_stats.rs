//! LuaRecalcUnitStats: hit points, mana, the recompute and the raises go through the
//! script's `CalcBaseHealth`, `CalcBaseMana`, `GetModifiedHealth`, `RecalcUnitStats` and `RecalcInternalHeroStats`.
//!
//! The alive bit goes into the stats' flags, and every attribute raise marks the unit changed. The strength raise
//! widens the unit's figure as the game's does. A raise or an experience gain for a unit without server vitals does
//! nothing (the game's own result) instead of crashing.
//!
//! Each site is hooked when the script defines a callback it reaches, the raises and the experience gain through
//! RefreshMaxima' and the recompute too. Without `RecalcUnitStats` the game's own Recompute runs; it computes the
//! maximum hit points, their effect scaling and the maximum mana inline, so two hooks inside it (#13, #14) put the
//! defined formulas there.
#![expect(
    unsafe_code,
    reason = "the replacements read and write the unit's statistics and call game.exe's unit code"
)]

use eix_core::lua_callbacks::Callback::{
    self, CalcBaseHealth, CalcBaseMana, GetExpMultiplierOnServer, GetModifiedHealth,
    GetSkillPriceDiff, RecalcInternalHeroStats, RecalcUnitStats,
};
use eix_core::stats::{Experience, narrowed_scale, widened_scale};
use eix_engine::float::ftol;
use eix_engine::game::unit::{self, MapObject, Unit, UnitStats, UnitVitals};
use eix_engine::hook::{Context, Mode};

use super::lua_common::{
    base_health, base_mana, context, has_text_key, modified_health, refresh_maxima, replace,
    require_host, skill_price_diff, unit_effect_maxima, wanted, with_host,
};
use super::{BuildError, Labelled, Setup};
use crate::log;
use crate::lua::{addr, defined, ptr};

/// `UnitStats::HitPointCurve` 0x523E80: `CalcBaseHealth(stats)`.
extern "thiscall" fn hit_point_curve(stats: *mut UnitStats) -> f32 {
    with_host(0.0, |h| h.calc_base_health(addr(stats)))
}

/// `UnitStats::ManaCurve` 0x523F50: `CalcBaseMana(stats)`.
extern "thiscall" fn mana_curve(stats: *mut UnitStats) -> f32 {
    with_host(0.0, |h| h.calc_base_mana(addr(stats)))
}

/// `ScaleHitPointsByEffects` 0x523E00 (`stdcall(unit, value)` hooked as thiscall): `GetModifiedHealth(ecx, unit,
/// value)`.
extern "thiscall" fn scale_hit_points(ecx: u32, unit: u32, value: f32) -> f32 {
    with_host(0.0, |h| h.get_modified_health(ecx, unit, value))
}

/// `UnitStats::RecomputeHitPoints` 0x5243D0.
extern "thiscall" fn recompute_hit_points(
    stats: *mut UnitStats,
    vitals: *mut UnitVitals,
    unit: *mut Unit,
) {
    // SAFETY: the game passes its unit, the unit's statistics and vitals, on its thread.
    let max = if unsafe { has_text_key(unit) } {
        // SAFETY: as above.
        unsafe { base_health(stats) }
    } else {
        // SAFETY: as above.
        unsafe { (*stats).fixed_max_hit_points }
    };
    // SAFETY: as above.
    unsafe { (*vitals).max_hit_points = max };
    // SAFETY: as above.
    let hp = unsafe { modified_health(stats, unit, max) };
    // SAFETY: as above.
    unsafe { (*vitals).max_hit_points = hp };
    // SAFETY: as above.
    unsafe { UnitStats::scale_slot_maxima(stats, hp) };
    // SAFETY: the maximum is read back, as the original does.
    let max = unsafe { (*vitals).max_hit_points };
    // SAFETY: as above.
    let left = unsafe { UnitStats::reduce(stats, max) };
    // SAFETY: the game's writable statistics and vitals.
    store_hit_points(unsafe { &mut *stats }, unsafe { &mut *vitals }, left);
}

fn store_hit_points(stats: &mut UnitStats, vitals: &mut UnitVitals, left: f32) {
    vitals.hit_points = left;
    if left > 0.0 {
        stats.flags |= 8;
    }
}

/// `UnitStats::GainExperience` 0x5239B0: the scaled experience, then RefreshMaxima'.
extern "thiscall" fn gain_experience(stats: *mut UnitStats, unit: *mut Unit, amount: f32) {
    if unit.is_null() {
        return;
    }
    // SAFETY: the game passes a live unit, on its thread.
    let vitals = unsafe { unit::server_vitals(unit) };
    if vitals.is_null() {
        return;
    }
    // SAFETY: the unit's statistics; the entry may hold LuaGetExpModifiers' or ExpLimit's patch, as in the original.
    unsafe { UnitStats::add_scaled_experience(stats, amount) };
    // SAFETY: the unit's statistics and server vitals.
    unsafe { refresh_maxima(stats, vitals, unit) };
}

/// `UnitStats::Recompute` 0x524560: `RecalcUnitStats(stats, vitals, unit, &effectMaxima)`.
extern "thiscall" fn recompute(stats: *mut UnitStats, vitals: *mut UnitVitals, unit: *mut Unit) {
    // SAFETY: the game passes its unit (or null), on its thread.
    let mut maxima = unsafe { unit_effect_maxima(unit) };
    let m = addr(maxima.as_mut_ptr());
    with_host((), |h| {
        h.recalc_unit_stats(addr(stats), addr(vitals), addr(unit), m)
    });
}

/// A word of the game's frame at `ebp + offset`, logged and `None` off the stack.
fn frame_word(ctx: &Context, offset: isize) -> Option<u32> {
    let at = (ctx.ebp as usize).wrapping_add_signed(offset);
    let word = ctx.stack_at(at);
    if word.is_none() {
        log::error(
            "lua",
            "a Recompute frame word is off the stack",
            &format!("{at:#x}"),
        );
    }
    word
}

/// The game's own `UnitStats::Recompute`, where the script defines no `RecalcUnitStats`, computes a keyed unit's
/// maxima inline. At 0x524A49, where its keyed and unkeyed paths join (EBX the statistics, ESI the vitals, the byte
/// at EBP-0x20 whether the unit has a text key, the x87 stack empty), a keyed unit's maxima come from the script's
/// `CalcBaseHealth` and `CalcBaseMana`, each where it is defined.
extern "C" fn recompute_maxima(_: usize, ctx: &mut Context) {
    let Some(keyed) = frame_word(ctx, -0x20) else {
        return;
    };
    if keyed & 0xFF == 0 {
        return;
    }
    let stats = ptr::<UnitStats>(ctx.ebx);
    let vitals = ptr::<UnitVitals>(ctx.esi);
    if defined().has(CalcBaseHealth) {
        // SAFETY: at this site EBX is the unit's live statistics, on the game's thread.
        let max = unsafe { base_health(stats) };
        // SAFETY: ESI is the unit's vitals, which the game stores its maxima in.
        unsafe { (*vitals).max_hit_points = max };
    }
    if defined().has(CalcBaseMana) {
        // SAFETY: as above.
        let max = unsafe { base_mana(stats) };
        // SAFETY: as above.
        unsafe { (*vitals).max_mana = max };
    }
}

/// Recompute at 0x524AB8, before `fld [ebp-8]; fst [esi+0xc]`: [EBP-8] holds the maximum hit points scaled by the
/// unit's effects, the vitals still the unscaled one, the unit is [EBP+0xC], and the x87 stack is empty. The scaled
/// value becomes the script's `GetModifiedHealth`.
extern "C" fn recompute_scaled(_: usize, ctx: &mut Context) {
    let Some(unit) = frame_word(ctx, 0xC) else {
        return;
    };
    let unit = ptr::<Unit>(unit);
    if unit.is_null() {
        return;
    }
    let stats = ptr::<UnitStats>(ctx.ebx);
    // SAFETY: ESI is the unit's vitals, on the game's thread.
    let max = unsafe { (*ptr::<UnitVitals>(ctx.esi)).max_hit_points };
    // SAFETY: EBX is the unit's live statistics and the frame's unit is live.
    let hp = unsafe { modified_health(stats, unit, max) };
    let at = (ctx.ebp as usize).wrapping_sub(8);
    if !ctx.set_stack_at(at, hp.to_bits()) {
        log::error(
            "lua",
            "the scaled maximum is off the stack",
            &format!("{at:#x}"),
        );
    }
}

/// Which attribute a raise changes, and which scale component it widens (strength) or narrows (dexterity).
#[derive(Clone, Copy)]
enum Raise {
    Strength,
    Dexterity,
    Intelligence,
}

/// RaiseStrength / RaiseDexterity / RaiseIntelligence.
///
/// # Safety
/// The game's arguments: the unit's statistics and the unit (or null), on its thread.
unsafe fn raise(kind: Raise, stats: *mut UnitStats, amount: i32, unit: *mut Unit) -> bool {
    if unit.is_null() {
        return false;
    }
    // SAFETY: a live unit.
    let vitals = unsafe { unit::server_vitals(unit) };
    if vitals.is_null() {
        return false;
    }
    // SAFETY: the unit's statistics.
    let s = unsafe { &mut *stats };
    let base = match kind {
        Raise::Strength => &mut s.strength_base,
        Raise::Dexterity => &mut s.dexterity_base,
        Raise::Intelligence => &mut s.intelligence_base,
    };
    *base = (f64::from(amount) + f64::from(*base)) as f32;
    // SAFETY: the unit's statistics and server vitals.
    unsafe { refresh_maxima(stats, vitals, unit) };
    let m = unit.cast::<MapObject>();
    let mark = |part| {
        // SAFETY: the unit's live map object, on the game's thread.
        unsafe { MapObject::mark_changed(m, part) };
    };
    let recompute = || {
        // SAFETY: the unit's statistics and vitals, on its thread; reaches this group's own Recompute.
        unsafe { UnitStats::recompute(stats, vitals, unit) };
    };
    // SAFETY: the unit's live map object; each closure calls the game on its thread.
    unsafe { finish_raise(kind, m, mark, recompute) };
    true
}

/// # Safety
/// `object` must be a live unit's map object, on its thread. The callbacks run on that same object.
unsafe fn finish_raise(
    kind: Raise,
    object: *mut MapObject,
    mut mark: impl FnMut(i32),
    recompute: impl FnOnce(),
) {
    match kind {
        Raise::Strength | Raise::Dexterity => {
            // SAFETY: the caller vouches for the object. This borrow ends before the callbacks run.
            let o = unsafe { &mut *object };
            if matches!(kind, Raise::Strength) {
                o.base_scale.x = widened_scale(o.base_scale.x);
                o.scale.x = widened_scale(o.scale.x);
            } else {
                o.base_scale.y = narrowed_scale(o.base_scale.y);
                o.scale.y = narrowed_scale(o.scale.y);
            }
        }
        Raise::Intelligence => {}
    }
    mark(1);
    recompute();
    mark(4);
}

/// `UnitStats::RaiseStrength` 0x529090.
extern "thiscall" fn raise_strength(stats: *mut UnitStats, amount: i32, unit: *mut Unit) -> bool {
    // SAFETY: the game's arguments.
    unsafe { raise(Raise::Strength, stats, amount, unit) }
}

/// `UnitStats::RaiseDexterity` 0x529220.
extern "thiscall" fn raise_dexterity(stats: *mut UnitStats, amount: i32, unit: *mut Unit) -> bool {
    // SAFETY: the game's arguments.
    unsafe { raise(Raise::Dexterity, stats, amount, unit) }
}

/// `UnitStats::RaiseIntelligence` 0x529400.
extern "thiscall" fn raise_intelligence(
    stats: *mut UnitStats,
    amount: i32,
    unit: *mut Unit,
) -> bool {
    // SAFETY: the game's arguments.
    unsafe { raise(Raise::Intelligence, stats, amount, unit) }
}

/// RaiseSkill / RaiseAttribute: the byte at `bytes + index` grows by the low byte of
/// `amount`, priced by `GetSkillPriceDiff`.
///
/// # Safety
/// The game's arguments, with `bytes` the offset of the skill or attribute bytes in the statistics.
unsafe fn raise_byte(
    bytes: usize,
    stats: *mut UnitStats,
    index: i32,
    amount: u32,
    unit: *mut Unit,
) -> bool {
    if unit.is_null() {
        return false;
    }
    // SAFETY: a live unit.
    let vitals = unsafe { unit::server_vitals(unit) };
    if vitals.is_null() {
        return false;
    }
    let step = amount.to_le_bytes()[0];
    // GiveSkill is game.exe's only caller of RaiseSkill and RaiseAttribute. It matches each name and passes a
    // constant index: skills 0..7, or attribute 0 for science; an unknown name calls neither function.
    let at = stats
        .cast::<u8>()
        .wrapping_add(bytes)
        .wrapping_offset(index as isize);
    // SAFETY: the statistics' byte the game names.
    let old = unsafe { at.read() };
    let cost = skill_price_diff(i32::from(old), i32::from(old) + i32::from(step));
    // SAFETY: as above; read again after the script ran, as the original's `add BYTE PTR [esi], bl`.
    let now = unsafe { at.read() };
    // SAFETY: as above.
    unsafe { at.write(now.wrapping_add(step)) };
    // SAFETY: the unit's statistics.
    let s = unsafe { &mut *stats };
    let mut e = Experience {
        experience: s.experience,
        spent: s.spent_experience,
        pending: s.pending_experience,
    };
    e.add_cost(cost);
    (s.experience, s.spent_experience, s.pending_experience) = (e.experience, e.spent, e.pending);
    // SAFETY: the unit's statistics and server vitals.
    unsafe { refresh_maxima(stats, vitals, unit) };
    // SAFETY: as above.
    unsafe { UnitStats::recompute(stats, vitals, unit) };
    true
}

/// `UnitStats::RaiseSkill` 0x528AB0 (index, amount, unit on the stack).
extern "thiscall" fn raise_skill(
    stats: *mut UnitStats,
    index: i32,
    amount: u32,
    unit: *mut Unit,
) -> bool {
    let bytes = core::mem::offset_of!(UnitStats, skills);
    // SAFETY: the game's arguments.
    unsafe { raise_byte(bytes, stats, index, amount, unit) }
}

/// `UnitStats::RaiseAttribute` 0x528DA0.
extern "thiscall" fn raise_attribute(
    stats: *mut UnitStats,
    index: i32,
    amount: u32,
    unit: *mut Unit,
) -> bool {
    let bytes = core::mem::offset_of!(UnitStats, attributes);
    // SAFETY: the game's arguments.
    unsafe { raise_byte(bytes, stats, index, amount, unit) }
}

/// `UnitStats::ResetToBase` 0x524CD0: `RecalcInternalHeroStats(stats, newLoad)`.
extern "thiscall" fn reset_to_base(stats: *mut UnitStats, new_load: f32) {
    with_host((), |h| h.recalc_internal_hero_stats(addr(stats), new_load));
}

/// `CUnit::SetFromStats` 0x532A20: pack the statistics, then the packed hit points from the script's
/// formulas.
extern "thiscall" fn set_from_stats(unit: *mut Unit, stats: *mut UnitStats) {
    // SAFETY: the game passes the unit and its statistics, on its thread.
    let params = unsafe { &raw mut (*unit).params };
    // SAFETY: as above.
    unsafe { UnitStats::pack(stats, unit, params) };
    // SAFETY: as above.
    let base = unsafe { base_health(stats) };
    // SAFETY: as above.
    let hp = unsafe { modified_health(stats, unit, base) };
    // The packed fields are 16 bits: the low half of `_ftol2_sse`'s result, as the original's `mov WORD PTR`.
    // SAFETY: as above.
    unsafe { (*unit).map_object.packed_vitals.max_hit_points = ftol(f64::from(hp)) as u16 };
    // SAFETY: as above.
    let left = unsafe { UnitStats::reduce(stats, hp) };
    // SAFETY: as above.
    unsafe { (*unit).map_object.packed_vitals.hit_points = ftol(f64::from(left)) as u16 };
}

/// A replaced function: its label, entry, first bytes, replacement, and the callbacks the replacement reaches.
type Replaced<'a> = (&'a str, eix_sites::Site, &'a [u8], usize, &'a [Callback]);

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    require_host()?;
    // What each replacement reaches: the hit points through `base_health` and `modified_health`, RefreshMaxima' the
    // mana too, a raise also the recompute, and the experience gain the scaled experience's entry.
    const HIT_POINTS: &[Callback] = &[CalcBaseHealth, GetModifiedHealth];
    const GAIN: &[Callback] = &[
        CalcBaseHealth,
        GetModifiedHealth,
        CalcBaseMana,
        GetExpMultiplierOnServer,
    ];
    const RAISE: &[Callback] = &[
        CalcBaseHealth,
        GetModifiedHealth,
        CalcBaseMana,
        RecalcUnitStats,
    ];
    const RAISE_PRICED: &[Callback] = &[
        CalcBaseHealth,
        GetModifiedHealth,
        CalcBaseMana,
        RecalcUnitStats,
        GetSkillPriceDiff,
    ];
    let sites: [Replaced<'_>; 13] = [
        (
            "LuaRecalcUnitStats #0",
            eix_sites::UNIT_STATS_HIT_POINT_CURVE,
            &[0x83, 0xEC, 0x08, 0xD9, 0x05, 0x60, 0x54, 0x7C, 0x00],
            hit_point_curve as *const () as usize,
            &[CalcBaseHealth],
        ),
        (
            "LuaRecalcUnitStats #1",
            eix_sites::UNIT_STATS_MANA_CURVE,
            &[0x83, 0xEC, 0x08, 0xD9, 0x05, 0x74, 0x54, 0x7C, 0x00],
            mana_curve as *const () as usize,
            &[CalcBaseMana],
        ),
        (
            "LuaRecalcUnitStats #2",
            eix_sites::SCALE_HIT_POINTS_BY_EFFECTS,
            &[0x55, 0x8B, 0xEC, 0x51, 0x56],
            scale_hit_points as *const () as usize,
            &[GetModifiedHealth],
        ),
        (
            "LuaRecalcUnitStats #3",
            eix_sites::UNIT_STATS_RECOMPUTE_HIT_POINTS,
            &[0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x08],
            recompute_hit_points as *const () as usize,
            HIT_POINTS,
        ),
        (
            "LuaRecalcUnitStats #4",
            eix_sites::UNIT_STATS_GAIN_EXPERIENCE,
            &[0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x0C],
            gain_experience as *const () as usize,
            GAIN,
        ),
        (
            "LuaRecalcUnitStats #5",
            eix_sites::UNIT_STATS_RECOMPUTE,
            &[0x55, 0x8B, 0xEC, 0x81, 0xEC, 0xD4, 0x00, 0x00, 0x00],
            recompute as *const () as usize,
            &[RecalcUnitStats],
        ),
        (
            "LuaRecalcUnitStats #6",
            eix_sites::UNIT_STATS_RAISE_STRENGTH,
            &[0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x08],
            raise_strength as *const () as usize,
            RAISE,
        ),
        (
            "LuaRecalcUnitStats #7",
            eix_sites::UNIT_STATS_RAISE_DEXTERITY,
            &[0x55, 0x8B, 0xEC, 0x51, 0x53],
            raise_dexterity as *const () as usize,
            RAISE,
        ),
        (
            "LuaRecalcUnitStats #8",
            eix_sites::UNIT_STATS_RAISE_INTELLIGENCE,
            &[0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x14],
            raise_intelligence as *const () as usize,
            RAISE,
        ),
        (
            "LuaRecalcUnitStats #9",
            eix_sites::UNIT_STATS_RAISE_SKILL,
            &[0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x0C],
            raise_skill as *const () as usize,
            RAISE_PRICED,
        ),
        (
            "LuaRecalcUnitStats #10",
            eix_sites::UNIT_STATS_RAISE_ATTRIBUTE,
            &[0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x0C],
            raise_attribute as *const () as usize,
            RAISE_PRICED,
        ),
        (
            "LuaRecalcUnitStats #11",
            eix_sites::UNIT_STATS_RESET_TO_BASE,
            &[0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x14],
            reset_to_base as *const () as usize,
            &[RecalcInternalHeroStats],
        ),
        (
            "LuaRecalcUnitStats #12",
            eix_sites::C_UNIT_SET_FROM_STATS,
            &[0x56, 0x8B, 0xF1, 0x57, 0x8B, 0x7C, 0x24, 0x0C],
            set_from_stats as *const () as usize,
            HIT_POINTS,
        ),
    ];
    let mut hooked = sites
        .into_iter()
        .filter(|&(.., reaches)| wanted(reaches))
        .map(|(label, site, original, to, _)| replace(label, site, original, to))
        .collect::<Result<Vec<_>, _>>()?;
    // Without `RecalcUnitStats` the game's Recompute runs, with the three formulas inline.
    if !defined().has(RecalcUnitStats) {
        if wanted(&[CalcBaseHealth, CalcBaseMana]) {
            hooked.push(context(
                "LuaRecalcUnitStats #13",
                eix_sites::UNIT_STATS_RECOMPUTE__4E9,
                &[0x8B, 0x7D, 0x0C, 0x8B, 0x4E, 0x0C],
                Mode::After,
                recompute_maxima,
            )?);
        }
        if wanted(&[GetModifiedHealth]) {
            hooked.push(context(
                "LuaRecalcUnitStats #14",
                eix_sites::UNIT_STATS_RECOMPUTE__558,
                &[0xD9, 0x45, 0xF8, 0xD9, 0x56, 0x0C],
                Mode::After,
                recompute_scaled,
            )?);
        }
    }
    Ok(hooked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_hit_points_set_the_alive_flag_and_preserve_build_flags() {
        // SAFETY: these structures contain only numeric fields, pointers and byte arrays.
        let mut stats: UnitStats = unsafe { core::mem::zeroed() };
        // SAFETY: UnitVitals contains only numeric fields and arrays.
        let mut vitals: UnitVitals = unsafe { core::mem::zeroed() };
        stats.built_flags = 1;
        stats.flags = 0x20;
        store_hit_points(&mut stats, &mut vitals, 10.0);
        assert_eq!(vitals.hit_points, 10.0);
        assert_eq!(stats.flags, 0x28);
        assert_eq!(stats.built_flags, 1);
        stats.flags = 0x20;
        store_hit_points(&mut stats, &mut vitals, 0.0);
        assert_eq!(stats.flags, 0x20);
        store_hit_points(&mut stats, &mut vitals, -1.0);
        assert_eq!(stats.flags, 0x20);
        stats.flags = 0x28;
        store_hit_points(&mut stats, &mut vitals, 0.0);
        assert_eq!(stats.flags, 0x28);
    }

    #[test]
    fn intelligence_marks_changes_around_recomputation_without_scaling() {
        // SAFETY: MapObject contains numeric fields, pointers and byte arrays; callbacks do not call the game.
        let mut object: MapObject = unsafe { core::mem::zeroed() };
        object.base_scale.x = 1.0;
        object.base_scale.y = 2.0;
        object.scale.x = 3.0;
        object.scale.y = 4.0;
        let calls = std::cell::RefCell::new(Vec::new());
        // SAFETY: the local object is live and uniquely used here; the callbacks only record the call sequence.
        unsafe {
            finish_raise(
                Raise::Intelligence,
                &raw mut object,
                |part| calls.borrow_mut().push(part),
                || calls.borrow_mut().push(0),
            )
        };
        assert_eq!(*calls.borrow(), [1, 0, 4]);
        assert_eq!(
            (
                object.base_scale.x,
                object.base_scale.y,
                object.scale.x,
                object.scale.y
            ),
            (1.0, 2.0, 3.0, 4.0)
        );
    }
}
