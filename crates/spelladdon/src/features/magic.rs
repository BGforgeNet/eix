//! Magic features. The original's added code is replaced by context hooks whose handlers compute what that code
//! computes, reading the parameters where the original keeps them.
#![expect(
    unsafe_code,
    reason = "the hooks read and write the game's spell effects and call game functions"
)]

use core::ffi::CStr;
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicUsize, Ordering};

use eix_engine::game::database::RUNES;
use eix_engine::game::object::{CLASS_UNIT, EngineObject};
use eix_engine::game::player::{PlayerIterator, PlayerServer};
use eix_engine::game::world::{SERVER_WORLD, World};
use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, FpuContext, Mode};
use eix_engine::patch;
use eix_engine::x87::{C0, Ext};
use eix_sites::Site;

use super::common::{self, Bytes, PushTail, read, write};
use super::{BuildError, Ctx};

/// `CWorldServer::CastSpell`+0x50: the three `je` that refuse Possession (0x15), Link (0x1B) and Charm (0x24), with
/// the compares between them.
const CAST_REFUSALS: (Site, &[u8]) = (
    eix_sites::C_WORLD_SERVER_CAST_SPELL__50,
    &[
        0x0F, 0x84, 0x1D, 0x04, 0x00, 0x00, 0x83, 0xF8, 0x1B, 0x0F, 0x84, 0x14, 0x04, 0x00, 0x00,
        0x83, 0xF8, 0x24, 0x0F, 0x84, 0x0B, 0x04, 0x00, 0x00,
    ],
);
/// The bytes after the refusals: `mov ecx, [edi+0x3C]; mov ebx, [edi+0x38]`. With `damageboost` = 5 the block moves
/// them 6 bytes up, into the freed bytes, and puts `and ebx, 0xFFFFFF` in their place.
const CAST_SPELL_LOADS: [u8; 6] = [0x8B, 0x4F, 0x3C, 0x8B, 0x5F, 0x38];
const AND_EBX_MASK: [u8; 6] = [0x81, 0xE3, 0xFF, 0xFF, 0xFF, 0x00];

/// `possessioncharmlink` = 1: `CastSpell` no longer refuses the three spells.
pub(crate) fn possession_charm_link(
    ctx: &Ctx<'_>,
    value: i8,
) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let (site, was) = CAST_REFUSALS;
    if ctx.values.flag("gameplay", "damageboost")? != 5 {
        return Ok(vec![SiteWrite::nop(site, was)]);
    }
    let was = [was, &CAST_SPELL_LOADS].concat();
    let now = [&[0x90; 18][..], &CAST_SPELL_LOADS, &AND_EBX_MASK].concat();
    Ok(vec![SiteWrite::bytes(site, &was, &now)?])
}

/// `test dword ptr [eax+0x3C], 0x20000000` at `AIAttention::CanReach`+0x93: the spell object's "flying" flag, after
/// which `je` refuses the target; EAX is the spell object.
const CAN_REACH_FLYING: Site = eix_sites::AI_ATTENTION_CAN_REACH__93;
const CAN_REACH_FLYING_TEST: [u8; 7] = [0xF7, 0x40, 0x3C, 0x00, 0x00, 0x00, 0x20];
/// `CanReach`+0x104: the path that goes on with the target.
const CAN_REACH_GO_ON: Site = eix_sites::AI_ATTENTION_CAN_REACH__104;
/// Spell type 4, Invoke Lightning.
const INVOKE_LIGHTNING: u32 = 4;
/// `IsSpellTarget`'s tail with value 2: `jne +0x28; cmp dword ptr [ebp+8], 4; je +0x28; xor eax, eax; ret;
/// mov eax, 1; ret`, and its first test's `jne` retargeted to the `mov eax, 1`.
const SPELL_TARGET_TYPE_4: [Bytes; 2] = [
    (eix_sites::IS_SPELL_TARGET__9, &[0x18], &[0x1E]),
    (
        eix_sites::IS_SPELL_TARGET__1E,
        &[
            0x03, 0x33, 0xC0, 0xC3, 0xB8, 0x01, 0x00, 0x00, 0x00, 0xC3, 0x90, 0x90, 0x90, 0x90,
            0x90, 0x90,
        ],
        &[
            0x09, 0x83, 0x7D, 0x08, 0x04, 0x74, 0x03, 0x33, 0xC0, 0xC3, 0xB8, 0x01, 0x00, 0x00,
            0x00, 0xC3,
        ],
    ),
];

/// The handler of `splashdamageonair` = 2 at `CanReach`+0x93: ECX gets the spell object's type (the high word of
/// `[eax+0x18]`); type 4 skips the flying test.
extern "C" fn can_reach_flying(_: usize, ctx: &mut Context) {
    // SAFETY: at the site EAX is the live spell object `CUnit::GetSpellObject` returned.
    let kind = unsafe { read::<u32>(ctx.eax + 0x18) } >> 16;
    ctx.ecx = kind;
    if kind == INVOKE_LIGHTNING {
        ctx.resume = CAN_REACH_GO_ON;
    }
}

/// `splashdamageonair` 1: `IsSpellTarget` no longer refuses flying units and `CSpellEffect::HitUnitsInRadius` does
/// not skip them. 2: only Invoke Lightning reaches them, and the AI aims it at them.
pub(crate) fn splash_damage_on_air(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    match value {
        1 => Ok(common::nops(&[
            (
                eix_sites::IS_SPELL_TARGET__1D,
                &[0x75, 0x03, 0x33, 0xC0, 0xC3],
            ),
            (
                eix_sites::C_SPELL_EFFECT_HIT_UNITS_IN_RADIUS__A5,
                &[0x74, 0x30],
            ),
        ])),
        2 => {
            let mut writes = vec![hook::context_hook(
                CAN_REACH_FLYING,
                &CAN_REACH_FLYING_TEST,
                Mode::After,
                can_reach_flying,
                0,
            )?];
            writes.extend(common::fixed(&SPELL_TARGET_TYPE_4)?);
            Ok(writes)
        }
        _ => Ok(Vec::new()),
    }
}

/// `offensivespells` = 1: two entries of the hostility table inside `CWorldServer::CreateMissile`, Stench and
/// Shrink, become 1.
pub(crate) fn offensive_spells(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    common::fixed(&[
        (
            eix_sites::C_WORLD_SERVER_CREATE_MISSILE__1C0,
            &[0x00],
            &[0x01],
        ),
        (
            eix_sites::C_WORLD_SERVER_CREATE_MISSILE__1D3,
            &[0x00],
            &[0x01],
        ),
    ])
}

/// `shrinkenlargerebalance` 1 or 2: Shrink and Enlarge stop changing the size (a table byte in
/// `CMapObject::AddEffectImpl`, `RecomputeScaleHook`'s `jne`, their test in `CMapObjectServer::Update`, a byte of
/// `IsOffensive`'s table); 1 also points `RecomputeScaleHook`'s multiplier at 0x73B850.
pub(crate) fn shrink_enlarge(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if !matches!(value, 1 | 2) {
        return Ok(Vec::new());
    }
    let mut writes = common::fixed(&[
        (
            eix_sites::C_MAP_OBJECT_ADD_EFFECT_IMPL__E0,
            &[0x00, 0x00],
            &[0x01, 0x01],
        ),
        // `jne rel32` becomes `nop; jmp rel32` to the same target.
        (
            eix_sites::RECOMPUTE_SCALE_HOOK__13A,
            &[0x0F, 0x85],
            &[0x90, 0xE9],
        ),
    ])?;
    if value == 1 {
        // `fmul [0x73B868]` becomes `fmul [0x73B850]`: the whole instruction, so another build gets its address.
        writes.push(SiteWrite::bytes(
            eix_sites::RECOMPUTE_SCALE_HOOK__220.back(2),
            &[0xD8, 0x0D, 0x68, 0xB8, 0x73, 0x00],
            &[0xD8, 0x0D, 0x50, 0xB8, 0x73, 0x00],
        )?);
    }
    writes.push(SiteWrite::nop(
        eix_sites::C_MAP_OBJECT_SERVER_UPDATE__84,
        &[0x83, 0xFB, 0x25, 0x74, 0x0C, 0x83, 0xFB, 0x26, 0x74, 0x07],
    ));
    writes.push(SiteWrite::bytes(
        eix_sites::C_SPELL_IS_OFFENSIVE__4E,
        &[0x00],
        &[0x01],
    )?);
    Ok(writes)
}

/// `UnitVitals::Regenerate`+0x17F rewritten in place: the Regeneration effect's value times `regenerationmul`
/// (kept in the padding at 0x523918) is added to the regeneration.
const REGENERATION: [Bytes; 2] = [
    (eix_sites::UNIT_VITALS_REGENERATE__17F, &[0x1E], &[0x09]),
    (
        eix_sites::UNIT_VITALS_REGENERATE__187,
        &[
            0xD8, 0x15, 0x58, 0xB8, 0x73, 0x00, 0xDF, 0xE0, 0xF6, 0xC4, 0x41, 0x75, 0x08, 0xD8,
            0x4D, 0x10, 0xD9, 0x5D, 0x10, 0xEB, 0x02, 0xDD, 0xD8, 0x8B, 0x46, 0x0C, 0x8B, 0x7D,
            0x08, 0x89, 0x45, 0x0C, 0x51, 0xD9, 0x45, 0x0C, 0xD8, 0x4D, 0x10,
        ],
        &[
            0xEB, 0x06, 0xD9, 0x05, 0x58, 0xB8, 0x73, 0x00, 0x8B, 0x46, 0x0C, 0x8B, 0x7D, 0x08,
            0x89, 0x45, 0x0C, 0x51, 0xD9, 0x45, 0x0C, 0xD8, 0x4D, 0x10, 0xDE, 0xC1, 0xD8, 0x0D,
            0x18, 0x39, 0x52, 0x00, 0xEB, 0x05, 0x90, 0x90, 0x90, 0x90, 0x90,
        ],
    ),
];

/// `regenerationremaster` = 1.
pub(crate) fn regeneration(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let mut writes = common::fixed(&REGENERATION)?;
    writes.push(common::float_over(
        ctx,
        "regenerationmul",
        eix_sites::UNIT_VITALS_REGENERATE__1D8,
        [0x90; 4],
    )?);
    Ok(writes)
}

/// The stats refresh's Strengthen (0x1D) and Weaken (0x1E) terms rewritten in place: their difference
/// (`[ebp-0x4C] - [ebx-0x50]`) times `strengthweakmul` (padding at 0x524F98) scales `[ebx+0x10]`.
const STRENGTH_WEAK: Bytes = (
    eix_sites::UNIT_STATS_RECOMPUTE__4F6,
    &[
        0x6A, 0x1D, 0x8B, 0xCF, 0xE8, 0x31, 0xAC, 0xFE, 0xFF, 0xD8, 0x0D, 0x30, 0xE1, 0x73, 0x00,
        0x6A, 0x1E, 0x8B, 0xCF, 0xD8, 0x05, 0x4C, 0xB8, 0x73, 0x00, 0xD9, 0x5D, 0xF4, 0xE8, 0x19,
        0xAC, 0xFE, 0xFF, 0xD8, 0x0D, 0x30, 0xE1, 0x73, 0x00, 0xD8, 0x05, 0x4C, 0xB8, 0x73, 0x00,
        0xD8, 0x7D, 0xF4,
    ],
    &[
        0xD9, 0x85, 0xB4, 0xFF, 0xFF, 0xFF, 0xD8, 0x63, 0xB0, 0xD8, 0x0D, 0x98, 0x4F, 0x52, 0x00,
        0xD8, 0x43, 0x10, 0xD9, 0xC0, 0xD8, 0x73, 0x10, 0xD9, 0xC9, 0xD9, 0x5B, 0x10, 0x90, 0x90,
        0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
        0x90, 0x90, 0x90,
    ],
);

/// `strengthweakrebalance` = 1.
pub(crate) fn strength_weak(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![
        SiteWrite::bytes(STRENGTH_WEAK.0, STRENGTH_WEAK.1, STRENGTH_WEAK.2)?,
        common::float_over(
            ctx,
            "strengthweakmul",
            eix_sites::UNIT_STATS_RESET_TO_BASE__2C8,
            [0x90; 4],
        )?,
    ])
}

/// `tornadochanges` >= 1: the tornado's damage pushed in `CEffectTornado::Tick` is `tordmg`; >= 2 also hurts
/// every unit (the `je` that skips some goes).
pub(crate) fn tornado(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    let mut writes = vec![common::float(
        ctx,
        "tordmg",
        eix_sites::C_EFFECT_TORNADO_TICK__16B,
        5.0,
    )?];
    if value >= 2 {
        writes.push(SiteWrite::nop(
            eix_sites::C_EFFECT_TORNADO_TICK__180,
            &[0x74, 0x18],
        ));
    }
    Ok(writes)
}

/// `filtersarealwaysusing` 1 or 2: `CSpellEffect::HitUnit` applies the target filter to every spell.
pub(crate) fn filters_always(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if !matches!(value, 1 | 2) {
        return Ok(Vec::new());
    }
    Ok(vec![SiteWrite::nop(
        eix_sites::C_SPELL_EFFECT_HIT_UNIT__B,
        &[0x85, 0xC0, 0x74, 0x0E],
    )])
}

/// `add eax, 0x1E` at `CSpellEffect::Start`+0x1DA9: the teleport's delay; its immediate.
const TELEPORT_DELAY: Site = eix_sites::C_SPELL_EFFECT_START__1DA9;
/// `mov dword ptr [ebx+0x30], 2` after it (0x68161A, 7 bytes): value 1 turns its first two bytes into NOPs and
/// jumps from the third to the added code, which comes back at 0x681621 (`mov [ebx+0xA0], eax`).
const TELEPORT_STATE_NOPS: (Site, &[u8]) = (eix_sites::C_SPELL_EFFECT_START__1DAA, &[0xC7, 0x43]);
const TELEPORT_STATE_REST: Site = eix_sites::C_SPELL_EFFECT_START__1DAC;
const TELEPORT_STATE_REST_BYTES: [u8; 5] = [0x30, 0x02, 0x00, 0x00, 0x00];

/// The handler of `teleportboost` = 1: the effect's state becomes 2 and EAX, the time the teleport takes, becomes
/// `round(900 / (strength / 2 + 22)) + 2`, from the effect's strength `[ebx+0x58]`; the `add eax, 5` before it is
/// overwritten.
extern "C" fn teleport_time(_: usize, ctx: &mut Context) {
    // SAFETY: EBX is the spell effect `CSpellEffect::Start` works on.
    unsafe { write::<u32>(ctx.ebx + 0x30, 2) };
    // SAFETY: as above.
    let strength = unsafe { read::<f32>(ctx.ebx + 0x58) };
    // The constants are game.exe's at 0x73CDB8 (2.0), 0x73F170 (22.0) and 0x73FA30 (900.0).
    let ticks = Ext::new(f64::from(strength))
        .fdiv(2.0)
        .fadd(22.0)
        .fdivr(900.0)
        .fistp();
    ctx.eax = ticks.wrapping_add(2) as u32;
}

/// `teleportboost` 1: the delay is 5, then added code makes it depend on the effect's strength; 2 or 3: the delay of 30
/// becomes 8 or 1.
pub(crate) fn teleport_boost(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    let delay = match value {
        1 => {
            return Ok(vec![
                SiteWrite::bytes(TELEPORT_DELAY, &[0x1E], &[0x05])?,
                SiteWrite::nop(TELEPORT_STATE_NOPS.0, TELEPORT_STATE_NOPS.1),
                hook::context_hook(
                    TELEPORT_STATE_REST,
                    &TELEPORT_STATE_REST_BYTES,
                    Mode::Drop,
                    teleport_time,
                    0,
                )?,
            ]);
        }
        2 => 8,
        3 => 1,
        _ => return Ok(Vec::new()),
    };
    Ok(vec![SiteWrite::bytes(TELEPORT_DELAY, &[0x1E], &[delay])?])
}

/// `mov ecx, [ebp-0x14]; mov eax, [esi+0xC]` at `CSpellEffect::UpdateLasting`+0xDDF, after Possession registers its
/// caster with the script group; the original registers the target too.
const POSSESSION_REGISTER: Site = eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__DDF;
const POSSESSION_REGISTER_BYTES: [u8; 6] = [0x8B, 0x4D, 0xEC, 0x8B, 0x46, 0x0C];
/// `ScriptGroup::Insert(unit)` and `ScriptGroup::InsertAll(list)`, both `ret 4`.
const SCRIPT_GROUP_INSERT: Site = eix_sites::SCRIPT_GROUP_INSERT;
const SCRIPT_GROUP_INSERT_ALL: Site = eix_sites::SCRIPT_GROUP_INSERT_ALL;
type ScriptGroupInsert = unsafe extern "thiscall" fn(group: u32, item: u32);
/// Possession's other fixes: the unit read is the effect's caster `[ebx+0x20]`, and the two lists it adds swap
/// (0xA90 <-> 0xAAC).
const POSSESSION_TARGET: [Bytes; 3] = [
    (
        eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__DA8,
        &[0x1C],
        &[0x20],
    ),
    (
        eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__DC3,
        &[0x90],
        &[0xAC],
    ),
    (
        eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__DD5,
        &[0xAC],
        &[0x90],
    ),
];

/// The handler of `possessionpatch`: as the game did for the target before the patch, the target `[ebx+0x1C]`, then
/// its lists +0xAAC and +0xA90 go into the script group (`[[ebp-0x14]+0x20] + 0x50F8`, also stored over the
/// function's argument `[ebp+8]`); ESI and EDI are the target and its `+0x240` afterwards, as the original leaves them.
extern "C" fn possession_register(_: usize, ctx: &mut Context) {
    // SAFETY: the frame of `UpdateLasting` at the site: `[ebp-0x14]` is the world.
    let world = unsafe { read::<u32>(ctx.ebp - 0x14) };
    // SAFETY: EBX is the effect; its target is a live unit.
    let target = unsafe { read::<u32>(ctx.ebx + 0x1C) };
    // SAFETY: the world's `+0x20` object, as above.
    let group = unsafe { read::<u32>(world + 0x20) }.wrapping_add(0x50F8);
    // SAFETY: the target, as above.
    let unit_part = unsafe { read::<u32>(target + 0x240) };
    // SAFETY: the function's first argument slot, which the original reuses as a local.
    unsafe { write::<u32>(ctx.ebp + 8, group) };
    // SAFETY: both are `void __thiscall (u32)` with `ret 4`.
    let insert: ScriptGroupInsert = unsafe { hook::as_fn(SCRIPT_GROUP_INSERT) };
    // SAFETY: as above.
    let insert_all: ScriptGroupInsert = unsafe { hook::as_fn(SCRIPT_GROUP_INSERT_ALL) };
    // SAFETY: the game's own calls with the game's objects, on its thread.
    unsafe { insert(group, target) };
    // SAFETY: as above.
    unsafe { insert_all(group, unit_part + 0xAAC) };
    // SAFETY: as above.
    unsafe { insert_all(group, unit_part + 0xA90) };
    ctx.esi = target;
    ctx.edi = unit_part;
    // SAFETY: as above.
    ctx.ecx = unsafe { read::<u32>(ctx.ebp - 0x14) };
    // SAFETY: as above.
    ctx.eax = unsafe { read::<u32>(target + 0x0C) };
}

/// `possessionpatch` = 1: Possession no longer takes the caster's sight.
pub(crate) fn possession_patch(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let mut writes = common::fixed(&POSSESSION_TARGET)?;
    // The original's layout: `nop; jmp <added code>`.
    writes.extend(common::jump_at(
        POSSESSION_REGISTER,
        &POSSESSION_REGISTER_BYTES,
        1,
        possession_register,
        0,
    )?);
    Ok(writes)
}

/// `mov [edx+0x14], ecx; fld dword ptr [esi+0x58]` at `CSpellEffect::GetDamage`+0x50: a lightning spell's damage into
/// the lightning slot; and `mov [edx+0xC], ecx; fld ...` at +0x63, a fire spell's into the fire slot. EDI is the
/// spell type, EDX the damage record, whose damage types are dwords from +0.
const DAMAGE_LIGHTNING: Site = eix_sites::C_SPELL_EFFECT_GET_DAMAGE__50;
const DAMAGE_LIGHTNING_BYTES: [u8; 6] = [0x89, 0x4A, 0x14, 0xD9, 0x46, 0x58];
const DAMAGE_FIRE: Site = eix_sites::C_SPELL_EFFECT_GET_DAMAGE__63;
const DAMAGE_FIRE_BYTES: [u8; 6] = [0x89, 0x4A, 0x0C, 0xD9, 0x46, 0x58];
/// After each store: `fld dword ptr [esi+0x58]; fstp dword ptr [edx+0x24]`, which the original's return runs.
const DAMAGE_LIGHTNING_TAIL: Site = eix_sites::C_SPELL_EFFECT_GET_DAMAGE__53;
const DAMAGE_FIRE_TAIL: Site = eix_sites::C_SPELL_EFFECT_GET_DAMAGE__66;
const DAMAGE_TAIL_BYTES: [u8; 6] = [0xD9, 0x46, 0x58, 0xD9, 0x5A, 0x24];
/// Spell types 0x29 (CurseMagic, the INI's `astralbeam`) and 0x28 (RickMagic, its `destroyray`), which get their
/// own damage types; the other spells of each site take `lightningbeam` and `firearrow`.
const ASTRAL_BEAM: u32 = 0x29;
const DESTROY_RAY: u32 = 0x28;

/// The four damage-type displacements: `[lightningbeam, astralbeam, firearrow, destroyray]`, each the low byte of
/// the value times 4, as the original's `mov [edx+disp8], ecx` holds them.
static DAMAGE_SLOTS: AtomicU32 = AtomicU32::new(0);
/// The trampolines that run each site's tail and go on after it.
static DAMAGE_TAILS: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];

/// The damage type's displacement for a key: its low byte times 4, as a signed 8-bit displacement.
fn damage_slot(ctx: &Ctx<'_>, key: &str) -> Result<u8, BuildError> {
    let value = ctx.values.byte("gameplay", key)?;
    let slot = value.wrapping_mul(4);
    // DamageInfo has seven weights, weapon/roll/amount/multiplier, location, hit kind, backstab and called shot.
    if slot > 0x3C - 4 {
        return Err(BuildError(format!(
            "damage type {value} leaves the damage record; using the game's damage type"
        )));
    }
    Ok(slot)
}

/// The handler of `lastspellsdamagetype` at either site (`arg` 0 lightning, 1 fire): the damage goes into the slot of
/// the type the INI gives the spell.
extern "C" fn damage_type(arg: usize, ctx: &mut Context) {
    let slots = DAMAGE_SLOTS.load(Ordering::Relaxed).to_le_bytes();
    let own = if arg == 0 { ASTRAL_BEAM } else { DESTROY_RAY };
    let slot = slots[2 * arg + usize::from(ctx.edi == own)] as i8;
    // SAFETY: EDX is the damage record `GetDamage` fills; the build checked the displacement stays in it.
    unsafe { write::<u32>(ctx.edx.wrapping_add_signed(i32::from(slot)), ctx.ecx) };
    ctx.resume = Site::of(DAMAGE_TAILS[arg].load(Ordering::Relaxed));
}

/// `lastspellsdamagetype` = 1: the damage types of Lightning, Astral Beam, Fire Arrow and Destroy Ray come from the
/// INI.
pub(crate) fn last_spells_damage_type(
    ctx: &Ctx<'_>,
    value: i8,
) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let mut slots = [0x14, 0x14, 0x0C, 0x0C];
    for (slot, key) in
        slots
            .iter_mut()
            .zip(["lightningbeam", "astralbeam", "firearrow", "destroyray"])
    {
        match damage_slot(ctx, key) {
            Ok(value) => *slot = value,
            Err(error) => (ctx.log)(
                crate::types::Level::Warn,
                "spelladdon",
                &error.to_string(),
                key,
            ),
        }
    }
    DAMAGE_SLOTS.store(u32::from_le_bytes(slots), Ordering::Relaxed);
    let mut checks = Vec::new();
    for (tail, site) in DAMAGE_TAILS
        .iter()
        .zip([DAMAGE_LIGHTNING_TAIL, DAMAGE_FIRE_TAIL])
    {
        let (check, copy) = hook::trampoline(site, &DAMAGE_TAIL_BYTES)?;
        checks.push(check);
        tail.store(copy, Ordering::Relaxed);
    }
    // The original's layout at both sites: `nop; jmp <added code>`.
    let mut writes = common::jump_at(DAMAGE_LIGHTNING, &DAMAGE_LIGHTNING_BYTES, 1, damage_type, 0)?;
    writes.extend(common::jump_at(
        DAMAGE_FIRE,
        &DAMAGE_FIRE_BYTES,
        1,
        damage_type,
        1,
    )?);
    writes.extend(checks);
    Ok(writes)
}

/// `CSpellEffect::GetDamage`+0x9B: `fild dword ptr [esp+0x14]` (the period), then `fdivr [0x73BAA0]; fstp
/// [edx+0x28]; fild [esi+0x54]; fdivr [0x73BAA0]` (the per-tick fraction, kept in ST0 for `fmul [edx+0x24]` at
/// 0x683A01). The patch calls added code from 0x6839F0 and keeps three floats after the call, where the code was.
const WALL_PERIOD_LOAD: (Site, &[u8]) = (
    eix_sites::C_SPELL_EFFECT_GET_DAMAGE__9B,
    &[0xDB, 0x44, 0x24, 0x14],
);
const WALL_DIVIDE_FIRST: (Site, &[u8]) = (eix_sites::C_SPELL_EFFECT_GET_DAMAGE__9F, &[0xD8]);
const WALL_CALL: Site = eix_sites::C_SPELL_EFFECT_GET_DAMAGE__A0;
const WALL_CALL_BYTES: [u8; 5] = [0x3D, 0xA0, 0xBA, 0x73, 0x00];
/// Acid, fire and lightning fractions, where the call is followed by data.
const WALL_FRACTIONS: Site = eix_sites::C_SPELL_EFFECT_GET_DAMAGE__A5;
const WALL_FRACTIONS_BYTES: [u8; 12] = [
    0xD9, 0x5A, 0x28, 0xDB, 0x46, 0x54, 0xD8, 0x3D, 0xA0, 0xBA, 0x73, 0x00,
];
/// Where the added code returns: `fmul dword ptr [edx+0x24]`.
const WALL_RESUME: Site = eix_sites::C_SPELL_EFFECT_GET_DAMAGE__B1;
/// The wall spell types: 6 the fire wall, 7 the lightning wall; the others take the acid fraction.
const FIRE_WALL: u32 = 6;
const LIGHTNING_WALL: u32 = 7;
/// What divides the fractions with value 1 (the module's own constant): they multiply the period then.
const WALL_PERIOD_SCALE: f32 = 150.0;

/// 1 when the handler multiplies the period on the stack by the fraction, 2 when it loads the fraction.
static WALL_MODE: AtomicU8 = AtomicU8::new(0);

/// The handler of `wallspercentage`: the fraction for the wall's type goes to `[edx+0x28]` and stays in ST0.
extern "C" fn wall_fraction(_: usize, ctx: &mut FpuContext) {
    let at = match ctx.regs.edi {
        FIRE_WALL => WALL_FRACTIONS.offset(4),
        LIGHTNING_WALL => WALL_FRACTIONS.offset(8),
        _ => WALL_FRACTIONS,
    };
    // SAFETY: the fractions the build wrote after the call.
    let fraction = f64::from(unsafe { read::<f32>(at.here32()) });
    let value = if WALL_MODE.load(Ordering::Relaxed) == 1 {
        // With value 1 the period `fild` before the call left it in ST0.
        ctx.fpu.pop().unwrap_or(Ext::new(0.0)).fmul(fraction)
    } else {
        Ext::new(fraction)
    };
    // SAFETY: EDX is the damage record `GetDamage` fills.
    unsafe { write::<f32>(ctx.regs.edx + 0x28, value.to_f32()) };
    // The code at the resume address multiplies with ST0 and pops it.
    let _ = ctx.fpu.push(value);
    ctx.regs.resume = WALL_RESUME;
}

/// `wallspercentage` 1 or 2: the walls' per-tick damage fraction comes from the INI per wall type; 1 scales the
/// period by the fractions (divided by 150), 2 takes them as they are.
pub(crate) fn walls_percentage(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if !matches!(value, 1 | 2) {
        return Ok(Vec::new());
    }
    let mut fractions = Vec::with_capacity(12);
    for key in [
        "acidpercentagevalue",
        "firepercentagevalue",
        "lightningpercentagevalue",
    ] {
        let mut f = ctx.values.float("gameplay", key)?;
        if value == 1 {
            f /= WALL_PERIOD_SCALE;
        }
        fractions.extend_from_slice(&f.to_le_bytes());
    }
    WALL_MODE.store(value as u8, Ordering::Relaxed);
    let mut writes = Vec::new();
    if value == 2 {
        writes.push(SiteWrite::nop(WALL_PERIOD_LOAD.0, WALL_PERIOD_LOAD.1));
    }
    writes.push(SiteWrite::nop(WALL_DIVIDE_FIRST.0, WALL_DIVIDE_FIRST.1));
    writes.push(hook::fpu_call(
        WALL_CALL,
        &WALL_CALL_BYTES,
        Mode::Drop,
        wall_fraction,
        0,
    )?);
    writes.push(SiteWrite::bytes(
        WALL_FRACTIONS,
        &WALL_FRACTIONS_BYTES,
        &fractions,
    )?);
    Ok(writes)
}

/// `fld [ebp-0x3C]; fsub [ebp-0x38]; fstp [ebp-0x14]` at 0x5248CB in the stats refresh: the Speed minus Slow effect
/// value.
const SPEED_SLOW: Site = eix_sites::UNIT_STATS_RECOMPUTE__36B;
const SPEED_SLOW_BYTES: [u8; 9] = [0xD9, 0x45, 0xC4, 0xD8, 0x65, 0xC8, 0xD9, 0x5D, 0xEC];
/// `UnitStats::RateValue`'s padding that keeps `speedslowmul`.
const SPEED_SLOW_MUL: Site = eix_sites::UNIT_STATS_RATE_VALUE__AC;

/// The handler of `speedslowremaster`: `[ebp-0x14] = speed * mul - slow * mul`.
extern "C" fn speed_slow(_: usize, ctx: &mut Context) {
    // SAFETY: the stats refresh's frame at the site.
    let speed = unsafe { read::<f32>(ctx.ebp - 0x3C) };
    // SAFETY: as above.
    let slow = unsafe { read::<f32>(ctx.ebp - 0x38) };
    // SAFETY: the factor the build wrote.
    let mul = f64::from(unsafe { read::<f32>(SPEED_SLOW_MUL.here32()) });
    let value = Ext::new(f64::from(speed))
        .fmul(mul)
        .fsub_ext(Ext::new(f64::from(slow)).fmul(mul));
    // SAFETY: as above.
    unsafe { write::<f32>(ctx.ebp - 0x14, value.to_f32()) };
}

/// `speedslowremaster` 1 or 2: the Speed and Slow effects count times `speedslowmul`.
pub(crate) fn speed_slow_remaster(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if !matches!(value, 1 | 2) {
        return Ok(Vec::new());
    }
    Ok(vec![
        common::float_over(ctx, "speedslowmul", SPEED_SLOW_MUL, [0x90; 4])?,
        hook::context_hook(SPEED_SLOW, &SPEED_SLOW_BYTES, Mode::Drop, speed_slow, 0)?,
    ])
}

/// `vampirism` = 1 is in the registry's skip list: its added code is copied from a stack buffer the module never fills,
/// so Link's update runs into breakpoint bytes. No other value changes anything.
pub(crate) fn vampirism(_: &Ctx<'_>, _: i8) -> Result<Vec<SiteWrite>, BuildError> {
    Ok(Vec::new())
}

/// `allowcastsonbriefings` = 1: `CWorldServer::CastSpell`+0x3D jumps past the briefing-zone refusal.
pub(crate) fn casts_on_briefings(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![SiteWrite::bytes(
        eix_sites::C_WORLD_SERVER_CAST_SPELL__3D,
        &[0x74],
        &[0xEB],
    )?])
}

/// `jl` of `CanDropItem`'s level tests.
const LEVEL_JL: [(Site, &[u8]); 3] = [
    (
        eix_sites::CAN_DROP_ITEM__CE,
        &[0x0F, 0x8C, 0x3F, 0x07, 0x00, 0x00],
    ),
    (
        eix_sites::CAN_DROP_ITEM__263,
        &[0x0F, 0x8C, 0xAA, 0x05, 0x00, 0x00],
    ),
    (
        eix_sites::CAN_DROP_ITEM__491,
        &[0x0F, 0x8C, 0x7C, 0x03, 0x00, 0x00],
    ),
];
/// `setge al` of `CanDropItem` and `CCampPages::CanMakeSpell`, which become `mov al, 1; nop`.
const LEVEL_SETGE: [Site; 4] = [
    eix_sites::CAN_DROP_ITEM__102,
    eix_sites::CAN_DROP_ITEM__274,
    eix_sites::CAN_DROP_ITEM__4A6,
    eix_sites::C_CAMP_PAGES_CAN_MAKE_SPELL__266,
];

/// `spellsarealwaysconstructable` = 1: spells can always be given to heroes and assembled.
pub(crate) fn always_constructable(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let mut writes = common::nops(&LEVEL_JL);
    for site in LEVEL_SETGE {
        writes.push(SiteWrite::bytes(
            site,
            &[0x0F, 0x9D, 0xC0],
            &[0xB0, 0x01, 0x90],
        )?);
    }
    // `CanMakeSpell`'s `xor al, al`.
    writes.push(SiteWrite::bytes(
        eix_sites::C_CAMP_PAGES_CAN_MAKE_SPELL__253,
        &[0x32, 0xC0],
        &[0xB0, 0x01],
    )?);
    Ok(writes)
}

/// Where `spellsaimod` puts `IsOffensive`'s longer table: header slack.
const OFFENSIVE_TABLE: Site = eix_sites::HEADER_OFFENSIVE_TABLE;
/// `IsOffensive`'s table for 54 spell types: the game's 38 entries, then the module's own for 0x26-0x35; the rest
/// of the 64 bytes is zero.
const OFFENSIVE_54: [u8; 64] = {
    let mut t = [0u8; 64];
    let ones = [
        9, 10, 11, 12, 13, 14, 15, 16, 18, 20, 21, 22, 23, 24, 26, 27, 28, 29, 31, 33, 35, 37, 43,
        45, 46,
    ];
    let mut i = 0;
    while i < ones.len() {
        t[ones[i]] = 1;
        i += 1;
    }
    t
};

/// `spellsaimod` = 1: `IsOffensive` reads its table, 54 types long, from the header slack; the `SpellKind` bound
/// becomes 0x36 as with the caster table.
pub(crate) fn spells_ai_mod(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![
        SiteWrite::bytes(OFFENSIVE_TABLE, &[0; 64], &OFFENSIVE_54)?,
        SiteWrite::pointer(
            eix_sites::C_SPELL_IS_OFFENSIVE__F,
            eix_sites::C_SPELL_IS_OFFENSIVE__28.ru(),
            OFFENSIVE_TABLE.ru(),
        ),
        SiteWrite::bytes(eix_sites::AI_ATTENTION_SPELL_KIND__13, &[0x29], &[0x36])?,
        SiteWrite::bytes(eix_sites::C_SPELL_IS_OFFENSIVE__8, &[0x26], &[0x36])?,
    ])
}

/// `je` in `CUnitServer::CanCast` (+0xA6) and `CUnitServer::FinishCast` (+0xF9) that spare units without a player.
const CAN_CAST_JE: (Site, &[u8]) = (eix_sites::C_UNIT_SERVER_CAN_CAST__A6, &[0x74, 0x39]);
const FINISH_CAST_JE: (Site, &[u8]) = (
    eix_sites::C_UNIT_SERVER_FINISH_CAST__F9,
    &[0x0F, 0x84, 0x8F, 0x00, 0x00, 0x00],
);
/// `mov ecx, [esi+0x1C]; call CUnit::HasPlayer; test eax, eax` in the two (from the second byte of the `mov`), and
/// value 4's `mov eax, [esi+0x28C]; nop; cmp eax, 0x10` (the AI class) in its place.
const HAS_PLAYER_TESTS: [Bytes; 2] = [
    (
        eix_sites::C_UNIT_SERVER_CAN_CAST__9D,
        &[0x4E, 0x1C, 0xE8, 0x7C, 0x41, 0xFE, 0xFF, 0x85, 0xC0],
        &[0x86, 0x8C, 0x02, 0x00, 0x00, 0x90, 0x83, 0xF8, 0x10],
    ),
    (
        eix_sites::C_UNIT_SERVER_FINISH_CAST__F0,
        &[0x4E, 0x1C, 0xE8, 0x99, 0x12, 0xFE, 0xFF, 0x85, 0xC0],
        &[0x86, 0x8C, 0x02, 0x00, 0x00, 0x90, 0x83, 0xF8, 0x10],
    ),
];

/// `npcsmana` 1: `FinishCast` charges every unit; 2: `CanCast` checks every unit's mana; 3: both; 4: both tests
/// look at the AI class instead of the player.
pub(crate) fn npcs_mana(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    match value {
        1 => Ok(common::nops(&[FINISH_CAST_JE])),
        2 => Ok(common::nops(&[CAN_CAST_JE])),
        3 => Ok(common::nops(&[CAN_CAST_JE, FINISH_CAST_JE])),
        4 => common::fixed(&HAS_PLAYER_TESTS),
        _ => Ok(Vec::new()),
    }
}

/// `nonstopmanaregen` >= 1: `UnitVitals::Regenerate`+0x34's `jne` that stops regeneration goes.
pub(crate) fn nonstop_mana_regen(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    Ok(vec![SiteWrite::nop(
        eix_sites::UNIT_VITALS_REGENERATE__34,
        &[0x0F, 0x85, 0xB8, 0x00, 0x00, 0x00],
    )])
}

/// `hidefireworks` = 1: `CSpellEffect::Start`+0x1824 jumps over the Fireworks drawing setup to +0x1847.
pub(crate) fn hide_fireworks(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![SiteWrite::bytes(
        eix_sites::C_SPELL_EFFECT_START__1824,
        &[0x8B, 0x45, 0xD4],
        &[0xEB, 0x21, 0x90],
    )?])
}

/// `changesounds` = 1: the fireball's volume factor pushed in `CSpellEffect::UpdateMissile`+0x9EF is `fireboom`.
pub(crate) fn change_sounds(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![common::float(
        ctx,
        "fireboom",
        eix_sites::C_SPELL_EFFECT_UPDATE_MISSILE__9ED,
        5.0,
    )?])
}

/// The period byte (3) of the four wall spells' damage ticks in `CSpellEffect::UpdateLasting`.
const WALL_PERIODS: [Site; 4] = [
    eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__46B,
    eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__5D5,
    eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__2CD,
    eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__687,
];

/// `wallsfrequency` = 1: the walls hit every `frequencyvalue` ticks.
pub(crate) fn walls_frequency(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let period = ctx.values.byte("gameplay", "frequencyvalue")?;
    WALL_PERIODS
        .iter()
        .map(|&site| Ok(SiteWrite::bytes(site, &[0x03], &[period])?))
        .collect()
}

/// `cmp dword ptr [eax+0x10], 0x3006; je +9` at `CanDropItem`+0x790: a wand (kind 0x3006) skips the spell test
/// `call 0x68B7C0` at 0x610879, whose result `test al, al` reads at 0x61087E. ECX is the dropped spell.
const WAND_TEST: Site = eix_sites::CAN_DROP_ITEM__790;
const WAND_TEST_BYTES: [u8; 9] = [0x81, 0x78, 0x10, 0x06, 0x30, 0x00, 0x00, 0x74, 0x09];
const WAND: u32 = 0x3006;
const DROP_SPELL_TEST: Site = eix_sites::CAN_DROP_ITEM__799;
const DROP_RESULT: Site = eix_sites::CAN_DROP_ITEM__79E;
/// A spell's modifiers: up to eight 1-based rune record numbers from +0x54, ended by 0.
const SPELL_MODIFIERS: u32 = 0x54;
/// The rune record's size and its code's offset; the codes a wand refuses, "ic" and "it".
const RUNE_RECORD: u32 = 0x24;
const RUNE_CODE: u32 = 4;
const REFUSED_CODES: [u32; 2] = [0x6369, 0x7469];

/// The handler of `batterycharge`: a spell dropped on a wand is refused (AL 0) when one of its modifiers' runes has the
/// code "ic" or "it"; other items take the game's test.
extern "C" fn battery_charge(_: usize, ctx: &mut Context) {
    // SAFETY: EAX is the dragged item `CanDropItem` tests.
    if unsafe { read::<u32>(ctx.eax + 0x10) } != WAND {
        ctx.resume = DROP_SPELL_TEST;
        return;
    }
    // SAFETY: the rune table the databases filled.
    let runes = unsafe { RUNES.read() };
    let count = (runes.end.wrapping_sub(runes.begin) as i32 / RUNE_RECORD as i32) as u32;
    let mut allowed = 1;
    for i in 0..8 {
        // SAFETY: ECX is the dropped spell, whose modifiers are eight bytes.
        let number = unsafe { read::<i8>(ctx.ecx + SPELL_MODIFIERS + i) };
        if number == 0 {
            break;
        }
        let index = (i32::from(number) - 1) as u32;
        // The original reads address 4 for a number past the table and faults; here that modifier is passed over.
        if index >= count {
            continue;
        }
        // SAFETY: a record inside the table.
        let code = unsafe { read::<u32>(runes.begin + index * RUNE_RECORD + RUNE_CODE) };
        if REFUSED_CODES.contains(&code) {
            allowed = 0;
            break;
        }
    }
    ctx.eax = allowed;
    ctx.resume = DROP_RESULT;
}

/// `batterycharge` >= 1: spells with the "ic" or "it" modifier cannot be put into wands.
pub(crate) fn battery_charge_build(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    Ok(vec![hook::context_hook(
        WAND_TEST,
        &WAND_TEST_BYTES,
        Mode::Drop,
        battery_charge,
        0,
    )?])
}

/// `CUnitServer::TickItemSpells`+0x60 rewritten in place: after `cmp eax, 0x180000` (Healing, the spell type in the
/// high word) the `jne` goes to a test of type 0x2B (Inspiration); each type jumps to its fullness test, at offsets 1
/// and 15.
const ITEM_SPELL_TESTS: Site = eix_sites::C_UNIT_SERVER_TICK_ITEM_SPELLS__60;
const ITEM_SPELL_TESTS_WAS: [u8; 33] = [
    0x20, 0x8A, 0x44, 0x24, 0x13, 0x84, 0xC0, 0x75, 0x51, 0x8B, 0x45, 0x1C, 0x66, 0x8B, 0x90, 0x7E,
    0x01, 0x00, 0x00, 0x66, 0x3B, 0x90, 0x80, 0x01, 0x00, 0x00, 0x74, 0x3E, 0xC6, 0x44, 0x24, 0x13,
    0x01,
];
const ITEM_SPELL_TESTS_NOW: [u8; 33] = [
    0x07, 0xE9, 0x00, 0x00, 0x00, 0x00, 0xEB, 0x19, 0x3D, 0x00, 0x00, 0x2B, 0x00, 0x75, 0x12, 0xE9,
    0x00, 0x00, 0x00, 0x00, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90,
];
const HEALING_JUMP: usize = 1;
const INSPIRATION_JUMP: usize = 15;
/// The unit's current and maximum health (16-bit, +0x17E and +0x180) and mana (+0x182, +0x184).
const HEALTH_PAIR: u32 = 0x17E;
const MANA_PAIR: u32 = 0x182;
/// `TickItemSpells`' paths: cast the item's spell, or pass the item over.
const ITEM_SPELL_CAST: Site = eix_sites::C_UNIT_SERVER_TICK_ITEM_SPELLS__81;
const ITEM_SPELL_SKIP: Site = eix_sites::C_UNIT_SERVER_TICK_ITEM_SPELLS__BA;

/// An item spell's fullness test, `arg` the unit's pair of 16-bit values (health for Healing, mana for
/// Inspiration): once one such spell was cast this tick (the byte `[esp+0x13]`), or while the value is full, the
/// item is passed over; otherwise the byte is set and the spell is cast.
extern "C" fn item_spell_full(pair: usize, ctx: &mut Context) {
    let cast_flag = (ctx.stack() + 0x13) as u32;
    // SAFETY: a byte of `TickItemSpells`' frame.
    let cast = unsafe { read::<u8>(cast_flag) };
    ctx.eax = (ctx.eax & !0xFF) | u32::from(cast);
    if cast != 0 {
        ctx.resume = ITEM_SPELL_SKIP;
        return;
    }
    // SAFETY: EBP is the unit controller; +0x1C is its unit.
    let unit = unsafe { read::<u32>(ctx.ebp + 0x1C) };
    // SAFETY: the unit's pair of values.
    let now = unsafe { read::<u16>(unit + pair as u32) };
    // SAFETY: as above.
    let full = unsafe { read::<u16>(unit + pair as u32 + 2) };
    ctx.eax = unit;
    ctx.edx = (ctx.edx & !0xFFFF) | u32::from(now);
    if now == full {
        ctx.resume = ITEM_SPELL_SKIP;
        return;
    }
    // SAFETY: as above.
    unsafe { write::<u8>(cast_flag, 1) };
    ctx.resume = ITEM_SPELL_CAST;
}

/// `inspirationintelligence` 1: an item's Inspiration is not cast at full mana, as its Healing is not at full health.
/// The registry skips 2 and 3, which send Healing items into the middle of an instruction.
pub(crate) fn inspiration_intelligence(
    _: &Ctx<'_>,
    value: i8,
) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let mut now = ITEM_SPELL_TESTS_NOW;
    for (at, pair) in [(HEALING_JUMP, HEALTH_PAIR), (INSPIRATION_JUMP, MANA_PAIR)] {
        let to = common::entry(item_spell_full, pair as usize)?;
        now[at..at + 5].copy_from_slice(&patch::jmp_rel32(ITEM_SPELL_TESTS.offset(at), to));
    }
    Ok(vec![SiteWrite::bytes(
        ITEM_SPELL_TESTS,
        &ITEM_SPELL_TESTS_WAS,
        &now,
    )?])
}

/// `mov eax, [ebx+0xA0]` at `CSpellEffect::UpdateLasting`+0x832, Link's tick counter before its `dec`.
const LINK_TICK: Site = eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__832;
const LINK_TICK_BYTES: [u8; 6] = [0x8B, 0x83, 0xA0, 0x00, 0x00, 0x00];
/// A unit's vitals (`[unit+0x170]`) hold its mana at +0x34 and the maximum at +0x38.
const VITALS: u32 = 0x170;
const MANA: u32 = 0x34;

/// The handler of `linkremaster` 1: every 32nd tick the mana of the two linked units moves by the effect's strength
/// (`[ebx+0x58]`) from the one fuller in proportion to its maximum (ECX, the caster, and the target
/// `[ebx+0x1C]`) to the other, and both are set to the shared proportion once a step would pass it.
extern "C" fn link_mana(_: usize, ctx: &mut Context) {
    // SAFETY: EBX is the Link effect.
    ctx.eax = unsafe { read::<u32>(ctx.ebx + 0xA0) };
    if ctx.eax & 0x1F != 0 {
        return;
    }
    // SAFETY: ECX is a live unit, as the original takes it.
    let a = unsafe { read::<u32>(ctx.ecx + VITALS) } + MANA;
    // SAFETY: the effect's target.
    let target = unsafe { read::<u32>(ctx.ebx + 0x1C) };
    // SAFETY: as above.
    let b = unsafe { read::<u32>(target + VITALS) } + MANA;
    let get = |at: u32| {
        // SAFETY: a mana value or maximum of the two vitals.
        f64::from(unsafe { read::<f32>(at) })
    };
    let (a_now, a_max, b_now, b_max) = (get(a), get(a + 4), get(b), get(b + 4));
    // SAFETY: the effect's strength.
    let step = f64::from(unsafe { read::<f32>(ctx.ebx + 0x58) });
    let ratio = Ext::new(a_max).fdiv(b_max);
    let b_share = Ext::new(b_now)
        .fadd(a_now)
        .fdiv_ext(Ext::new(1.0).fadd_ext(ratio));
    let a_share = ratio.fmul_ext(b_share);
    let a_full = Ext::new(a_now).fdiv(a_max);
    let b_full = Ext::new(b_now).fdiv(b_max);
    let set = |at: u32, v: f32| {
        // SAFETY: as in `get`.
        unsafe { write::<f32>(at, v) }
    };
    // The unit less full receives; it stops at its share. Each step reads the value it changes, as the original does.
    let (low, low_now, low_share, high) = if b_full.fcom_ext(a_full) & C0 != 0 {
        (b, b_now, b_share, a)
    } else {
        (a, a_now, a_share, b)
    };
    if Ext::new(low_now).fadd(step).fcom_ext(low_share) & C0 != 0 {
        set(high, Ext::new(get(high)).fsub(step).to_f32());
        set(low, Ext::new(get(low)).fadd(step).to_f32());
    } else {
        set(b, b_share.to_f32());
        set(a, a_share.to_f32());
    }
}

/// `linkremaster` 1: Link shares the two units' mana. The registry skips 2 and 3, which run into a `hlt` with one
/// word too many on the stack.
pub(crate) fn link_remaster(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(common::jump_at(
        LINK_TICK,
        &LINK_TICK_BYTES,
        1,
        link_mana,
        0,
    )?)
}

/// `jne +0x15; mov eax, [esi+0x6C]` at `CSpellContainer::Build`+0x2A5, after `cmp al, 't'` on a modifier's code
/// letter: 't' and 'c' set flags in `[esi+0x6C]`.
const FINALIZE_LETTER: Site = eix_sites::C_SPELL_CONTAINER_BUILD__2A5;
const FINALIZE_LETTER_BYTES: [u8; 5] = [0x75, 0x15, 0x8B, 0x46, 0x6C];
const FINALIZE_T: Site = eix_sites::C_SPELL_CONTAINER_BUILD__2AA;
const FINALIZE_STORE: Site = eix_sites::C_SPELL_CONTAINER_BUILD__2B9;
const FINALIZE_NEXT: Site = eix_sites::C_SPELL_CONTAINER_BUILD__2BC;
/// The flag of the 'f' modifier.
const FAKE_SOURCE: u32 = 0x8_0000;
/// `lea eax, [ebx+0x1C]; mov ecx, [ebx+0x1C]` at `CWorldServer::CreateMissile`+0x75: the caster's position becomes
/// the missile's start; +0x8D builds the missile without it.
const MISSILE_SOURCE: Site = eix_sites::C_WORLD_SERVER_CREATE_MISSILE__75;
const MISSILE_SOURCE_BYTES: [u8; 6] = [0x8D, 0x43, 0x1C, 0x8B, 0x4B, 0x1C];
const MISSILE_NO_SOURCE: Site = eix_sites::C_WORLD_SERVER_CREATE_MISSILE__8D;
/// `mov edx, [ecx]; lea eax, [ebp-0x318]` at `CSpellEffect::Start`+0x59: the effect takes its target unit's
/// position; +0x42 takes the effect's own point instead.
const EFFECT_TARGET: Site = eix_sites::C_SPELL_EFFECT_START__59;
const EFFECT_TARGET_BYTES: [u8; 8] = [0x8B, 0x11, 0x8D, 0x85, 0xE8, 0xFC, 0xFF, 0xFF];
const EFFECT_POINT: Site = eix_sites::C_SPELL_EFFECT_START__42;

/// `Build`'s letter test: 't' as before; 'f' sets [`FAKE_SOURCE`].
extern "C" fn finalize_letter(_: usize, ctx: &mut Context) {
    // SAFETY: ESI is the spell container `Build` fills.
    let flags = unsafe { read::<u32>(ctx.esi + 0x6C) };
    if ctx.eflags & 0x40 != 0 {
        ctx.eax = flags;
        ctx.resume = FINALIZE_T;
    } else if ctx.eax as u8 == b'f' {
        ctx.eax = flags | FAKE_SOURCE;
        ctx.resume = FINALIZE_STORE;
    } else {
        ctx.resume = FINALIZE_NEXT;
    }
}

/// `CreateMissile` for a spell with the 'f' modifier (the spell object at `[esp+0x30]`, flags at +0x3C) starts
/// the missile without the caster's position.
extern "C" fn missile_source(_: usize, ctx: &mut Context) {
    let Some(spell) = ctx.stack_word(0x30 / 4) else {
        return;
    };
    ctx.ecx = spell;
    // SAFETY: the spell `CreateMissile` was given.
    if unsafe { read::<u32>(spell + 0x3C) } & FAKE_SOURCE != 0 {
        ctx.resume = MISSILE_NO_SOURCE;
        return;
    }
    ctx.eax = ctx.ebx + 0x1C;
    // SAFETY: EBX is the caster, a live unit.
    ctx.ecx = unsafe { read::<u32>(ctx.ebx + 0x1C) };
}

/// `Start` for an effect with the 'f' modifier (`[ebx+0x70]`) takes its own point, as an effect without a target.
extern "C" fn effect_target(_: usize, ctx: &mut Context) {
    // SAFETY: EBX is the effect `Start` runs on.
    ctx.eax = unsafe { read::<u32>(ctx.ebx + 0x70) };
    if ctx.eax & FAKE_SOURCE != 0 {
        ctx.resume = EFFECT_POINT;
        return;
    }
    // SAFETY: ECX is the target unit, whose first word is its vtable.
    ctx.edx = unsafe { read::<u32>(ctx.ecx) };
    ctx.eax = ctx.ebp - 0x318;
}

/// `fakesrcrune` = 1: the modifier 'f' makes a spell start from its target point instead of the caster.
pub(crate) fn fake_source_rune(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let mut writes = common::jump_at(MISSILE_SOURCE, &MISSILE_SOURCE_BYTES, 1, missile_source, 0)?;
    writes.extend(common::jump_at(
        EFFECT_TARGET,
        &EFFECT_TARGET_BYTES,
        3,
        effect_target,
        0,
    )?);
    writes.push(hook::context_hook(
        FINALIZE_LETTER,
        &FINALIZE_LETTER_BYTES,
        Mode::Drop,
        finalize_letter,
        0,
    )?);
    Ok(writes)
}

/// `lea ecx, [ebp-0x320]` at `CSpellEffect::Start`+0x1639, before the effect is applied (0x680EAF); ESI is the spell
/// type, `[ebx+0x58]` the strength, `[ebx+0x1C]` the unit whose resistance byte +0x40A the control spells meet.
const CONTROL_SITE: Site = eix_sites::C_SPELL_EFFECT_START__1639;
const CONTROL_SITE_BYTES: [u8; 6] = [0x8D, 0x8D, 0xE0, 0xFC, 0xFF, 0xFF];
/// `Start`+0x1BE1: the effect ends without being applied.
const CONTROL_FAILED: Site = eix_sites::C_SPELL_EFFECT_START__1BE1;
/// Value 1's multipliers: `weaknessmultiplier`, `slownessmultiplier`, then a -1.0 its code does not read.
const WEAKNESS_MUL: Site = eix_sites::DATA_79A0B0;
const SLOWNESS_MUL: Site = eix_sites::DATA_79A0B4;
const CONTROL_MINUS_ONE: Site = eix_sites::DATA_79A0B8;
/// The control spell types of value 1: Stun 0x19 and 0x20 compare, Weaken 0x1E and Slow 0x22 are reduced first.
const STUN: u32 = 0x19;
const STUN_LIKE: u32 = 0x20;
const WEAKEN: u32 = 0x1E;
const SLOW: u32 = 0x22;

/// The AH that `fnstsw ax` gives after the original's `fcom st(1)` with two values pushed: the condition bits of
/// `c`, the stack top two below the one at the site, and the busy bit.
fn compare_ah(c: u16) -> u8 {
    let live = eix_engine::x87::status_word();
    let top = ((live >> 11).wrapping_add(6)) & 7;
    ((c >> 8) as u8) | ((top as u8) << 3) | ((live >> 8) as u8 & 0x80)
}

/// The handler of `controlmagic` 1, after the replaced `lea`: Stun-like spells apply only while the target's resistance
/// `r` is below the strength `s`; Weaken's strength becomes `(s * weaknessmultiplier - r) / weaknessmultiplier` and
/// applies while above `r`; Slow's becomes `(s * m - r) / m` and applies while that times `m` is above `r`. Each
/// test also needs an empty x87 stack at the site, as the original's `cmp ah` on the whole status byte does.
extern "C" fn control_spell(_: usize, ctx: &mut Context) {
    ctx.ecx = ctx.ebp.wrapping_sub(0x320);
    let kind = ctx.esi;
    if ![STUN, STUN_LIKE, WEAKEN, SLOW].contains(&kind) {
        return;
    }
    // SAFETY: the effect's unit, a live unit.
    let unit = unsafe { read::<u32>(ctx.ebx + 0x1C) };
    // SAFETY: as above.
    let resist = Ext::new(f64::from(unsafe { read::<u8>(unit + 0x40A) }));
    let strength_at = ctx.ebx + 0x58;
    // SAFETY: the effect's strength.
    let strength = f64::from(unsafe { read::<f32>(strength_at) });
    let applies = if kind == STUN || kind == STUN_LIKE {
        compare_ah(resist.fcom(strength)) == 0x31
    } else {
        let at = if kind == WEAKEN {
            WEAKNESS_MUL
        } else {
            SLOWNESS_MUL
        };
        // SAFETY: the multiplier the build wrote.
        let mul = f64::from(unsafe { read::<f32>(at.here32()) });
        let reduced = Ext::new(strength).fmul(mul).fsub_ext(resist).fdiv(mul);
        // SAFETY: as above.
        unsafe { write::<f32>(strength_at, reduced.to_f32()) };
        let tested = if kind == WEAKEN {
            reduced
        } else {
            reduced.fmul(mul)
        };
        compare_ah(tested.fcom_ext(resist)) == 0x30
    };
    if !applies {
        ctx.resume = CONTROL_FAILED;
    }
}

/// The 22 spell types of the per-spell keys, in the INI's order: `<name>` switches a type on for value 3,
/// `b<name>` is its strength bonus.
const SPELL_BONUSES: [(u32, &str); 22] = [
    (0x09, "prot_fire"),
    (0x0A, "prot_electro"),
    (0x0B, "prot_acid"),
    (0x0C, "eagle_sight"),
    (0x0D, "infravision"),
    (0x0E, "detect_life"),
    (0x0F, "invisibility"),
    (0x10, "silence"),
    (0x11, "lichdom"),
    (0x13, "stench"),
    (0x19, "stun"),
    (0x1A, "antimagic"),
    (0x1D, "strength"),
    (0x1E, "weak"),
    (0x1F, "regeneration"),
    (0x20, "feeblemind"),
    (0x21, "speed"),
    (0x22, "slow"),
    (0x25, "enlarge"),
    (0x26, "shrink"),
    (0x2D, "necro"),
    (0x2E, "destroy"),
];

/// Each type's bonus in [`SPELL_BONUSES`]' order, `None` where value 3 leaves the type out.
type Bonuses = [Option<f32>; 22];

/// The bonuses `controlmagic` `value` applies. Value 3 keeps a type's compare for an odd switch and jumps over it for
/// an even one.
fn bonuses(ctx: &Ctx<'_>, value: i8) -> Result<Bonuses, BuildError> {
    let mut out = [None; 22];
    for (slot, &(_, name)) in out.iter_mut().zip(&SPELL_BONUSES) {
        if value != 3 || ctx.values.byte("gameplay", name)? & 1 == 1 {
            *slot = Some(ctx.values.float("gameplay", &format!("b{name}"))?);
        }
    }
    Ok(out)
}

/// Constants values 2 and up write into `.text` padding for code of theirs that never runs: -0.8 at 0x401538 and
/// `lvldurcoeff` at 0x409A38.
const CONTROL_MINUS_POINT_8: Site = eix_sites::C_TRANSFORM_OBJECT_SAVE_TRANSFORM_IMPL__28;
const LEVEL_DURATION_COEFF: Site = eix_sites::CHUNK_ARCHIVE_LEAVE_CHUNK__B8;

/// The arena routine of `controlmagic` 2, 3, 5 and 7, called before the effect is applied: ECX becomes the replaced
/// `lea`'s value, and a spell of one of the 22 types gets its bonus added to the strength `[ebx+0x58]` (with value
/// 3 only the types switched on). Every type's block jumps over its duration and resistance code.
extern "C" fn control_bonus(arg: usize, ctx: &mut Context) {
    ctx.ecx = ctx.ebp.wrapping_sub(0x320);
    // SAFETY: the build kept the table at `arg`.
    let table = unsafe { common::kept::<Bonuses>(arg) };
    let Some(Some(bonus)) = SPELL_BONUSES
        .iter()
        .position(|&(t, _)| t == ctx.esi)
        .map(|i| table[i])
    else {
        return;
    };
    let bonus = f64::from(bonus);
    let at = ctx.ebx + 0x58;
    // SAFETY: EBX is the effect `CSpellEffect::Start` runs on; +0x58 is its strength.
    let strength = f64::from(unsafe { read::<f32>(at) });
    // SAFETY: as above.
    unsafe { write::<f32>(at, Ext::new(strength).fadd(bonus).to_f32()) };
}

/// `controlmagic` 1: control spells meet the target's resistance. 2, 3, 5, 7: the 22 spell types get their strength
/// bonus (3: only those switched on). The registry skips 4 and 6, which jump into the middle of an instruction in
/// every spell's block.
pub(crate) fn control_magic(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    let mut writes = match value {
        1 => common::jump_at(CONTROL_SITE, &CONTROL_SITE_BYTES, 1, control_spell, 0)?,
        2 | 3 | 5 | 7 => {
            let table = common::keep(bonuses(ctx, value)?);
            let mut w =
                common::call_at(CONTROL_SITE, &CONTROL_SITE_BYTES, 1, control_bonus, table)?;
            w.push(SiteWrite::bytes(
                CONTROL_MINUS_POINT_8,
                &[0x90; 4],
                &(-0.8f32).to_le_bytes(),
            )?);
            w.push(common::float_over(
                ctx,
                "lvldurcoeff",
                LEVEL_DURATION_COEFF,
                [0x90; 4],
            )?);
            w
        }
        _ => return Ok(Vec::new()),
    };
    writes.push(common::float_over(
        ctx,
        "weaknessmultiplier",
        WEAKNESS_MUL,
        [0; 4],
    )?);
    writes.push(common::float_over(
        ctx,
        "slownessmultiplier",
        SLOWNESS_MUL,
        [0; 4],
    )?);
    writes.push(common::float_at(CONTROL_MINUS_ONE, 0.0, -1.0)?);
    Ok(writes)
}

/// Where a spell line is formatted: `mov ecx, [panel+...]; push eax` (the value `_ftol` returned), then the name
/// pushed, the format, the `CString` and `CString::Format`. The patch writes `call <added code>; jmp short` over the
/// first seven bytes; the jump's displacement leads past the original's `Format` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpellLine {
    /// `CItemInfoPanel::DescribeSpellRecord`+0x2B0: EBP the panel, the record at `[esp+4]`, its type at +0x54.
    Record,
    /// `CItemInfoPanel::DescribeCarriedSpell`+0x331: EBX the panel, EBP the spell, its type the high word of +0x18.
    Carried,
    /// `CHudSpellColumn::SetUnit`+0x238: ESI the column, EDI the spell; the line shows the sum.
    Hud,
}

/// `(site, its seven bytes, the short jump's displacement, which line)`.
const SPELL_LINES: [(Site, [u8; 7], u8, SpellLine); 3] = [
    (
        eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_CARRIED_SPELL__331,
        [0x8B, 0x8B, 0xCC, 0x01, 0x00, 0x00, 0x50],
        0x1A,
        SpellLine::Carried,
    ),
    (
        eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_SPELL_RECORD__2B0,
        [0x8B, 0x8D, 0xCC, 0x01, 0x00, 0x00, 0x50],
        0x23,
        SpellLine::Record,
    ),
    (
        eix_sites::C_HUD_SPELL_COLUMN_SET_UNIT__238,
        [0x8B, 0x8E, 0x5C, 0x01, 0x00, 0x00, 0x50],
        0x1A,
        SpellLine::Hud,
    ),
];
/// The game's formats: "%s %d" for the panels, "%s %d  " for the HUD; and the patch's "%s %d+%d".
const PANEL_FORMAT: Site = eix_sites::DATA_792F14;
const HUD_FORMAT: Site = eix_sites::DATA_794500;
static BONUS_FORMAT: &[u8] = b"%s %d+%d\0";
/// `CString::Format` 0x6FDF5F: cdecl, the string first.
const CSTRING_FORMAT: Site = eix_sites::C_STRING_FORMAT;
type CStringFormat = unsafe extern "C" fn(string: u32, format: *const u8, ...);

/// What a spell line's handler needs: which line, and the bonuses.
#[derive(Debug)]
struct LineSetup {
    line: SpellLine,
    bonuses: Bonuses,
}

/// The handler of `effectredraws` at a spell line: a spell of one of the 22 types shows `value+bonus` (the HUD the
/// sum), the bonus rounded under the control word; others the game's line. The string is the site's `[esp+0x10]`
/// (HUD `[esp+0x48]`), as the replaced code formats it.
extern "C" fn spell_line(arg: usize, ctx: &mut Context) {
    // SAFETY: the build kept the setup at `arg`.
    let setup = unsafe { common::kept::<LineSetup>(arg) };
    let stack = ctx.stack() as u32;
    let (kind, panel, name) = match setup.line {
        SpellLine::Record => {
            // SAFETY: the record `DescribeSpellRecord` keeps at `[esp+4]`.
            let record = unsafe { read::<u32>(stack + 4) };
            // SAFETY: its type.
            let kind = unsafe { read::<u32>(record + 0x54) };
            // SAFETY: EBP is the panel, whose +0x1CC holds the line's text source.
            (kind, unsafe { read::<u32>(ctx.ebp + 0x1CC) }, 0x20)
        }
        SpellLine::Carried => {
            // SAFETY: EBP is the carried spell.
            let kind = unsafe { read::<u32>(ctx.ebp + 0x18) } >> 16;
            // SAFETY: EBX is the panel, as above.
            (kind, unsafe { read::<u32>(ctx.ebx + 0x1CC) }, 0x20)
        }
        SpellLine::Hud => {
            // SAFETY: EDI is the column's spell.
            let kind = unsafe { read::<u32>(ctx.edi + 0x18) } >> 16;
            // SAFETY: ESI is the column, whose +0x15C holds the text source.
            (kind, unsafe { read::<u32>(ctx.esi + 0x15C) }, 8)
        }
    };
    // SAFETY: the text source's name.
    let name = unsafe { read::<u32>(panel + name) };
    let bonus = SPELL_BONUSES
        .iter()
        .position(|&(t, _)| t == kind)
        .and_then(|i| setup.bonuses[i])
        .map(|b| Ext::new(f64::from(b)).fistp());
    let value = ctx.eax as i32;
    // SAFETY: 0x6FDF5F is `void __cdecl CString::Format(CString*, LPCTSTR, ...)`.
    let format = unsafe { hook::as_fn::<CStringFormat>(CSTRING_FORMAT) };
    let plain = |at: Site| at.ptr::<u8>().cast_const();
    if setup.line == SpellLine::Hud {
        let string = stack + 0x48;
        let shown = bonus.map_or(value, |b| value.wrapping_add(b));
        // SAFETY: the column's `CString` local, the game's format and arguments, on the game's thread.
        unsafe { format(string, plain(HUD_FORMAT), name, shown) };
        ctx.ecx = string;
        return;
    }
    let string = stack + 0x10;
    match bonus {
        // SAFETY: the panel's `CString` local, as above, with our format of two numbers.
        Some(b) => unsafe { format(string, BONUS_FORMAT.as_ptr(), name, value, b) },
        // SAFETY: as above, the game's own format.
        None => unsafe { format(string, plain(PANEL_FORMAT), name, value) },
    }
    // SAFETY: the `CString`'s buffer pointer, which the code after the site draws.
    ctx.edx = unsafe { read::<u32>(string) };
}

/// `DescribeSpellRecord`+0x2C0 to its end (736 bytes): the patch moves its code 9 bytes up into the padding after
/// its `ret` and puts 9 NOPs in front; the short jump into +0x2C0 then runs the NOPs. The bytes are read from the
/// running game and checked by hash: they are game code, not ours to carry.
const RECORD_CODE: Site = eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_SPELL_RECORD__2C0;
const RECORD_CODE_LEN: usize = 736;
const RECORD_SHIFT: usize = 9;
/// FNV-1a 64 of a supported game.exe's bytes there.
const RECORD_CODE_HASH: u64 = 0xE07C_E8D6_323D_C005;
/// Where the moved code's calls out of the range keep their 32-bit displacements (offsets into the range).
const RECORD_CODE_CALLS: [usize; 23] = [
    6, 37, 93, 119, 150, 179, 205, 236, 292, 329, 360, 408, 470, 501, 541, 572, 624, 631, 645, 659,
    673, 687, 704,
];

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xCBF2_9CE4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01B3)
    })
}

/// `code` moved `by` bytes up over its trailing NOPs, with NOPs in front and the displacements at `calls` (which
/// leave the range) shortened by `by`.
fn shift_up(code: &[u8], by: usize, calls: &[usize]) -> Result<Vec<u8>, BuildError> {
    let keep = code.len() - by;
    if code[keep..].iter().any(|&b| b != 0x90) {
        return Err(BuildError(
            "the code to move has no room after it".to_owned(),
        ));
    }
    let mut out = vec![0x90; by];
    out.extend_from_slice(&code[..keep]);
    for &at in calls {
        let field = &mut out[by + at..by + at + 4];
        let rel = i32::from_le_bytes([field[0], field[1], field[2], field[3]]);
        field.copy_from_slice(&rel.wrapping_sub(by as i32).to_le_bytes());
    }
    Ok(out)
}

/// `effectredraws` = 1: spell lines in the item panels and the HUD show the `b<spell>` bonus.
pub(crate) fn effect_redraws(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let table = bonuses(ctx, 2)?;
    let mut writes = Vec::new();
    for (site, was, jump, line) in SPELL_LINES {
        let setup = common::keep(LineSetup {
            line,
            bonuses: table,
        });
        writes.push(hook::context_call(
            site,
            &was[..5],
            Mode::Drop,
            spell_line,
            setup,
        )?);
        writes.push(SiteWrite::bytes(site.offset(5), &was[5..], &[0xEB, jump])?);
    }
    // SAFETY: game.exe's code, mapped and readable; nothing writes it while the features are prepared.
    let code = unsafe { patch::read(RECORD_CODE, RECORD_CODE_LEN) };
    if fnv1a(&code) != RECORD_CODE_HASH {
        return Err(BuildError(format!(
            "the code at {RECORD_CODE:#x} is not game.exe RU 1.06's"
        )));
    }
    let moved = shift_up(&code, RECORD_SHIFT, &RECORD_CODE_CALLS)?;
    writes.push(SiteWrite::bytes(RECORD_CODE, &code, &moved)?);
    Ok(writes)
}

// --- newspells ---------------------------------------------------------------------------------------------------------

/// The spell-type bounds of `CSpellEffect::Start` (`cmp esi, 0x29`), `UpdateMissile` (`cmp eax, 0x28`) and `GetDamage`
/// (`cmp edi, 0x29`), and the case bytes of the new types after each one's table (from 0x2A, 0x29 and 0x2A). The
/// original raises the bounds to 0x36 over the same tables, so 0x34-0x36 dispatch through the code after them; ending
/// the bounds at [`LAST_SPELL_TYPE`] sends those types to each function's out-of-range path instead.
const SPELL_TYPES: [Bytes; 6] = [
    (
        eix_sites::C_SPELL_EFFECT_START__83,
        &[0x29],
        &[LAST_SPELL_TYPE],
    ),
    (
        eix_sites::C_SPELL_EFFECT_START__2306,
        &[0x90; 10],
        &[0x11, 0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x03, 0x04, 0x05],
    ),
    (
        eix_sites::C_SPELL_EFFECT_UPDATE_MISSILE__10,
        &[0x28],
        &[LAST_SPELL_TYPE],
    ),
    (
        eix_sites::C_SPELL_EFFECT_UPDATE_MISSILE__A95,
        &[0x90; 11],
        &[
            0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x02, 0x02, 0x02, 0x02, 0x02,
        ],
    ),
    (
        eix_sites::C_SPELL_EFFECT_GET_DAMAGE__3E,
        &[0x29],
        &[LAST_SPELL_TYPE],
    ),
    (
        eix_sites::C_SPELL_EFFECT_GET_DAMAGE__106,
        &[0x90; 10],
        &[0x00; 10],
    ),
];
/// The last spell type the three tables have a case byte for.
const LAST_SPELL_TYPE: u8 = 0x33;
/// Value 3's `CSpellEffect::UpdateLasting` bound (`cmp eax, 0x29`) and case bytes for 0x2A-0x2F (0 the countdown, 14
/// the default, 4 the fire wall), and `CSpell::GetDamageType`'s bound, which gives 0x2A the damage type 3 of the byte
/// after its table. The original's lasting bound is 0x32 with no case bytes for 0x30-0x32, which then dispatch through
/// the code after the table; 0x2F sends them to the default case instead.
const LASTING_TYPES: [Bytes; 3] = [
    (
        eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__5F,
        &[0x29],
        &[0x2F],
    ),
    (
        eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__F4A,
        &[0x90; 6],
        &[0x00, 0x00, 0x00, 0x0E, 0x0E, 0x04],
    ),
    (eix_sites::C_SPELL_GET_DAMAGE_TYPE__8, &[0x29], &[0x2A]),
];

/// The spell types with code of their own: 0x2B Inspiration (the Healing case's), 0x2C-0x30 (the InvokeLightning
/// case's, for scripts) and 0x31 (the Fireball case's).
const INSPIRATION: u32 = 0x2B;
const SCRIPT_SPELLS: core::ops::RangeInclusive<u32> = 0x2C..=0x30;
const SACRIFICE: u32 = 0x31;

/// A spell effect's fields.
mod effect {
    /// The target and the caster, objects or null.
    pub(super) const TARGET: u32 = 0x1C;
    pub(super) const CASTER: u32 = 0x20;
    /// The point, two floats.
    pub(super) const X: u32 = 0x24;
    pub(super) const Y: u32 = 0x28;
    /// The duration, an integer, and the power, a float.
    pub(super) const DURATION: u32 = 0x54;
    pub(super) const POWER: u32 = 0x58;
}

/// The spell type `CSpellEffect::Start` dispatches on: its argument `[ebp+8]`, which it never writes.
fn start_type(ctx: &Context) -> u32 {
    // SAFETY: at every site in `Start` EBP is its frame, whose +8 is the type it was called with.
    unsafe { read::<u32>(ctx.ebp.wrapping_add(8)) }
}

/// `CUnit::SetHostileMask` 0x52E970 and `CUnit::SetSide` 0x530120: thiscall on the unit, one word, `ret 4`.
const SET_HOSTILE_MASK: Site = eix_sites::C_UNIT_SET_HOSTILE_MASK;
const SET_SIDE: Site = eix_sites::C_UNIT_SET_SIDE;
type SetUnitWord = unsafe extern "thiscall" fn(unit: u32, value: i32);

/// Gives `unit` the hostile mask `mask`, then the side `side`.
///
/// # Safety
/// `unit` must be a live unit with a controller (`SetHostileMask` calls it), on the game's thread.
unsafe fn set_mask_and_side(unit: u32, mask: i32, side: i32) {
    // SAFETY: both are `void __thiscall (int)`.
    let set_mask: SetUnitWord = unsafe { hook::as_fn(SET_HOSTILE_MASK) };
    // SAFETY: as above.
    let set_side: SetUnitWord = unsafe { hook::as_fn(SET_SIDE) };
    // SAFETY: the caller guarantees the unit and the thread.
    unsafe { set_mask(unit, mask) };
    // SAFETY: as above.
    unsafe { set_side(unit, side) };
}

/// The effect's caster when it is a unit with a controller, which the mask and side calls need; the original calls
/// them on whatever the field holds.
///
/// # Safety
/// `effect` must be the live spell effect `CSpellEffect::Start` runs on, on the game's thread.
unsafe fn caster_unit(effect: u32) -> Option<u32> {
    // SAFETY: the caller guarantees the effect.
    let caster = unsafe { read::<u32>(effect + effect::CASTER) };
    if caster == 0 {
        return None;
    }
    let object = core::ptr::with_exposed_provenance_mut::<EngineObject>(caster as usize);
    // SAFETY: a non-null caster is a live engine object.
    if unsafe { EngineObject::class_id(object) } != CLASS_UNIT {
        return None;
    }
    // SAFETY: a unit's +0x240 is its controller or null.
    (unsafe { read::<u32>(caster + 0x240) } != 0).then_some(caster)
}

/// The hostile mask 0x2C-0x30 and 0x31 give their caster, and 0x31's side.
const CASTER_MARK: i32 = 29;

/// `fld dword ptr [edi+0x200]` at `CSpellEffect::Start`+0x205A, the caster's intelligence the Charm case (0x24, and
/// 0x2A through the same case byte) adds to the power, and the `fadd st(1)` that adds it (+0x206F).
const CHARM_INTELLIGENCE: Site = eix_sites::C_SPELL_EFFECT_START__205A;
const CHARM_INTELLIGENCE_BYTES: [u8; 6] = [0xD9, 0x87, 0x00, 0x02, 0x00, 0x00];
const CHARM_ADD: (Site, &[u8]) = (eix_sites::C_SPELL_EFFECT_START__206F, &[0xD8, 0xC1]);
/// `call CUnitServer::TryTame` at +0x2084 after `push power; push caster; mov ecx, esi` (ESI the target's
/// controller), and the case's two ways on: the Charm sound, then the effect ends (+0x20A3), or the caster's
/// acknowledgement 0x1A, then the effect ends (+0x20EC).
const CHARM_TAME: Site = eix_sites::C_SPELL_EFFECT_START__2084;
const CHARM_TAME_BYTES: [u8; 5] = [0xE8, 0xE7, 0x01, 0xED, 0xFF];
const CHARM_SUCCESS: Site = eix_sites::C_SPELL_EFFECT_START__20A3;
const CHARM_FAILURE: Site = eix_sites::C_SPELL_EFFECT_START__20EC;
/// `add esp, 8`: the two arguments pushed for `TryTame`.
const DROP_TAME_ARGS: [u8; 3] = [0x83, 0xC4, 0x08];
/// `UnitStats::AttributeWithItems` 0x525E90: thiscall on the stats at the controller's +0x1E0, the attribute's index
/// (2, the resistance to taming) and the unit, `ret 8`.
const ATTRIBUTE_WITH_ITEMS: Site = eix_sites::UNIT_STATS_ATTRIBUTE_WITH_ITEMS;
type AttributeWithItems = unsafe extern "thiscall" fn(stats: u32, index: i32, unit: u32) -> f32;
const CONTROLLER_STATS: u32 = 0x1E0;
const TAME_RESISTANCE: i32 = 2;

/// At the Charm case's intelligence load: a copy of the power (ST0) takes its place, so the case's two stores leave
/// the power alone at `[ebp-0x10]` and the x87 stack empty (the original's second store pops an empty stack).
extern "C" fn charm_power(_: usize, ctx: &mut FpuContext) {
    // The site's stack holds only the power `fld [ebx+0x58]` loaded, so the push has room.
    if let Some(power) = ctx.fpu.st(0) {
        ctx.fpu.push(power);
    }
}

/// Whether a Charm takes: the target's resistance below the power, as an ordered compare (NaN fails).
fn charmed(resistance: f32, power: f32) -> bool {
    Ext::new(f64::from(resistance)).fcom(f64::from(power)) == C0
}

/// In place of `TryTame`, its arguments dropped: a target whose resistance is below the power gets hostile mask 0 and
/// side 0 and the Charm sound; otherwise the case's failure way runs.
extern "C" fn charm(_: usize, ctx: &mut Context) {
    // SAFETY: EBX is the effect; the Charm case checked that its target is a unit.
    let target = unsafe { read::<u32>(ctx.ebx + effect::TARGET) };
    // SAFETY: the power the case stored in `Start`'s frame.
    let power = unsafe { read::<f32>(ctx.ebp.wrapping_sub(0x10)) };
    // SAFETY: 0x525E90 is `float __thiscall (int, CUnit*)`.
    let attribute: AttributeWithItems = unsafe { hook::as_fn(ATTRIBUTE_WITH_ITEMS) };
    // SAFETY: ESI is the target's controller, whose +0x1E0 is its stats; on the game's thread.
    let resistance = unsafe { attribute(ctx.esi + CONTROLLER_STATS, TAME_RESISTANCE, target) };
    ctx.resume = if charmed(resistance, power) {
        // SAFETY: the target is a unit whose controller ESI is.
        unsafe { set_mask_and_side(target, 0, 0) };
        CHARM_SUCCESS
    } else {
        CHARM_FAILURE
    };
}

/// `mov edx, [ecx]; call [edx+0x54]` at `CSpellEffect::Start`+0x1DE0: the Healing case's heal of the target's
/// controller (ECX) by the amount pushed from EAX.
const HEAL_CALL: Site = eix_sites::C_SPELL_EFFECT_START__1DE0;
const HEAL_CALL_BYTES: [u8; 5] = [0x8B, 0x11, 0xFF, 0x52, 0x54];

/// Inspiration's mana: `mana + amount`, or the maximum when that sum is greater (no lower bound).
fn inspired_mana(mana: f32, max: f32, amount: f32) -> f32 {
    let sum = Ext::new(f64::from(amount)).fadd(f64::from(mana));
    if sum.fcom(f64::from(max)) == 0 {
        max
    } else {
        sum.to_f32()
    }
}

/// Before the heal, Inspiration adds the amount to the target's mana ([`inspired_mana`]) and heals by 0; Healing heals
/// as the game's.
extern "C" fn inspiration_mana(_: usize, ctx: &mut Context) {
    if start_type(ctx) != INSPIRATION {
        return;
    }
    // SAFETY: ECX is the target unit's controller, whose +0x1C is the unit.
    let unit = unsafe { read::<u32>(ctx.ecx + 0x1C) };
    // SAFETY: the unit's vitals pointer.
    let at = unsafe { read::<u32>(unit + VITALS) } + MANA;
    // SAFETY: the vitals' mana.
    let mana = unsafe { read::<f32>(at) };
    // SAFETY: its maximum, after it.
    let max = unsafe { read::<f32>(at + 4) };
    // SAFETY: as above.
    unsafe { write::<f32>(at, inspired_mana(mana, max, f32::from_bits(ctx.eax))) };
    // The pushed amount, which the heal takes.
    ctx.set_stack_word(0, 0);
}

/// `lea ecx, [ebp-0x1D4]` at +0x1EB7, before the Healing particle's arguments are pushed (EDI the target, EDX the
/// size), and the call's setup after the pushes (+0x1ED0).
const HEAL_PARTICLE: Site = eix_sites::C_SPELL_EFFECT_START__1EB7;
const HEAL_PARTICLE_BYTES: [u8; 6] = [0x8D, 0x8D, 0x2C, 0xFE, 0xFF, 0xFF];
const PARTICLE_CALL: Site = eix_sites::C_SPELL_EFFECT_START__1ED0;
/// Inspiration's particle; Healing's is 0x2006.
const INSPIRATION_PARTICLE: u32 = 0x2018;

/// Inspiration's particle with the game's other arguments. The original also writes `size * 2.5 + 2.5` over the size
/// local `[ebp-0x2C]` after it was pushed; the case stores that local again before it reads it, so that is left out.
extern "C" fn inspiration_particle(arg: usize, ctx: &mut Context) {
    ctx.ecx = ctx.ebp.wrapping_sub(0x1D4);
    if start_type(ctx) != INSPIRATION {
        return;
    }
    // SAFETY: the build kept the tail at `arg`.
    let tail = unsafe { common::kept::<&'static PushTail>(arg) };
    ctx.eax = ctx.ebp.wrapping_sub(0x6C);
    let args = [
        ctx.edi,
        ctx.ecx,
        1.0f32.to_bits(),
        ctx.edx,
        ctx.eax,
        INSPIRATION_PARTICLE,
        0,
    ];
    tail.resume(ctx, &args);
}

/// `push 50.0` at +0x1F10, the Healing sound's hearing distance (EAX the target), and the call's setup after the
/// sound's name is pushed (+0x1F2A).
const HEAL_SOUND: Site = eix_sites::C_SPELL_EFFECT_START__1F10;
const HEAL_SOUND_BYTES: [u8; 5] = [0x68, 0x00, 0x00, 0x48, 0x42];
const SOUND_CALL: Site = eix_sites::C_SPELL_EFFECT_START__1F2A;
/// Inspiration's sound and its volume, Healing's.
static INSPIRATION_SOUND: &CStr = c"magic\\Inspiration\\start.mp3";
const SOUND_VOLUME: f32 = 10.0;

/// After the hearing distance, Inspiration's sound with the game's other arguments.
extern "C" fn inspiration_sound(arg: usize, ctx: &mut Context) {
    if start_type(ctx) != INSPIRATION {
        return;
    }
    // SAFETY: the build kept the tail at `arg`.
    let tail = unsafe { common::kept::<&'static PushTail>(arg) };
    ctx.edx = ctx.ebp.wrapping_sub(0x1EC);
    ctx.eax = ctx.eax.wrapping_add(0x1C);
    let args = [
        SOUND_VOLUME.to_bits(),
        ctx.edx,
        ctx.eax,
        INSPIRATION_SOUND.as_ptr().expose_provenance() as u32,
    ];
    tail.resume(ctx, &args);
}

/// `mov eax, [ebx+0x28]; mov ecx, [ebx+8]` at +0x8B4, the start of the InvokeLightning case, and +0x1BE1, where the
/// effect ends.
const LIGHTNING_START: Site = eix_sites::C_SPELL_EFFECT_START__8B4;
const LIGHTNING_START_BYTES: [u8; 6] = [0x8B, 0x43, 0x28, 0x8B, 0x4B, 0x08];
const EFFECT_ENDS: Site = eix_sites::C_SPELL_EFFECT_START__1BE1;
/// The player's tick `CWorldServer::UpdatePlayerTimes` counts its time from.
const PLAYER_TICK: u32 = 0x110;

/// The variables types 0x2C-0x30 publish, in the original's order: the effect's point, its power, its type (the low
/// 16 bits, signed) and its duration.
fn script_variables(
    x: f32,
    y: f32,
    power: f32,
    kind: u32,
    duration: i32,
) -> [(&'static CStr, f32); 5] {
    [
        (c"S3x", x),
        (c"S4y", y),
        (c"S0e", power),
        (c"S2t", f32::from(kind as u16 as i16)),
        (c"S1d", Ext::new(f64::from(duration)).to_f32()),
    ]
}

/// The side types 0x2C-0x30 give their caster: `fistp` of `power * 32.0` under the rounding mode in effect.
fn caster_side(power: f32) -> i32 {
    Ext::new(f64::from(power)).fmul(32.0).fistp()
}

/// The first player of the server world's list gets the world's tick as its [`PLAYER_TICK`], then `variables` through
/// its server half; nothing without a player.
///
/// # Safety
/// On the game's thread, with the server world constructed.
unsafe fn publish(variables: &[(&CStr, f32)]) {
    let world = SERVER_WORLD.as_ptr();
    let mut players = PlayerIterator::new();
    // SAFETY: the caller guarantees the world and the thread.
    let list = unsafe { World::players(world) };
    // SAFETY: the world's player manager is a live list.
    let player = unsafe { players.first(list) };
    if !player.is_null() {
        // SAFETY: the constructed world, as above.
        let tick = unsafe { (*world).tick };
        // SAFETY: a live player, whose +0x110 is a tick the game's own code writes.
        unsafe { write::<i32>(player.expose_provenance() as u32 + PLAYER_TICK, tick) };
        // SAFETY: the player's server half or null.
        let server = unsafe { (*player).server };
        if !server.is_null() {
            for &(name, value) in variables {
                // SAFETY: a live server half; the names are NUL-terminated.
                unsafe { PlayerServer::set_variable(server, name, value) };
            }
        }
    }
    // SAFETY: the walk `first` started.
    unsafe { players.release() };
}

/// At the InvokeLightning case: types 0x2C-0x30 publish their variables, give their caster hostile mask 29 and the
/// side [`caster_side`], and end; the other types load what the replaced instructions load.
extern "C" fn script_spell(_: usize, ctx: &mut Context) {
    let effect = ctx.ebx;
    let field = |offset: u32| {
        // SAFETY: EBX is the live spell effect `Start` runs on.
        unsafe { read::<u32>(effect + offset) }
    };
    if !SCRIPT_SPELLS.contains(&ctx.esi) {
        ctx.eax = field(effect::Y);
        ctx.ecx = field(8);
        return;
    }
    let power = f32::from_bits(field(effect::POWER));
    let variables = script_variables(
        f32::from_bits(field(effect::X)),
        f32::from_bits(field(effect::Y)),
        power,
        ctx.esi,
        field(effect::DURATION) as i32,
    );
    // SAFETY: a spell effect runs on the game's thread in the server world.
    unsafe { publish(&variables) };
    // SAFETY: the effect, as above.
    if let Some(caster) = unsafe { caster_unit(effect) } {
        // SAFETY: a live unit with a controller.
        unsafe { set_mask_and_side(caster, CASTER_MARK, caster_side(power)) };
    }
    ctx.resume = EFFECT_ENDS;
}

/// `fld [ebx+0x64]; lea esi, [ebx+0x24]` at +0x63D, the start of the Fireball case, which 0x31 reaches.
const FIREBALL_START: Site = eix_sites::C_SPELL_EFFECT_START__63D;
const FIREBALL_START_BYTES: [u8; 6] = [0xD9, 0x43, 0x64, 0x8D, 0x73, 0x24];

/// Before the Fireball case runs for 0x31, its caster gets hostile mask 29 and side 29.
extern "C" fn sacrifice_caster(_: usize, ctx: &mut Context) {
    if ctx.esi != SACRIFICE {
        return;
    }
    // SAFETY: EBX is the live spell effect `Start` runs on.
    if let Some(caster) = unsafe { caster_unit(ctx.ebx) } {
        // SAFETY: a live unit with a controller.
        unsafe { set_mask_and_side(caster, CASTER_MARK, CASTER_MARK) };
    }
}

/// `newspells` 1-3: spell types 0x2A-0x33 get cases in `Start`, `UpdateMissile` and `GetDamage`; Charm and Ensorcell
/// (0x2A) meet the target's resistance; Inspiration (0x2B) gives mana; 0x2C-0x30 publish variables for scripts. 1 also
/// marks 0x31's caster, 1 and 2 load the AutoRun mob, 3 adds lasting cases and Ensorcell's damage type.
pub(crate) fn new_spells(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if !matches!(value, 1..=3) {
        return Ok(Vec::new());
    }
    let mut writes = common::fixed(&SPELL_TYPES)?;
    if value != 3 {
        writes.push(super::script::auto_run_mob()?);
    }
    // The original's layout at each site: `nop; jmp <added code>` or `jmp <added code>`.
    if value == 1 {
        let resume = FIREBALL_START.offset(FIREBALL_START_BYTES.len());
        let stub = hook::stub(
            FIREBALL_START,
            &[],
            &FIREBALL_START_BYTES,
            false,
            sacrifice_caster,
            0,
            resume,
        )?;
        writes.push(common::branch(
            FIREBALL_START,
            &FIREBALL_START_BYTES,
            &[0x90, 0xE9, 0, 0, 0, 0],
            1,
            stub,
        )?);
    }
    writes.extend(common::jump_at(
        LIGHTNING_START,
        &LIGHTNING_START_BYTES,
        1,
        script_spell,
        0,
    )?);
    writes.push(hook::context_hook(
        HEAL_CALL,
        &HEAL_CALL_BYTES,
        Mode::After,
        inspiration_mana,
        0,
    )?);
    let particle = common::keep(PushTail::new(7, PARTICLE_CALL)?);
    writes.extend(common::jump_at(
        HEAL_PARTICLE,
        &HEAL_PARTICLE_BYTES,
        1,
        inspiration_particle,
        particle,
    )?);
    let sound = common::keep(PushTail::new(4, SOUND_CALL)?);
    writes.push(hook::context_hook(
        HEAL_SOUND,
        &HEAL_SOUND_BYTES,
        Mode::After,
        inspiration_sound,
        sound,
    )?);
    let load = &CHARM_INTELLIGENCE_BYTES[1..];
    writes.push(SiteWrite::nop(
        CHARM_INTELLIGENCE,
        &CHARM_INTELLIGENCE_BYTES[..1],
    ));
    writes.push(hook::fpu_hook(
        CHARM_INTELLIGENCE.offset(1),
        load,
        load,
        Mode::Drop,
        charm_power,
        0,
    )?);
    writes.push(SiteWrite::nop(CHARM_ADD.0, CHARM_ADD.1));
    let tame = common::jump_stub(CHARM_TAME, &DROP_TAME_ARGS, charm, 0, CHARM_FAILURE)?;
    writes.push(common::branch(
        CHARM_TAME,
        &CHARM_TAME_BYTES,
        &[0xE9, 0, 0, 0, 0],
        0,
        tame,
    )?);
    if value == 3 {
        writes.extend(common::fixed(&LASTING_TYPES)?);
    }
    Ok(writes)
}

#[cfg(test)]
mod tests {
    #[test]
    fn damage_displacements_stay_in_the_record_and_keep_aliases() {
        for (input, expected) in [
            ("0", Some(0)),
            ("14", Some(56)),
            ("15", None),
            ("64", Some(0)),
            ("255", None),
        ] {
            let values = values(&[("gameplay", "lightningbeam", input)]);
            let ctx = Ctx::new(&values, no_log);
            assert_eq!(damage_slot(&ctx, "lightningbeam").ok(), expected, "{input}");
        }
    }

    use super::super::testing::{Original, assert_original, no_log, plain, values};
    use super::*;

    /// The bytes of game.exe's code and constants the original changes for each INI, every other key at its
    /// built-in default.
    // One row per INI: rustfmt would spread each over dozens of lines.
    #[rustfmt::skip]
    const ORIGINAL: &[Original] = &[
        ("gameplay", "possessioncharmlink", 1, &[], &[(eix_sites::C_WORLD_SERVER_CAST_SPELL__50, "90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90")]),
        ("gameplay", "possessioncharmlink", 1, &[("gameplay", "damageboost", "5")], &[(eix_sites::C_UNIT_SERVER_FINISH_CAST__80, "90 E8 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_FINISH_CAST__1E5, "E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__191, "89 4B 2C EB 7C D9 5B 24"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__21B, "E9 ?? ?? ?? ??"), (eix_sites::C_WORLD_SERVER_CAST_SPELL__50, "90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 8B 4F 3C 8B 5F 38 81 E3 FF FF FF 00"), (eix_sites::C_SPELL_EFFECT_START__92, "90 90 E9 ?? ?? ?? ??")]),
        ("gameplay", "splashdamageonair", 1, &[], &[(eix_sites::IS_SPELL_TARGET__1D, "90 90 90 90 90"), (eix_sites::C_SPELL_EFFECT_HIT_UNITS_IN_RADIUS__A5, "90 90")]),
        ("gameplay", "splashdamageonair", 2, &[], &[(eix_sites::AI_ATTENTION_CAN_REACH__93, "E9 ?? ?? ?? ?? 90 90"), (eix_sites::IS_SPELL_TARGET__9, "1E"), (eix_sites::IS_SPELL_TARGET__1E, "09 83 7D 08 04 74 03 33 C0"), (eix_sites::IS_SPELL_TARGET__28, "B8 01 00 00 00 C3")]),
        ("gameplay", "offensivespells", 1, &[], &[(eix_sites::C_WORLD_SERVER_CREATE_MISSILE__1C0, "01"), (eix_sites::C_WORLD_SERVER_CREATE_MISSILE__1D3, "01")]),
        ("gameplay", "shrinkenlargerebalance", 1, &[], &[(eix_sites::C_MAP_OBJECT_ADD_EFFECT_IMPL__E0, "01 01"), (eix_sites::RECOMPUTE_SCALE_HOOK__13A, "90 E9"), (eix_sites::RECOMPUTE_SCALE_HOOK__220, "50"), (eix_sites::C_MAP_OBJECT_SERVER_UPDATE__84, "90 90 90 90 90 90 90 90 90 90"), (eix_sites::C_SPELL_IS_OFFENSIVE__4E, "01")]),
        ("gameplay", "shrinkenlargerebalance", 2, &[], &[(eix_sites::C_MAP_OBJECT_ADD_EFFECT_IMPL__E0, "01 01"), (eix_sites::RECOMPUTE_SCALE_HOOK__13A, "90 E9"), (eix_sites::C_MAP_OBJECT_SERVER_UPDATE__84, "90 90 90 90 90 90 90 90 90 90"), (eix_sites::C_SPELL_IS_OFFENSIVE__4E, "01")]),
        ("gameplay", "regenerationremaster", 1, &[], &[(eix_sites::UNIT_VITALS_REGENERATE__17F, "09"), (eix_sites::UNIT_VITALS_REGENERATE__187, "EB 06 D9 05 58 B8 73 00 8B 46 0C 8B 7D 08 89 45 0C 51 D9 45 0C D8 4D 10 DE C1 D8 0D 18 39 52 00 EB 05 90 90 90 90 90"), (eix_sites::UNIT_VITALS_REGENERATE__1D8, "CD CC CC 3D")]),
        ("gameplay", "strengthweakrebalance", 1, &[], &[(eix_sites::UNIT_STATS_RECOMPUTE__4F6, "D9 85 B4 FF FF FF D8 63 B0"), (eix_sites::UNIT_STATS_RECOMPUTE__501, "98 4F 52"), (eix_sites::UNIT_STATS_RECOMPUTE__505, "D8 43 10 D9 C0 D8 73 10 D9 C9"), (eix_sites::UNIT_STATS_RECOMPUTE__510, "5B 10 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90"), (eix_sites::UNIT_STATS_RESET_TO_BASE__2C8, "CD CC CC 3D")]),
        ("gameplay", "tornadochanges", 1, &[], &[]),
        ("gameplay", "tornadochanges", 2, &[], &[(eix_sites::C_EFFECT_TORNADO_TICK__180, "90 90")]),
        ("gameplay", "filtersarealwaysusing", 1, &[], &[(eix_sites::C_SPELL_EFFECT_HIT_UNIT__B, "90 90 90 90")]),
        ("gameplay", "teleportboost", 1, &[], &[(eix_sites::C_SPELL_EFFECT_START__1DA9, "05 90 90 E9 ?? ?? ?? ??")]),
        ("gameplay", "teleportboost", 2, &[], &[(eix_sites::C_SPELL_EFFECT_START__1DA9, "08")]),
        ("gameplay", "teleportboost", 3, &[], &[(eix_sites::C_SPELL_EFFECT_START__1DA9, "01")]),
        ("gameplay", "allowcastsonbriefings", 1, &[], &[(eix_sites::C_WORLD_SERVER_CAST_SPELL__3D, "EB")]),
        ("speed", "spellsarealwaysconstructable", 1, &[], &[(eix_sites::CAN_DROP_ITEM__CE, "90 90 90 90 90 90"), (eix_sites::CAN_DROP_ITEM__102, "B0 01 90"), (eix_sites::CAN_DROP_ITEM__263, "90 90 90 90 90 90"), (eix_sites::CAN_DROP_ITEM__274, "B0 01 90"), (eix_sites::CAN_DROP_ITEM__491, "90 90 90 90 90 90"), (eix_sites::CAN_DROP_ITEM__4A6, "B0 01 90"), (eix_sites::C_CAMP_PAGES_CAN_MAKE_SPELL__253, "B0 01"), (eix_sites::C_CAMP_PAGES_CAN_MAKE_SPELL__266, "B0 01 90")]),
        ("speed", "spellsaimod", 1, &[], &[(eix_sites::AI_ATTENTION_SPELL_KIND__13, "36"), (eix_sites::C_SPELL_IS_OFFENSIVE__8, "36"), (eix_sites::C_SPELL_IS_OFFENSIVE__F, "00 04 40")]),
        ("gameplay", "npcsmana", 1, &[], &[(eix_sites::C_UNIT_SERVER_FINISH_CAST__F9, "90 90 90 90 90 90")]),
        ("gameplay", "npcsmana", 2, &[], &[(eix_sites::C_UNIT_SERVER_CAN_CAST__A6, "90 90")]),
        ("gameplay", "npcsmana", 3, &[], &[(eix_sites::C_UNIT_SERVER_CAN_CAST__A6, "90 90"), (eix_sites::C_UNIT_SERVER_FINISH_CAST__F9, "90 90 90 90 90 90")]),
        ("gameplay", "npcsmana", 4, &[], &[(eix_sites::C_UNIT_SERVER_CAN_CAST__9D, "86 8C 02 00 00 90 83 F8 10"), (eix_sites::C_UNIT_SERVER_FINISH_CAST__F0, "86 8C 02 00 00 90 83 F8 10")]),
        ("gameplay", "nonstopmanaregen", 1, &[], &[(eix_sites::UNIT_VITALS_REGENERATE__34, "90 90 90 90 90 90")]),
        ("gameplay", "hidefireworks", 1, &[], &[(eix_sites::C_SPELL_EFFECT_START__1824, "EB 21 90")]),
        ("gameplay", "changesounds", 1, &[], &[]),
        ("gameplay", "wallsfrequency", 1, &[], &[(eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__2CD, "00"), (eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__46B, "00"), (eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__5D5, "00"), (eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__687, "00")]),
        ("gameplay", "wallspercentage", 1, &[], &[(eix_sites::C_SPELL_EFFECT_GET_DAMAGE__9F, "90 E8 ?? ?? ?? ?? 3E C3 2E 3A 3E C3 2E 3A 3E C3 2E 3A")]),
        ("gameplay", "wallspercentage", 2, &[], &[(eix_sites::C_SPELL_EFFECT_GET_DAMAGE__9B, "90 90 90 90 90 E8 ?? ?? ?? ?? CD CC CC 3D CD CC CC"), (eix_sites::C_SPELL_EFFECT_GET_DAMAGE__AD, "CD CC CC 3D")]),
        ("gameplay", "lastspellsdamagetype", 1, &[], &[(eix_sites::C_SPELL_EFFECT_GET_DAMAGE__50, "90 E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_GET_DAMAGE__63, "90 E9 ?? ?? ?? ??")]),
        ("gameplay", "possessionpatch", 1, &[], &[(eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__DA8, "20"), (eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__DC3, "AC"), (eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__DD5, "90"), (eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__DDF, "90 E9 ?? ?? ?? ??")]),
        ("gameplay", "speedslowremaster", 1, &[], &[(eix_sites::UNIT_STATS_RATE_VALUE__AC, "CD CC CC 3D"), (eix_sites::UNIT_STATS_RECOMPUTE__36B, "E9 ?? ?? ?? ?? 90 90 90 90")]),
        ("gameplay", "speedslowremaster", 2, &[], &[(eix_sites::UNIT_STATS_RATE_VALUE__AC, "CD CC CC 3D"), (eix_sites::UNIT_STATS_RECOMPUTE__36B, "E9 ?? ?? ?? ?? 90 90 90 90")]),
        ("gameplay", "batterycharge", 1, &[], &[(eix_sites::CAN_DROP_ITEM__790, "E9 ?? ?? ?? ?? 90 90 90 90")]),
        ("gameplay", "inspirationintelligence", 1, &[], &[(eix_sites::C_UNIT_SERVER_TICK_ITEM_SPELLS__60, "07 E9 ?? ?? ?? ?? EB 19 3D 00 00 2B 00 75 12 E9 ?? ?? ?? ?? 90"), (eix_sites::C_UNIT_SERVER_TICK_ITEM_SPELLS__76, "90 90 90 90 90 90 90 90 90 90 90")]),
        ("gameplay", "linkremaster", 1, &[], &[(eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__832, "90 E9 ?? ?? ?? ??")]),
        ("gameplay", "fakesrcrune", 1, &[], &[(eix_sites::C_WORLD_SERVER_CREATE_MISSILE__75, "90 E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__59, "90 90 90 E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_CONTAINER_BUILD__2A5, "E9 ?? ?? ?? ??")]),
        ("gameplay", "controlmagic", 1, &[], &[(eix_sites::C_SPELL_EFFECT_START__1639, "90 E9 ?? ?? ?? ??")]),
    ("gameplay", "controlmagic", 2, &[], &[(eix_sites::C_TRANSFORM_OBJECT_SAVE_TRANSFORM_IMPL__28, "CD CC 4C BF"), (eix_sites::CHUNK_ARCHIVE_LEAVE_CHUNK__B8, "8F C2 F5 3C"), (eix_sites::C_SPELL_EFFECT_START__1639, "90 E8 ?? ?? ?? ??")]),
    ("gameplay", "controlmagic", 3, &[("gameplay", "prot_fire", "1"), ("gameplay", "bstun", "2.5")], &[(eix_sites::C_TRANSFORM_OBJECT_SAVE_TRANSFORM_IMPL__28, "CD CC 4C BF"), (eix_sites::CHUNK_ARCHIVE_LEAVE_CHUNK__B8, "8F C2 F5 3C"), (eix_sites::C_SPELL_EFFECT_START__1639, "90 E8 ?? ?? ?? ??")]),
    ("gameplay", "controlmagic", 5, &[], &[(eix_sites::C_TRANSFORM_OBJECT_SAVE_TRANSFORM_IMPL__28, "CD CC 4C BF"), (eix_sites::CHUNK_ARCHIVE_LEAVE_CHUNK__B8, "8F C2 F5 3C"), (eix_sites::C_SPELL_EFFECT_START__1639, "90 E8 ?? ?? ?? ??")]),
    ("gameplay", "controlmagic", 7, &[("gameplay", "lvldurcoeff", "0.5")], &[(eix_sites::C_TRANSFORM_OBJECT_SAVE_TRANSFORM_IMPL__28, "CD CC 4C BF"), (eix_sites::CHUNK_ARCHIVE_LEAVE_CHUNK__B8, "00 00 00 3F"), (eix_sites::C_SPELL_EFFECT_START__1639, "90 E8 ?? ?? ?? ??")]),
        // newspells' type bounds (0x67F8F3, 0x681BC0, 0x68398E, 0x6826AF) differ from the original's on purpose; the
        // test `new_spell_types_end_at_their_tables` pins ours.
        ("gameplay", "newspells", 1, &[], &[(eix_sites::C_WORLD_SERVER_CREATE_UNITS__B0C, "E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__83, "??"), (eix_sites::C_SPELL_EFFECT_START__63D, "90 E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__8B4, "90 E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__1DE0, "E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__1EB7, "90 E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__1F10, "E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__205A, "90 E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__206F, "90 90"), (eix_sites::C_SPELL_EFFECT_START__2084, "E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__2306, "11 0E 04 04 04 04 04 03 04 05"), (eix_sites::C_SPELL_EFFECT_UPDATE_MISSILE__10, "??"), (eix_sites::C_SPELL_EFFECT_UPDATE_MISSILE__A95, "03 03 03 03 03 03 02 02 02 02 02"), (eix_sites::C_SPELL_EFFECT_GET_DAMAGE__3E, "??"), (eix_sites::C_SPELL_EFFECT_GET_DAMAGE__106, "00 00 00 00 00 00 00 00 00 00")]),
        ("gameplay", "newspells", 2, &[], &[(eix_sites::C_WORLD_SERVER_CREATE_UNITS__B0C, "E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__83, "??"), (eix_sites::C_SPELL_EFFECT_START__8B4, "90 E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__1DE0, "E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__1EB7, "90 E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__1F10, "E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__205A, "90 E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__206F, "90 90"), (eix_sites::C_SPELL_EFFECT_START__2084, "E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__2306, "11 0E 04 04 04 04 04 03 04 05"), (eix_sites::C_SPELL_EFFECT_UPDATE_MISSILE__10, "??"), (eix_sites::C_SPELL_EFFECT_UPDATE_MISSILE__A95, "03 03 03 03 03 03 02 02 02 02 02"), (eix_sites::C_SPELL_EFFECT_GET_DAMAGE__3E, "??"), (eix_sites::C_SPELL_EFFECT_GET_DAMAGE__106, "00 00 00 00 00 00 00 00 00 00")]),
        ("gameplay", "newspells", 3, &[], &[(eix_sites::C_SPELL_EFFECT_START__83, "??"), (eix_sites::C_SPELL_EFFECT_START__8B4, "90 E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__1DE0, "E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__1EB7, "90 E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__1F10, "E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__205A, "90 E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__206F, "90 90"), (eix_sites::C_SPELL_EFFECT_START__2084, "E9 ?? ?? ?? ??"), (eix_sites::C_SPELL_EFFECT_START__2306, "11 0E 04 04 04 04 04 03 04 05"), (eix_sites::C_SPELL_EFFECT_UPDATE_MISSILE__10, "??"), (eix_sites::C_SPELL_EFFECT_UPDATE_MISSILE__A95, "03 03 03 03 03 03 02 02 02 02 02"), (eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__5F, "??"), (eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__F4A, "00 00 00 0E 0E 04"), (eix_sites::C_SPELL_EFFECT_GET_DAMAGE__3E, "??"), (eix_sites::C_SPELL_EFFECT_GET_DAMAGE__106, "00 00 00 00 00 00 00 00 00 00"), (eix_sites::C_SPELL_GET_DAMAGE_TYPE__8, "2A")]),
    ];

    #[test]
    fn the_sites_match_the_original() {
        assert_original(ORIGINAL);
    }

    #[test]
    fn the_registry_skips_the_defective_values() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        let skips = |key: &str, value: i8| {
            let f = crate::registry::FEATURES
                .iter()
                .find(|f| f.key == key)
                .unwrap();
            crate::registry::skipped(f, value, &v).is_some()
        };
        assert!(skips("vampirism", 1));
        assert!(skips("inspirationintelligence", 2) && skips("inspirationintelligence", 3));
        assert!(!skips("inspirationintelligence", 1));
        assert!(skips("linkremaster", 2) && skips("linkremaster", 3) && !skips("linkremaster", 1));
        assert!(skips("controlmagic", 4) && skips("controlmagic", 6) && !skips("controlmagic", 5));
        assert!(link_remaster(&ctx, 4).unwrap().is_empty());
    }

    #[test]
    fn moved_code_keeps_its_calls_targets() {
        // call +0x10; ret; then two NOPs of room.
        let code = [0xE8, 0x10, 0, 0, 0, 0xC3, 0x90, 0x90];
        assert_eq!(
            shift_up(&code, 2, &[1]).unwrap(),
            [0x90, 0x90, 0xE8, 0x0E, 0, 0, 0, 0xC3]
        );
        assert!(shift_up(&code, 3, &[1]).is_err());
        assert_eq!(fnv1a(b""), 0xCBF2_9CE4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xAF63_DC4C_8601_EC8C);
    }

    #[test]
    fn new_spell_types_end_at_their_tables() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        for value in 1..=3 {
            let writes = plain(&new_spells(&ctx, value).unwrap());
            let at = |site: Site| writes.iter().find(|w| w.0 == site).map(|w| w.2.clone());
            for site in [
                eix_sites::C_SPELL_EFFECT_START__83,
                eix_sites::C_SPELL_EFFECT_UPDATE_MISSILE__10,
                eix_sites::C_SPELL_EFFECT_GET_DAMAGE__3E,
            ] {
                assert_eq!(at(site), Some(vec![LAST_SPELL_TYPE]), "{value}: {site:#x}");
            }
            let lasting = (value == 3).then(|| vec![0x2F]);
            assert_eq!(
                at(eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__5F),
                lasting,
                "{value}"
            );
        }
        assert!(new_spells(&ctx, 4).unwrap().is_empty());
        assert!(new_spells(&ctx, 0).unwrap().is_empty());
    }

    #[test]
    fn charm_takes_below_the_power_only() {
        assert!(charmed(10.0, 20.0));
        assert!(!charmed(20.0, 20.0));
        assert!(!charmed(30.0, 20.0));
        assert!(!charmed(f32::NAN, 20.0));
        assert!(!charmed(10.0, f32::NAN));
    }

    #[test]
    fn inspiration_caps_above_the_maximum_only() {
        assert_eq!(inspired_mana(90.0, 100.0, 20.0), 100.0);
        assert_eq!(inspired_mana(90.0, 100.0, 5.0), 95.0);
        assert_eq!(inspired_mana(80.0, 100.0, 20.0), 100.0);
        // No lower bound, and an unordered compare writes the sum.
        assert_eq!(inspired_mana(90.0, 100.0, -100.0), -10.0);
        assert_eq!(inspired_mana(90.0, f32::NAN, 20.0), 110.0);
        assert!(inspired_mana(90.0, 100.0, f32::NAN).is_nan());
    }

    #[test]
    fn script_spells_publish_their_effect() {
        assert_eq!(
            script_variables(12.5, 25.5, 20.0, 44, 150),
            [
                (c"S3x", 12.5),
                (c"S4y", 25.5),
                (c"S0e", 20.0),
                (c"S2t", 44.0),
                (c"S1d", 150.0)
            ]
        );
        // The type's low 16 bits, signed.
        assert_eq!(script_variables(0.0, 0.0, 0.0, 0x1_002D, 0)[3].1, 45.0);
        assert_eq!(
            script_variables(0.0, 0.0, 0.0, 0xFFFF, -3)[3..],
            [(c"S2t", -1.0), (c"S1d", -3.0)]
        );
        assert_eq!(caster_side(20.0), 640);
        assert_eq!(caster_side(0.4), 13);
        assert_eq!(caster_side(-1.0), -32);
        assert_eq!(caster_side(f32::MAX), i32::MIN);
    }

    #[test]
    fn control_magic_3_takes_the_odd_switches() {
        let v = values(&[
            ("gameplay", "prot_fire", "1"),
            ("gameplay", "prot_electro", "2"),
            ("gameplay", "stun", "7"),
            ("gameplay", "bstun", "2.5"),
        ]);
        let ctx = Ctx::new(&v, no_log);
        let three = bonuses(&ctx, 3).unwrap();
        assert_eq!(
            (three[0], three[1], three[10], three[21]),
            (Some(0.0), None, Some(2.5), None)
        );
        assert!(bonuses(&ctx, 5).unwrap().iter().all(Option::is_some));
    }
}
