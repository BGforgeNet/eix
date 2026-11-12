//! AI features. The original's added code is replaced by context hooks whose handlers compute what that code
//! computes, reading the parameters where the original keeps them.
#![expect(
    unsafe_code,
    reason = "the hooks read and write the game's units and AI state and call game functions"
)]

use eix_engine::game::unit::Unit;
use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, FpuContext, Mode};
use eix_engine::patch;
use eix_engine::x87::{C0, C3, Ext};
use eix_sites::Site;

use super::common::{self, Bytes, NOWHERE, read, write};
use super::{BuildError, Ctx};
use crate::types::Level;

/// `AIAttention::SpellKind`'s bound, the operand of `cmp eax, 0x29` at 0x5CEDD1: the number of spell types.
const SPELL_KIND_BOUND: Site = eix_sites::AI_ATTENTION_SPELL_KIND__13;
/// The number of entries the caster table gets: `[aicastertable] 0` to `53`.
const CASTER_ENTRIES: u8 = 0x36;
/// `mov cl, [eax + 0x5CEE4C]` at 0x5CEDD8, whose table address's low byte the feature changes (written whole, so
/// another build gets the whole address).
const SPELL_KIND_LOOKUP: Site = eix_sites::AI_ATTENTION_SPELL_KIND__1A.back(2);
const SPELL_KIND_LOOKUP_WAS: [u8; 6] = [0x8A, 0x88, 0x4C, 0xEE, 0x5C, 0x00];
/// Where the caster table is written: from the last entry of the jump table before the game's byte table, over that
/// table, into the padding after it.
const SPELL_KIND_TABLE: Site = eix_sites::AI_ATTENTION_SPELL_KIND__88;
const SPELL_KIND_TABLE_BYTES: [u8; CASTER_ENTRIES as usize] = [
    0x1D, 0xEE, 0x5C, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x02, 0x02, 0x02, 0x03, 0x03, 0x03,
    0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x05, 0x05, 0x08, 0x08, 0x08, 0x08, 0x06, 0x07, 0x03, 0x08,
    0x08, 0x03, 0x07, 0x04, 0x07, 0x04, 0x05, 0x08, 0x05, 0x05, 0x05, 0x05, 0x00, 0x00, 0x90, 0x90,
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
];
/// `CSpell::IsOffensive`'s byte table, one entry per spell type: `[aioffendtable] 0` to `37`.
const IS_OFFENSIVE_TABLE: Site = eix_sites::C_SPELL_IS_OFFENSIVE__28;
const IS_OFFENSIVE_TABLE_BYTES: [u8; 38] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x01, 0x01, 0x01, 0x01, 0x00, 0x01,
    0x00, 0x01, 0x00, 0x01, 0x00, 0x01,
];
/// The low byte of each `[<section>] 0` .. `<count - 1>`.
fn table(ctx: &Ctx<'_>, section: &str, count: usize) -> Result<Vec<u8>, BuildError> {
    (0..count)
        .map(|i| Ok(ctx.values.byte(section, &i.to_string())?))
        .collect()
}

/// `rebuildedaicastertable` = 1: `SpellKind` maps 54 spell types through the INI's table, which starts 4 bytes
/// before the game's.
pub(crate) fn caster_table(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let mut lookup = SPELL_KIND_LOOKUP_WAS;
    lookup[2] = SPELL_KIND_TABLE.ru() as u8;
    Ok(vec![
        SiteWrite::bytes(SPELL_KIND_BOUND, &[0x29], &[CASTER_ENTRIES])?,
        SiteWrite::bytes(SPELL_KIND_LOOKUP, &SPELL_KIND_LOOKUP_WAS, &lookup)?,
        SiteWrite::bytes(
            SPELL_KIND_TABLE,
            &SPELL_KIND_TABLE_BYTES,
            &table(ctx, "aicastertable", CASTER_ENTRIES.into())?,
        )?,
    ])
}

/// `newbigtables` = 1 is in the registry's skip list, and the tables above ignore it: the original moves the caster
/// lookup past the table it writes and raises `IsOffensive`'s bound past its 39 entries, so spell types from 39 on
/// read code bytes as jump-table slots.
pub(crate) fn new_big_tables(_: &Ctx<'_>, _: i8) -> Result<Vec<SiteWrite>, BuildError> {
    Ok(Vec::new())
}

/// `rebuildedaioffendtable` = 1: `IsOffensive`'s table comes from the INI (0 harmful, 1 friendly).
pub(crate) fn offend_table(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![SiteWrite::bytes(
        IS_OFFENSIVE_TABLE,
        &IS_OFFENSIVE_TABLE_BYTES,
        &table(ctx, "aioffendtable", IS_OFFENSIVE_TABLE_BYTES.len())?,
    )?])
}

/// `je 0x5CAE03` at `AIViewer::Update`+0x2A0, the "lost from sight" jump after the radius test.
const VIEWER_RADIUS_JE: (Site, &[u8]) = (
    eix_sites::AI_VIEWER_UPDATE__2A0,
    &[0x0F, 0x84, 0x5D, 0x01, 0x00, 0x00],
);
/// `je 0x5CAE03` at `AIViewer::Update`+0x2FC.
const VIEWER_SECOND_JE: (Site, &[u8]) = (
    eix_sites::AI_VIEWER_UPDATE__2FC,
    &[0x0F, 0x84, 0x01, 0x01, 0x00, 0x00],
);

/// `visionfix` = 1: both "lost from sight" jumps of `AIViewer::Update` go; with `aggrmod` above 5 only the first,
/// which those `aggrmod` values remove as well.
pub(crate) fn vision_fix(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    if ctx.values.flag("gameplay", "aggrmod")? > 5 {
        Ok(common::nops(&[VIEWER_RADIUS_JE]))
    } else {
        Ok(common::nops(&[VIEWER_SECOND_JE, VIEWER_RADIUS_JE]))
    }
}

/// `spellsprioritys` = 1: the AI target rating's seven constants (one `.rdata` row from 0x73FF04) come from the INI.
pub(crate) fn spells_priorities(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    const ROW: [(&str, f32); 7] = [
        ("sametarget", 10.0),
        ("magicattack", 3.0),
        ("damagemult", 1.0),
        ("magicdamagemult", 1.3),
        ("savedamagemult", 3.0),
        ("rangecost", 25.0),
        ("kickprice", 2.0),
    ];
    if value != 1 {
        return Ok(Vec::new());
    }
    ROW.iter()
        .enumerate()
        .map(|(i, &(key, was))| common::float(ctx, key, eix_sites::RDATA_73FF04.offset(4 * i), was))
        .collect()
}

/// `otherspellcastingai` = 1: `AIAttention::IsUsable`+0x192 returns 1, so splash spells are cast with allies near.
pub(crate) fn other_spell_casting_ai(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![SiteWrite::bytes(
        eix_sites::AI_ATTENTION_IS_USABLE__192,
        &[0x32, 0xC0],
        &[0xB0, 0x01],
    )?])
}

/// `stupidenemies` = 1: `CUnitServer::WitnessMurder` drops its call of `CDiplomacy::OnAttack` with its arguments.
pub(crate) fn stupid_enemies(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(common::nops(&[(
        eix_sites::C_UNIT_SERVER_WITNESS_MURDER__16D,
        &[
            0x6A, 0x0A, 0x53, 0x55, 0x8D, 0x8F, 0x64, 0x2C, 0x01, 0x00, 0xE8, 0x34, 0x1C, 0xF0,
            0xFF,
        ],
    )]))
}

/// `fearpatch` = 1: the flee distance and the leg damage that ends fleeing come from the INI.
pub(crate) fn fear_patch(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![
        common::float(ctx, "fearsafetydist", eix_sites::RDATA_73FF00, 10.0)?,
        common::float(ctx, "legsdamagedmul", eix_sites::RDATA_73CDE0, 0.6)?,
    ])
}

/// The five `fcomp dword ptr [0x73CDB8]` (2.0) of `CPlayerServer::SendPartyState`: their operand addresses.
const PARTY_STATE_COMPARES: [Site; 5] = [
    eix_sites::C_PLAYER_SERVER_SEND_PARTY_STATE__7F,
    eix_sites::C_PLAYER_SERVER_SEND_PARTY_STATE__92,
    eix_sites::C_PLAYER_SERVER_SEND_PARTY_STATE__A5,
    eix_sites::C_PLAYER_SERVER_SEND_PARTY_STATE__B8,
    eix_sites::C_PLAYER_SERVER_SEND_PARTY_STATE__CB,
];
/// Padding after `SendPartyState` that takes `safetymuldist`.
const SAFETY_DISTANCE: Site = eix_sites::C_PLAYER_SERVER_SEND_PARTY_STATE__36C;

/// `newplayersafetyconditions` = 1: the party-state distance tests compare with `safetymuldist`, kept in padding,
/// and the visibility factor at 0x73FE90 becomes `maxvisibilitymul`.
pub(crate) fn player_safety(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let mut writes = PARTY_STATE_COMPARES
        .iter()
        .map(|&site| SiteWrite::pointer(site, eix_sites::RDATA_73CDB8.ru(), SAFETY_DISTANCE.ru()))
        .collect::<Vec<_>>();
    writes.push(common::float(
        ctx,
        "maxvisibilitymul",
        eix_sites::RDATA_73FE90,
        3.0,
    )?);
    writes.push(common::float_over(
        ctx,
        "safetymuldist",
        SAFETY_DISTANCE,
        [0x90; 4],
    )?);
    Ok(writes)
}

/// The NPC-only branches of the stats code, and the store of the NPC marker 0xC0000000 into `[ebx+0x44]`.
const NPC_BRANCHES: [(Site, &[u8]); 4] = [
    (
        eix_sites::UNIT_STATS_RECOMPUTE_HIT_POINTS__18,
        &[0x0F, 0x84, 0xB4, 0x00, 0x00, 0x00],
    ),
    (
        eix_sites::UNIT_STATS_RECOMPUTE__5BB,
        &[0x0F, 0x84, 0x88, 0x00, 0x00, 0x00],
    ),
    (
        eix_sites::UNIT_STATS_RECOMPUTE__5CA,
        &[0xC7, 0x43, 0x44, 0x00, 0x00, 0x00, 0xC0],
    ),
    (eix_sites::UNIT_STATS_RECOMPUTE__6F3, &[0x74, 0x15]),
];
/// `jne` in `AttackParams::ScaleForDifficulty`: value 1 only.
const NPC_DIFFICULTY: (Site, &[u8]) = (
    eix_sites::ATTACK_PARAMS_SCALE_FOR_DIFFICULTY__9,
    &[0x75, 0x2F],
);
/// `jne` in `AIAttention::Build`.
const NPC_ATTENTION: (Site, &[u8]) = (
    eix_sites::AI_ATTENTION_BUILD__58,
    &[0x0F, 0x85, 0x89, 0x01, 0x00, 0x00],
);

/// `removenpcsblock` 1 or 2: NPCs lose their own branches and marker (1 also in the difficulty scaling), and
/// `CBattleScreen::UnitName` always takes its monster path.
pub(crate) fn remove_npcs_block(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if !matches!(value, 1 | 2) {
        return Ok(Vec::new());
    }
    let mut writes = common::nops(&[NPC_ATTENTION]);
    writes.extend(common::nops(&NPC_BRANCHES));
    if value == 1 {
        writes.extend(common::nops(&[NPC_DIFFICULTY]));
    }
    writes.push(SiteWrite::bytes(
        eix_sites::C_BATTLE_SCREEN_UNIT_NAME__63,
        &[0x74],
        &[0xEB],
    )?);
    Ok(writes)
}

/// `call IsTextKey` sites that value 2 turns into `mov eax, 1`; the `bool` says whether value 3 does too (it keeps
/// the difficulty scaling of `CUnitClientSpecific::GetAttackParams` and `CUnitServer::GetAttackParams`).
const HERO_CALLS: [(Site, [u8; 5], bool); 14] = [
    (
        eix_sites::HOOVER_COLLECT_LOOT__D,
        [0xE8, 0xDE, 0xF6, 0xFF, 0xFF],
        true,
    ),
    (
        eix_sites::UNIT_STATS_PACK_BODY_PROTECTION__F,
        [0xE8, 0x6C, 0xE3, 0xF3, 0xFF],
        true,
    ),
    (
        eix_sites::UNIT_STATS_RECOMPUTE_HIT_POINTS__11,
        [0xE8, 0xBA, 0xCF, 0xF3, 0xFF],
        true,
    ),
    (
        eix_sites::UNIT_STATS_RECOMPUTE__14,
        [0xE8, 0x27, 0xCE, 0xF3, 0xFF],
        true,
    ),
    (eix_sites::TEXT_525083, [0xE8, 0x18, 0xC3, 0xF3, 0xFF], true),
    (
        eix_sites::UNIT_STATS_ATTRIBUTE_WITH_ITEMS__13,
        [0xE8, 0xF8, 0xB4, 0xF3, 0xFF],
        true,
    ),
    (
        eix_sites::UNIT_STATS_WEAPON_SKILL_BONUS__86,
        [0xE8, 0xA5, 0xA6, 0xF3, 0xFF],
        true,
    ),
    (
        eix_sites::UNIT_STATS_WEAPON_SKILL_BONUS__19B,
        [0xE8, 0x90, 0xA5, 0xF3, 0xFF],
        true,
    ),
    (
        eix_sites::UNIT_STATS_WEAPON_SKILL_BONUS__285,
        [0xE8, 0xA6, 0xA4, 0xF3, 0xFF],
        true,
    ),
    (
        eix_sites::C_UNIT_LEVEL__6,
        [0xE8, 0x55, 0xE6, 0xF2, 0xFF],
        true,
    ),
    (
        eix_sites::C_UNIT_CLIENT_SPECIFIC_GET_ATTACK_PARAMS__C,
        [0xE8, 0x2F, 0x8A, 0xF2, 0xFF],
        false,
    ),
    (
        eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__14,
        [0xE8, 0x57, 0x1D, 0xF1, 0xFF],
        true,
    ),
    (
        eix_sites::C_UNIT_SERVER_GET_ATTACK_PARAMS__F,
        [0xE8, 0x8C, 0xB9, 0xF0, 0xFF],
        false,
    ),
    (
        eix_sites::C_HUD_UNIT_PANEL_DRAW_UNIT_INFO__3F1,
        [0xE8, 0x6A, 0x9F, 0xE1, 0xFF],
        true,
    ),
];
const MOV_EAX_1: [u8; 5] = [0xB8, 0x01, 0x00, 0x00, 0x00];

/// `everyoneishero` 1: `IsTextKey` compares the id with 0; 2 and 3: chosen callers take every unit as a hero.
pub(crate) fn everyone_is_hero(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    match value {
        // The immediate of `cmp ecx, 1000000000` at `IsTextKey`, bytes +2 to +5.
        1 => Ok(vec![SiteWrite::bytes(
            eix_sites::IS_TEXT_KEY__2,
            &[0x00, 0xCA, 0x9A, 0x3B],
            &[0x00; 4],
        )?]),
        2 | 3 => HERO_CALLS
            .iter()
            .filter(|&&(_, _, in_3)| value == 2 || in_3)
            .map(|&(site, was, _)| Ok(SiteWrite::bytes(site, &was, &MOV_EAX_1)?))
            .collect(),
        _ => Ok(Vec::new()),
    }
}

/// `call CUnit::HasPlayer` at `AIViewer::See`+0x21.
const SEE_HAS_PLAYER: Site = eix_sites::AI_VIEWER_SEE__21;
const SEE_HAS_PLAYER_CALL: [u8; 5] = [0xE8, 0xDA, 0x5B, 0xF6, 0xFF];
/// `AIViewer::See`'s "has a player" call under value 4 (ECX and EDI the seen unit): EAX 1 for an id from 1000000000
/// up to 2000000000, else 0, and ECX the unit. The original writes this test into `HooverCollectLoot`+0x50, bytes
/// only its `improvedstealing` leaves unused; without that, looting runs into it. Ours keeps it in our own code, so
/// looting stays the game's either way.
extern "C" fn id_has_player(_: usize, ctx: &mut Context) {
    // SAFETY: ECX is the seen `CUnit`, whose +0xC is its id.
    let id = unsafe { read::<i32>(ctx.ecx + 0xC) };
    ctx.eax = u32::from((1_000_000_000..2_000_000_000).contains(&id));
    ctx.ecx = ctx.edi;
}

/// `allysseeseachothers` 1-3: `AIViewer::See` treats every unit as having a player; 2 and 3 also make
/// `AIViewer::Update` take its player branch. 4: `See` takes a hero's id range as having a player
/// ([`id_has_player`]).
pub(crate) fn allies_see_each_other(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    match value {
        1..=3 => {
            let mut writes = vec![SiteWrite::bytes(
                SEE_HAS_PLAYER,
                &SEE_HAS_PLAYER_CALL,
                &MOV_EAX_1,
            )?];
            if value >= 2 {
                // `je 0x5CAD02` -> `je 0x5CAC76` (whole, for another build's displacement), and the `jmp 0x5CAD02`
                // after it goes.
                writes.push(SiteWrite::bytes(
                    eix_sites::AI_VIEWER_UPDATE__264.back(2),
                    &[0x0F, 0x84, 0x9A, 0x00, 0x00, 0x00],
                    &[0x0F, 0x84, 0x0E, 0x00, 0x00, 0x00],
                )?);
                writes.push(SiteWrite::nop(
                    eix_sites::AI_VIEWER_UPDATE__271,
                    &[0xE9, 0x8C, 0x00, 0x00, 0x00],
                ));
            }
            Ok(writes)
        }
        4 => {
            let stub = common::call_stub(
                id_has_player,
                0,
                SEE_HAS_PLAYER.offset(SEE_HAS_PLAYER_CALL.len()),
            )?;
            Ok(vec![common::branch(
                SEE_HAS_PLAYER,
                &SEE_HAS_PLAYER_CALL,
                &[0xE8, 0, 0, 0, 0],
                0,
                stub,
            )?])
        }
        _ => Ok(Vec::new()),
    }
}

/// `allvisible`: the registry skips 1, its one value, whose original takes 0 for `DrawVisibleOnly` in
/// `GameOptions::LoadFromRegistry` instead of calling `CWinRegistry::GetInt`, so the call's three stack arguments
/// stay pushed. No other value changes anything.
pub(crate) fn all_visible(_: &Ctx<'_>, _: i8) -> Result<Vec<SiteWrite>, BuildError> {
    Ok(Vec::new())
}

/// `rewritenpcvisionarc` >= 1: `visionarc` over the immediate of `mov dword ptr [ebx+0x44], 0xC0000000` at
/// 0x524B2A, the NPC marker whose bits are the float -2.0.
pub(crate) fn npc_vision_arc(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    Ok(vec![common::float(
        ctx,
        "visionarc",
        eix_sites::UNIT_STATS_RECOMPUTE__5CD,
        -2.0,
    )?])
}

/// `UnitParams::VisionRange` rewritten in place: in a dark-zone level (the world flag at 0x7AFC98) the night value is
/// taken without the day's growth, through a `ret 4` placed in `UnitParams::Period`'s padding.
const CAVE_VISION: [Bytes; 3] = [
    (
        eix_sites::UNIT_PARAMS_PERIOD__16,
        &[0x90, 0x90, 0x90],
        &[0xC2, 0x04, 0x00],
    ),
    (
        eix_sites::UNIT_PARAMS_VISION_RANGE__13,
        &[
            0xD8, 0x4C, 0x24, 0x08, 0xD8, 0x0D, 0x9C, 0xD0, 0x73, 0x00, 0xD9, 0x05, 0x4C, 0xB8,
            0x73, 0x00, 0xD8, 0x64, 0x24, 0x08, 0x89, 0x54, 0x24, 0x08, 0xDE, 0xC1, 0xDB, 0x44,
        ],
        &[
            0x8B, 0x0D, 0x98, 0xFC, 0x7A, 0x00, 0x85, 0xC9, 0xD8, 0x0D, 0x9C, 0xD0, 0x73, 0x00,
            0x75, 0x10, 0xD8, 0x4C, 0x24, 0x08, 0xD9, 0x05, 0x4C, 0xB8, 0x73, 0x00, 0xD8, 0x64,
        ],
    ),
    (
        eix_sites::UNIT_PARAMS_VISION_RANGE__32,
        &[
            0xC9, 0x59, 0xC2, 0x04, 0x00, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
        ],
        &[
            0xC1, 0x89, 0x54, 0x24, 0x08, 0xDB, 0x44, 0x24, 0x08, 0xDE, 0xC9, 0x59, 0xEB, 0xB6,
        ],
    ),
];

/// `nightvisionincaves` >= 1: vision in dark zones is the night value.
pub(crate) fn night_vision_in_caves(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    common::fixed(&CAVE_VISION)
}

/// `visioninweather` = 1: the bad-weather vision factor of `CWorldServer::UpdateWeather` comes from the INI.
pub(crate) fn vision_in_weather(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![common::float(
        ctx,
        "visionweathermul",
        eix_sites::C_WORLD_SERVER_UPDATE_WEATHER__4D2,
        0.9,
    )?])
}

/// `hyperreactions` = 1: `AIViewer::See` and `AIMemory::SeeCorpse` drop their "own side only" tests.
pub(crate) fn hyper_reactions(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![
        SiteWrite::nop(
            eix_sites::AI_VIEWER_SEE__A9,
            &[0x0F, 0x85, 0x8C, 0x03, 0x00, 0x00],
        ),
        // `je rel32` becomes `nop; jmp rel32` to the same target.
        SiteWrite::bytes(
            eix_sites::AI_MEMORY_SEE_CORPSE__B7,
            &[0x0F, 0x84],
            &[0x90, 0xE9],
        )?,
    ])
}

/// `fadd dword ptr [ebx+0x104]` at 0x5248BA in the stats refresh, with the Stench effect's value `[ebp-0x74]` in
/// ST0; `fstp dword ptr [ebx+0x104]` at 0x5248C5 stores the sum.
const STENCH_ADD: Site = eix_sites::UNIT_STATS_RECOMPUTE__35A;
const STENCH_ADD_BYTES: [u8; 6] = [0xD8, 0x83, 0x04, 0x01, 0x00, 0x00];
const STENCH_STORE: (Site, &[u8], &[u8]) = (
    eix_sites::UNIT_STATS_RECOMPUTE__365,
    &[0xD9, 0x9B, 0x04, 0x01, 0x00, 0x00],
    // fstp st(0); nop x4
    &[0xDD, 0xD8, 0x90, 0x90, 0x90, 0x90],
);
/// `UnitStats::RateValue`'s padding that keeps `stenchmul`.
const STENCH_MUL: Site = eix_sites::UNIT_STATS_RATE_VALUE__A8;
/// The four sense stats Stench raises, in the original's order.
const STENCH_STATS: [u32; 4] = [0x10C, 0xFC, 0xF4, 0xE4];

/// The handler of `stenchremaster`: the Stench value times `stenchmul` goes onto four stats of `[ebx]` instead of
/// `+0x104`, and stays in ST0 for the `fstp st(0)` that replaces the store.
extern "C" fn stench(_: usize, ctx: &mut FpuContext) {
    // SAFETY: the factor the build wrote.
    let mul = f64::from(unsafe { read::<f32>(STENCH_MUL.here32()) });
    // The stats refresh left the Stench value in ST0.
    let value = ctx.fpu.pop().unwrap_or(Ext::new(0.0)).fmul(mul);
    for offset in STENCH_STATS {
        // SAFETY: EBX is the stats block the refresh fills.
        let stat = unsafe { read::<f32>(ctx.regs.ebx + offset) };
        let sum = Ext::new(f64::from(stat)).fadd_ext(value).to_f32();
        // SAFETY: as above.
        unsafe { write::<f32>(ctx.regs.ebx + offset, sum) };
    }
    // The code after the site pops one value with `fstp st(0)`; the pop above left room.
    let _ = ctx.fpu.push(value);
}

/// `stenchremaster` = 1: Stench makes the unit easier to see and hear.
pub(crate) fn stench_remaster(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![
        common::float_over(ctx, "stenchmul", STENCH_MUL, [0x90; 4])?,
        hook::fpu_hook(
            STENCH_ADD,
            &STENCH_ADD_BYTES,
            &STENCH_ADD_BYTES,
            Mode::Drop,
            stench,
            0,
        )?,
        SiteWrite::bytes(STENCH_STORE.0, STENCH_STORE.1, STENCH_STORE.2)?,
    ])
}

/// `je 0x5CAC8D` at `AIViewer::Update`+0x280, after the "the seen unit is an enemy" test: values 2 to 7 make it a
/// `jmp`, so the "already fighting" test after it never runs.
const VIEWER_ENEMY_JE: Bytes = (eix_sites::AI_VIEWER_UPDATE__280, &[0x74], &[0xEB]);
/// `je 0x5CADCD` at `AIViewer::Update`+0x353, which `follow` makes a `jmp`.
const VIEWER_FOLLOW_JE: Bytes = (eix_sites::AI_VIEWER_UPDATE__353, &[0x74], &[0xEB]);
/// The viewer's distance factor for units without a player (1.05 at 0x73FEAC), `visiondependence`.
const VISION_DEPENDENCE: Site = eix_sites::RDATA_73FEAC;
/// `mov eax, [ebp-0x14]; test eax, eax` at `AIViewer::Update`+0x403, the "lost from sight" path.
const VIEWER_LOST: Site = eix_sites::AI_VIEWER_UPDATE__403;
const VIEWER_LOST_BYTES: [u8; 5] = [0x8B, 0x45, 0xEC, 0x85, 0xC0];
/// `AIViewer::Update`+0x302: the path that goes on with the seen unit.
const VIEWER_SEEN: Site = eix_sites::AI_VIEWER_UPDATE__302;
/// `fmul dword ptr [edx+0x294]` at `AIViewer::Update`+0x2BB: the viewer's radius times its vision factor.
const VIEWER_RADIUS_MUL: Site = eix_sites::AI_VIEWER_UPDATE__2BB;
const VIEWER_RADIUS_MUL_BYTES: [u8; 6] = [0xD8, 0x8A, 0x94, 0x02, 0x00, 0x00];
/// Where value 1 keeps the last EAX of the lost path, and `aggrrangemulmodifier` (both read by nothing).
const AGGR_LAST_EAX: Site = eix_sites::DATA_799CF8;
const AGGR_RANGE_MUL: Site = eix_sites::DATA_799CF0;
/// Where the original keeps the parameters of values 2, 4 and 5, in game.exe's data padding: `losehardcore` (i32),
/// `losefactor1` (i32), `losefactor2` (a shift count).
const LOSE_HARDCORE_32: Site = eix_sites::DATA_799D8A;
const LOSE_FACTOR1_32: Site = eix_sites::DATA_799D70;
const LOSE_FACTOR2_SHIFT: Site = eix_sites::DATA_799D88;
/// The fixed divisor of values 4 and 5's tick remainder.
const LOSE_TICK_MODULUS: u32 = 45_000;
/// `g_updatePeriod`, the game's update period in milliseconds.
const UPDATE_PERIOD: Site = eix_sites::DATA_78FF34;
/// `GetTickCount` in game.exe's import table, which the original calls.
const GET_TICK_COUNT_IAT: Site = eix_sites::RDATA_73B344;
/// Where the original keeps values 6 and 7's parameters, in game.exe's data padding: `losehardcore` (u16), then one
/// signed byte each for `losefactor1`, `losefactor2`, `losefactor3`, `difffactor`, `maxadd`; `maxadd` once more at
/// 0x799DB0, in an instruction whose result is dropped.
const COUNTER_LIMIT: Site = eix_sites::DATA_799D64;
const COUNTER_FACTOR1: Site = eix_sites::DATA_799D91;
const COUNTER_FACTOR2: Site = eix_sites::DATA_799D94;
const COUNTER_FACTOR3: Site = eix_sites::DATA_799D97;
const COUNTER_DIFF_FACTOR: Site = eix_sites::DATA_799DA4;
const COUNTER_MAX_ADD: Site = eix_sites::DATA_799DA7;
const COUNTER_MAX_ADD_UNUSED: Site = eix_sites::DATA_799DB0;
/// The difficulty word scripts read as `GSGetVar("Diff")`.
const DIFFICULTY: Site = eix_sites::DATA_79BEC0;
/// The viewer's 16-bit aggression counter, in its AI object (`[ebp+8]`).
const AGGRESSION_COUNTER: u32 = 0x8E0;
/// `CUnit::Level` 0x532D40 (thiscall, no arguments).
const UNIT_LEVEL: Site = eix_sites::C_UNIT_LEVEL;
type UnitLevel = unsafe extern "thiscall" fn(unit: u32) -> i32;
/// `mov ecx, [edi+0x1C]; push 0x2E` at `StepAlarm`+0xB8, which `improve` hooks.
const STEP_ALARM_IMPROVE: Site = eix_sites::STEP_ALARM__B8;

/// Values 6 and 7's rewrite of `AIViewer::Update`+0x2CC (54 bytes; 7 leaves the last two as they were): the radius
/// tests end in a jump to the counter at offset [`COUNTER_JUMP`], or at [`COUNTER_JUMP_FAST`] with `loseallyfast`.
const COUNTER_TESTS: Site = eix_sites::AI_VIEWER_UPDATE__2CC;
const COUNTER_TESTS_WAS: [u8; 54] = [
    0x0F, 0x84, 0x31, 0x01, 0x00, 0x00, 0xD9, 0x81, 0xD4, 0x02, 0x00, 0x00, 0xD8, 0x8A, 0xA4, 0x02,
    0x00, 0x00, 0xD8, 0x4D, 0xF4, 0xD8, 0x5D, 0xF8, 0xDF, 0xE0, 0xF6, 0xC4, 0x41, 0x0F, 0x84, 0x14,
    0x01, 0x00, 0x00, 0x81, 0xC1, 0x80, 0x09, 0x00, 0x00, 0xE8, 0x26, 0xB2, 0x00, 0x00, 0x3B, 0xC3,
    0x0F, 0x84, 0x01, 0x01, 0x00, 0x00,
];
const COUNTER_TESTS_NOW: [u8; 54] = [
    0x74, 0x1E, 0xD9, 0x81, 0xD4, 0x02, 0x00, 0x00, 0xD8, 0x8A, 0xA4, 0x02, 0x00, 0x00, 0xD8, 0x4D,
    0xF4, 0xD8, 0x5D, 0xF8, 0xDF, 0xE0, 0xF6, 0xC4, 0x41, 0x74, 0x05, 0xE9, 0x00, 0x00, 0x00, 0x00,
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0xE9, 0x0B, 0x01, 0x00, 0x00, 0x90, 0x90, 0x90, 0x90,
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
];
const COUNTER_JUMP: usize = 0x1B;
/// `loseallyfast`'s form: the result of the enemy test (EAX) is kept on the stack across the radius tests, and a
/// unit the test calls no enemy is seen without the counter.
const COUNTER_TESTS_FAST: [u8; 54] = [
    0x74, 0x27, 0xD9, 0x81, 0xD4, 0x02, 0x00, 0x00, 0xD8, 0x8A, 0xA4, 0x02, 0x00, 0x00, 0xD8, 0x4D,
    0xF4, 0xD8, 0x5D, 0xF8, 0xDF, 0xE0, 0xF6, 0xC4, 0x41, 0x74, 0x0E, 0x58, 0x84, 0xC0, 0x0F, 0x84,
    0x12, 0x00, 0x00, 0x00, 0xE9, 0x00, 0x00, 0x00, 0x00, 0x58, 0xE9, 0x08, 0x01, 0x00, 0x00, 0x90,
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
];
const COUNTER_JUMP_FAST: usize = 0x24;
/// `loseallyfast`'s other writes: `test eax, eax; je` at +0x27E becomes `push eax; nop; jmp`, and the `je` at +0x2A0
/// that would leave with the pushed word goes.
const FAST_PUSH: Bytes = (
    eix_sites::AI_VIEWER_UPDATE__27E,
    &[0x85, 0xC0, 0x74],
    &[0x50, 0x90, 0xEB],
);

/// Value 1's hook at `AIViewer::Update`+0x403: EAX is kept at 0x799CF8 before the replaced instructions run. The
/// original also copies a word of the main thread's stack at the fixed address 0x18F2F8, which nothing reads.
extern "C" fn aggression_record(_: usize, ctx: &mut Context) {
    // SAFETY: a word of game.exe's `.data` padding that only this feature uses.
    unsafe { write::<u32>(AGGR_LAST_EAX.here32(), ctx.eax) };
}

/// The value of `fistp word`: `x` rounded under the control word, or 0x8000 outside the 16-bit range.
fn fistp_word(x: Ext) -> u16 {
    let v = x.fistp();
    i16::try_from(v).map_or(0x8000, |w| w as u16)
}

/// `GetTickCount` through the game's import, as the original calls it.
fn tick_count() -> u32 {
    // SAFETY: the import table entry game.exe's loader filled.
    let at = unsafe { read::<u32>(GET_TICK_COUNT_IAT.here32()) } as usize;
    // SAFETY: that entry is `GetTickCount`, `DWORD WINAPI (void)`.
    let f: unsafe extern "system" fn() -> u32 = unsafe { hook::as_fn_at(at) };
    // SAFETY: `GetTickCount` takes nothing and cannot fail.
    unsafe { f() }
}

/// Values 2, 4 and 5's hook after the viewer's radius product (ST0, the replaced `fmul` already applied): a viewer
/// of side 0 goes on; otherwise a number `r` is drawn, and the seen unit is lost from sight when `r / dist^1.5`,
/// rounded, is not zero. Value 2 draws the time stamp counter modulo `losehardcore`; 4 and 5 draw the tick count
/// modulo 45000, unless `ticks / (losefactor1 * g_updatePeriod)` is a multiple of that product shifted right by
/// `losefactor2`, which keeps the unit.
extern "C" fn lose_by_chance(value: usize, ctx: &mut FpuContext) {
    ctx.regs.eax = 0;
    // SAFETY: EBX is the viewer's unit; +0x250 is its side, the index of the diplomacy test above.
    if unsafe { read::<u32>(ctx.regs.ebx + 0x250) } == 0 {
        return;
    }
    let r = if value == 2 {
        // SAFETY: the parameter the build wrote.
        let modulus = unsafe { read::<u32>(LOSE_HARDCORE_32.here32()) };
        // SAFETY: `rdtsc` reads the time stamp counter; it has no other effect.
        let tsc = unsafe { core::arch::x86::_rdtsc() } as u32;
        // The original faults on a zero `losehardcore`; here the draw is 0 and the unit stays.
        tsc.checked_rem(modulus).unwrap_or(0)
    } else {
        let ticks = tick_count();
        // SAFETY: the parameters the build wrote.
        let factor = unsafe { read::<i32>(LOSE_FACTOR1_32.here32()) };
        // SAFETY: as above.
        let shift = unsafe { read::<u8>(LOSE_FACTOR2_SHIFT.here32()) };
        // SAFETY: a word of game.exe's `.data`.
        let period = unsafe { read::<i32>(UPDATE_PERIOD.here32()) };
        let product = factor.wrapping_mul(period) as u32;
        // The original faults when the shifted product is zero; here that case draws.
        if let Some(steps) = ticks.checked_div(product)
            && steps.checked_rem(product >> (shift & 31)) == Some(0)
        {
            return;
        }
        ticks % LOSE_TICK_MODULUS
    };
    // SAFETY: `[ebp-8]` is the distance to the seen unit in `AIViewer::Update`'s frame.
    let dist = f64::from(unsafe { read::<f32>(ctx.regs.ebp - 8) });
    let d = Ext::new(dist).fmul_ext(Ext::new(dist)).fmul(dist).fsqrt();
    let q = Ext::new(f64::from(r as i32)).fdiv_ext(d).fistp();
    if q != 0 {
        // The radius product the replaced `fmul` left in ST0, which the lost path does not expect.
        let _ = ctx.fpu.pop();
        ctx.regs.resume = VIEWER_LOST;
    }
}

/// Values 6 and 7's hook after the radius tests passed: a viewer of side 0 goes on with the seen unit. Otherwise its
/// AI object's counter (`[edx+0x8E0]`, 16 bits) decides: at `losehardcore` or above it restarts at 0 and the unit is
/// seen; below, the unit is lost from sight and the counter grows by `(Diff + difffactor)`, times
/// `q = (round(dist) * losefactor1 + losefactor3) / (level + losefactor2)` (unsigned) unless `q < maxadd`.
extern "C" fn aggression_counter(_: usize, ctx: &mut Context) {
    ctx.resume = VIEWER_SEEN;
    // SAFETY: EBX is the viewer's unit, as in `lose_by_chance`.
    if unsafe { read::<u32>(ctx.ebx + 0x250) } == 0 {
        return;
    }
    let counter = ctx.edx + AGGRESSION_COUNTER;
    // SAFETY: EDX is the viewer's AI object (`[ebp+8]`).
    let count = unsafe { read::<u16>(counter) };
    // SAFETY: the parameter the build wrote.
    let limit = unsafe { read::<u16>(COUNTER_LIMIT.here32()) };
    if count >= limit {
        // SAFETY: as above.
        unsafe { write::<u16>(counter, 0) };
        return;
    }
    // SAFETY: the parameters the build wrote, sign-extended as the original's 8-bit immediates are.
    let [f1, f2, f3, diff, max_add] = [
        COUNTER_FACTOR1,
        COUNTER_FACTOR2,
        COUNTER_FACTOR3,
        COUNTER_DIFF_FACTOR,
        COUNTER_MAX_ADD,
    ]
    .map(|at| i32::from(unsafe { read::<i8>(at.here32()) }));
    // SAFETY: the distance in the frame, as in `lose_by_chance`.
    let dist = fistp_word(Ext::new(f64::from(unsafe { read::<f32>(ctx.ebp - 8) })));
    // SAFETY: 0x532D40 is `int __thiscall CUnit::Level()`.
    let unit_level = unsafe { hook::as_fn::<UnitLevel>(UNIT_LEVEL) };
    // SAFETY: the viewer's unit, on the game's thread.
    let level = unsafe { unit_level(ctx.ebx) };
    let num = u32::from(dist)
        .wrapping_mul(f1 as u32)
        .wrapping_add(f3 as u32);
    // The original faults on a zero divisor; here the quotient is 0.
    let q = num
        .checked_div((level as u32).wrapping_add(f2 as u32))
        .unwrap_or(0);
    // SAFETY: a word of game.exe's `.data`.
    let mut add = unsafe { read::<u32>(DIFFICULTY.here32()) }.wrapping_add(diff as u32);
    if (q as i32) >= max_add {
        add = add.wrapping_mul(q);
    }
    // SAFETY: as above.
    unsafe { write::<u16>(counter, count.wrapping_add(add as u16)) };
    ctx.resume = VIEWER_LOST;
}

/// The low byte of the integer `[gameplay] key` at `site`, over the zero byte there.
fn param_byte(ctx: &Ctx<'_>, key: &str, site: Site) -> Result<SiteWrite, BuildError> {
    let low = ctx.values.int("gameplay", key)?.to_le_bytes()[0];
    Ok(SiteWrite::bytes(site, &[0], &[low])?)
}

/// The 32-bit `[gameplay] key` at `site`, over the zero bytes there.
fn param_int(ctx: &Ctx<'_>, key: &str, site: Site) -> Result<SiteWrite, BuildError> {
    let v = ctx.values.int("gameplay", key)?;
    Ok(SiteWrite::bytes(site, &[0; 4], &v.to_le_bytes())?)
}

/// `aggrmod` 1 to 7 (the registry skips 3, which corrupts `AIViewer::Update`'s frame): when a viewer loses sight of units, with `follow` (all values)
/// letting it keep after them. 1 only records state; 2, 4, 5 lose units by chance over distance; 6 and 7 through a
/// counter per viewer.
pub(crate) fn aggrmod(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    let flag = |key: &str| ctx.values.int("gameplay", key);
    let mut writes = match value {
        1 => vec![
            hook::context_hook(
                VIEWER_LOST,
                &VIEWER_LOST_BYTES,
                Mode::After,
                aggression_record,
                0,
            )?,
            common::float_over(ctx, "aggrrangemulmodifier", AGGR_RANGE_MUL, [0; 4])?,
        ],
        2 | 4 | 5 => {
            let mut w = common::fixed(&[VIEWER_ENEMY_JE])?;
            w.push(hook::fpu_hook(
                VIEWER_RADIUS_MUL,
                &VIEWER_RADIUS_MUL_BYTES,
                &VIEWER_RADIUS_MUL_BYTES,
                Mode::Before,
                lose_by_chance,
                value as usize,
            )?);
            if value == 2 {
                w.push(param_int(ctx, "losehardcore", LOSE_HARDCORE_32)?);
            } else {
                w.push(param_byte(ctx, "losefactor2", LOSE_FACTOR2_SHIFT)?);
                w.push(param_int(ctx, "losefactor1", LOSE_FACTOR1_32)?);
            }
            w.push(common::float(
                ctx,
                "visiondependence",
                VISION_DEPENDENCE,
                1.05,
            )?);
            w
        }
        6 | 7 => {
            let fast = flag("loseallyfast")? != 0;
            let entry = common::entry(aggression_counter, 0)?;
            let (tests, at) = if fast {
                (&COUNTER_TESTS_FAST, COUNTER_JUMP_FAST)
            } else {
                (&COUNTER_TESTS_NOW, COUNTER_JUMP)
            };
            // Value 7's block writes two bytes less, which only reach dead code. Those two end the displacement of a
            // `je` the block cuts: they stay out of the write, as the addon's fix may hold NOPs there.
            let len = if value == 6 { 54 } else { 52 };
            let mut w = if fast {
                common::fixed(&[FAST_PUSH])?
            } else {
                common::fixed(&[VIEWER_ENEMY_JE])?
            };
            w.push(common::branch(
                COUNTER_TESTS,
                &COUNTER_TESTS_WAS[..len],
                &tests[..len],
                at,
                entry,
            )?);
            if fast {
                w.extend(common::nops(&[VIEWER_RADIUS_JE]));
            }
            let limit = ctx.values.int("gameplay", "losehardcore")? as u16;
            w.push(SiteWrite::bytes(
                COUNTER_LIMIT,
                &[0; 2],
                &limit.to_le_bytes(),
            )?);
            for (key, site) in [
                ("losefactor1", COUNTER_FACTOR1),
                ("losefactor2", COUNTER_FACTOR2),
                ("losefactor3", COUNTER_FACTOR3),
                ("difffactor", COUNTER_DIFF_FACTOR),
                ("maxadd", COUNTER_MAX_ADD),
                ("maxadd", COUNTER_MAX_ADD_UNUSED),
            ] {
                w.push(param_byte(ctx, key, site)?);
            }
            w.push(common::float(
                ctx,
                "visiondependence",
                VISION_DEPENDENCE,
                1.05,
            )?);
            if flag("improve")? != 0 {
                // The original loads the unit into EAX and lets the game push ECX, the log stream, as the unit.
                (ctx.log)(
                    Level::Warn,
                    "spelladdon",
                    &format!(
                        "aggrmod: improve's hook at {STEP_ALARM_IMPROVE:#x} passes the alarm a wrong unit \
                         and is left out"
                    ),
                    "",
                );
            }
            w
        }
        _ => return Ok(Vec::new()),
    };
    if flag("follow")? != 0 {
        writes.extend(common::fixed(&[VIEWER_FOLLOW_JE])?);
    }
    if value == 1 {
        ctx.runtime_write::<u32>(AGGR_LAST_EAX);
    }
    Ok(writes)
}

/// `AIViewer::Noise`'s register fixes, shared by both values: the heard unit stays in ESI and the listener's sound
/// list moves to EBX, so the unit is sent to where the sound came from.
const NOISE_REGISTERS: [Bytes; 8] = [
    (eix_sites::AI_VIEWER_NOISE__13F, &[0x76], &[0x5E]),
    (eix_sites::AI_VIEWER_NOISE__160, &[0x56], &[0x53]),
    (eix_sites::AI_VIEWER_NOISE__17E, &[0xB7], &[0x9F]),
    (eix_sites::AI_VIEWER_NOISE__18E, &[0x56], &[0x53]),
    (
        eix_sites::AI_VIEWER_NOISE__1AB,
        &[0x8B, 0xCE],
        &[0x89, 0xD9],
    ),
    (eix_sites::AI_VIEWER_NOISE__1B7, &[0x46], &[0x43]),
    (eix_sites::AI_VIEWER_NOISE__1C4, &[0x53], &[0x56]),
    (
        eix_sites::AI_VIEWER_NOISE__1F8,
        &[0x8B, 0xCE],
        &[0x89, 0xD9],
    ),
];
/// Value 2: the copy of the heard position into the order (`[esp+0x18]`, `+0x1C`) becomes two moved instructions and
/// a call, followed by NOPs.
const NOISE_ORDER_MOVE: Bytes = (
    eix_sites::AI_VIEWER_NOISE__1CB,
    &[0x02, 0x89, 0x44, 0x24, 0x18, 0x8B, 0x44],
    &[0x44, 0x24, 0x64, 0x89, 0x44, 0x24, 0x24],
);
const NOISE_ORDER_CALL: Site = eix_sites::AI_VIEWER_NOISE__1D2;
const NOISE_ORDER_CALL_BYTES: [u8; 13] = [
    0x24, 0x64, 0x8B, 0x4A, 0x04, 0x89, 0x44, 0x24, 0x24, 0x89, 0x4C, 0x24, 0x1C,
];
/// `AIViewer::Noise`'s padding that keeps `runtosoundsmul` and `randsoundfactor`.
const RUN_TO_SOUNDS_MUL: Site = eix_sites::AI_VIEWER_NOISE__278;
const RAND_SOUND_FACTOR: Site = eix_sites::AI_VIEWER_NOISE__27C;

/// Value 2's call: the order's x and y are `p + runtosoundsmul * (p - heard)`, with `p` the point at `[esp+0x5C]`
/// (`Noise`'s argument) and `heard` EDX, each plus `randsoundfactor * (r - 32768)` for `r` the low 16 bits of the time
/// stamp counter.
extern "C" fn noise_target(_: usize, ctx: &mut Context) {
    // SAFETY: the factors the build wrote into `Noise`'s padding.
    let mul = f64::from(unsafe { read::<f32>(RUN_TO_SOUNDS_MUL.here32()) });
    // SAFETY: as above.
    let rand = f64::from(unsafe { read::<f32>(RAND_SOUND_FACTOR.here32()) });
    let Some(point) = ctx.stack_word(0x5C / 4) else {
        return;
    };
    for (i, word) in [(0u32, 0x18 / 4), (4, 0x1C / 4)] {
        // SAFETY: the point is a live vector of three floats.
        let p = f64::from(unsafe { read::<f32>(point + i) });
        // SAFETY: EDX is the heard unit's position, likewise.
        let heard = f64::from(unsafe { read::<f32>(ctx.edx + i) });
        let toward = Ext::new(p).fsub(heard).fmul(mul).fadd(p);
        // SAFETY: as in `lose_by_chance`.
        let r = (unsafe { core::arch::x86::_rdtsc() } as u32) & 0xFFFF;
        let jitter = Ext::new(f64::from(r)).fsub(32768.0).fmul(rand);
        let bits = toward.fadd_ext(jitter).to_f32().to_bits();
        ctx.set_stack_word(word, bits);
        ctx.eax = bits;
    }
}

/// `hearingreactionspatch` 1 or 2: units go towards what they hear; 2 aims near the heard unit, scattered.
pub(crate) fn hearing_reactions(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if !matches!(value, 1 | 2) {
        return Ok(Vec::new());
    }
    let mut writes = common::fixed(&NOISE_REGISTERS)?;
    if value == 2 {
        writes.extend(common::fixed(&[NOISE_ORDER_MOVE])?);
        writes.push(hook::context_call(
            NOISE_ORDER_CALL,
            &NOISE_ORDER_CALL_BYTES,
            Mode::Drop,
            noise_target,
            0,
        )?);
        writes.push(common::float_over(
            ctx,
            "runtosoundsmul",
            RUN_TO_SOUNDS_MUL,
            [0x90; 4],
        )?);
        writes.push(common::float_over(
            ctx,
            "randsoundfactor",
            RAND_SOUND_FACTOR,
            [0x90; 4],
        )?);
    }
    Ok(writes)
}

/// `jne +8; pop edi; xor al, al` at `CUnitServer::CanCast`+0xD7, the "not enough mana" exit.
const CAN_CAST_MANA: Site = eix_sites::C_UNIT_SERVER_CAN_CAST__D7;
const CAN_CAST_MANA_BYTES: [u8; 5] = [0x75, 0x08, 0x5F, 0x32, 0xC0];
/// `CanCast`'s "can cast" exit, and its "cannot" exit at the `pop edi`.
const CAN_CAST_YES: Site = eix_sites::C_UNIT_SERVER_CAN_CAST__E1;
const CAN_CAST_NO: Site = eix_sites::C_UNIT_SERVER_CAN_CAST__D9;
/// `test eax, eax; je` and the first byte of the AI class test at `CUnitServer::UpdateSpellBlockFlag`+0xD.
const SPELL_BLOCK: Site = eix_sites::C_UNIT_SERVER_UPDATE_SPELL_BLOCK_FLAG__D;
const SPELL_BLOCK_BYTES: [u8; 5] = [0x85, 0xC0, 0x74, 0x19, 0x83];
/// `UpdateSpellBlockFlag`'s tails: set the unit's "no spells" flag, or clear it.
const SPELL_BLOCK_SET: Site = eix_sites::C_UNIT_SERVER_UPDATE_SPELL_BLOCK_FLAG__1A;
const SPELL_BLOCK_CLEAR: Site = eix_sites::C_UNIT_SERVER_UPDATE_SPELL_BLOCK_FLAG__2A;
/// The unit's "no spells" flag in `[unit+8]`.
const NO_SPELLS: u32 = 0x1000_0000;
/// The 1.0 `UnitStats::Period20` divides by the stats' +0x20.
const PERIOD_NUMERATOR: Site = eix_sites::RDATA_73E124;

/// `CanCast` without the mana for the spell: a unit that is no hero gets the "no spells" flag, so it fights with its
/// weapons; both return false as before.
extern "C" fn can_cast_mana(_: usize, ctx: &mut Context) {
    if ctx.eflags & 0x40 == 0 {
        ctx.resume = CAN_CAST_YES;
        return;
    }
    // SAFETY: ESI is the unit controller `CanCast` runs on; +0x1C is its unit.
    let unit = unsafe { read::<u32>(ctx.esi + 0x1C) };
    // SAFETY: as above; +0xC is the unit's id.
    let id = unsafe { read::<u32>(unit + 0xC) };
    // SAFETY: the unit's flags.
    let mut flags = unsafe { read::<u32>(unit + 8) };
    if !common::is_hero_id(id) {
        flags |= NO_SPELLS;
        // SAFETY: as above.
        unsafe { write::<u32>(unit + 8, flags) };
    }
    ctx.ecx = unit;
    ctx.eax = flags;
    ctx.resume = CAN_CAST_NO;
}

/// `UpdateSpellBlockFlag` where the game would clear the flag: a unit without a player and with a spell in the slot
/// EAX names keeps it set while its mana is below the spell's cost times `Period20`. EDI is left holding the spell,
/// as the original's code leaves it.
extern "C" fn spell_block_mana(_: usize, ctx: &mut Context) {
    // SAFETY: ESI is the unit controller; +0x28C is its AI class.
    if ctx.eax != 0 && unsafe { read::<u32>(ctx.esi + 0x28C) } != 0x10 {
        ctx.resume = SPELL_BLOCK_SET;
        return;
    }
    ctx.resume = SPELL_BLOCK_CLEAR;
    // SAFETY: as above.
    let unit = unsafe { read::<u32>(ctx.esi + 0x1C) };
    // SAFETY: `CUnit::GetSpellObject` of the controller's unit, on the game's thread.
    let spell = unsafe { Unit::spell(unit as *mut Unit, ctx.eax) } as u32;
    ctx.edi = spell;
    if spell == 0 {
        return;
    }
    // SAFETY: the controller's unit, on the game's thread.
    if unsafe { common::has_player(unit) } {
        return;
    }
    // SAFETY: the spell's cost.
    let cost = unsafe { read::<f32>(spell + 0x40) };
    // SAFETY: the unit's vitals, whose +0x34 is the mana.
    let vitals = unsafe { read::<u32>(unit + 0x170) };
    // SAFETY: as above.
    let mana = unsafe { read::<f32>(vitals + 0x34) };
    // SAFETY: the controller's stats (+0x1E0), whose +0x20 `Period20` divides by.
    let period = unsafe { read::<f32>(ctx.esi + 0x1E0 + 0x20) };
    // SAFETY: a constant of game.exe's `.rdata`.
    let numerator = unsafe { read::<f32>(PERIOD_NUMERATOR.here32()) };
    let need = Ext::new(f64::from(numerator))
        .fdiv(f64::from(period))
        .fmul(f64::from(cost));
    if need.fcom(f64::from(mana)) & (C0 | C3) == 0 {
        ctx.resume = SPELL_BLOCK_SET;
    }
}

/// `npcsspellcastingtactics` = 1: units without the mana for their spell fight with weapons until it is back.
pub(crate) fn spell_casting_tactics(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![
        hook::context_hook(
            CAN_CAST_MANA,
            &CAN_CAST_MANA_BYTES,
            Mode::Drop,
            can_cast_mana,
            0,
        )?,
        hook::context_hook(
            SPELL_BLOCK,
            &SPELL_BLOCK_BYTES,
            Mode::Drop,
            spell_block_mana,
            0,
        )?,
    ])
}

/// `unitsrunfromdangers` = 1 is in the registry's skip list: its code passes `FearMode`'s danger point (`ebx+0x10`)
/// to the path search as the unit, which reads unit fields from it. No other value changes anything.
pub(crate) fn units_run_from_dangers(_: &Ctx<'_>, _: i8) -> Result<Vec<SiteWrite>, BuildError> {
    Ok(Vec::new())
}

/// `sizerebalance` 1 to 3 are in the registry's skip list: the original's stats routine stores the item size sum
/// into `unit+0x49c`, the vtable pointer of an object inside the unit. No other value changes anything.
pub(crate) fn size_rebalance(_: &Ctx<'_>, _: i8) -> Result<Vec<SiteWrite>, BuildError> {
    Ok(Vec::new())
}

/// `fcom dword ptr [0x73CCF8]` (20.0, evening) at `CWorld::NightLevel`+0x15: its operand.
const NIGHT_EVENING_OPERAND: Site = eix_sites::C_WORLD_NIGHT_LEVEL__17;
/// `CWorld::NightLevel`+0x29 rewritten in place: the branches of the hour test, whose jumps at +1 and +0x1D go to
/// the two ramps, then the five parameters.
const NIGHT_CODE: Site = eix_sites::C_WORLD_NIGHT_LEVEL__29;
const NIGHT_CODE_WAS: [u8; 35] = [
    0x0E, 0xD8, 0x25, 0xF8, 0xCC, 0x73, 0x00, 0xD8, 0x0D, 0xB0, 0xC3, 0x73, 0x00, 0x59, 0xC3, 0xD8,
    0x1D, 0x50, 0xCE, 0x73, 0x00, 0xDF, 0xE0, 0xF6, 0xC4, 0x01, 0x74, 0x08, 0xD9, 0x05, 0x4C, 0xB8,
    0x73, 0x00, 0x59,
];
const NIGHT_CODE_NOW: [u8; 35] = [
    0x05, 0xE9, 0x00, 0x00, 0x00, 0x00, 0xD8, 0x15, 0xA8, 0xFC, 0x55, 0x00, 0xDF, 0xE0, 0xF6, 0xC4,
    0x01, 0x74, 0x0A, 0xDD, 0xD8, 0xD9, 0x05, 0x4C, 0xB8, 0x73, 0x00, 0x59, 0xC3, 0xE9, 0x00, 0x00,
    0x00, 0x00, 0x90,
];
const NIGHT_EVENING_JUMP: usize = 1;
const NIGHT_MORNING_JUMP: usize = 0x1D;
/// `(key, address, the game's bytes there)`: the hours and the ramp's slope.
const NIGHT_PARAMS: [(&str, Site, [u8; 4]); 5] = [
    (
        "visionevening",
        eix_sites::C_WORLD_NIGHT_LEVEL__4C,
        [0xC3, 0xD9, 0x44, 0x24],
    ),
    (
        "visionnight",
        eix_sites::C_WORLD_NIGHT_LEVEL__50,
        [0x00, 0x59, 0xC3, 0x90],
    ),
    ("visionday", eix_sites::C_WORLD_NIGHT_LEVEL__54, [0x90; 4]),
    (
        "visionmorning",
        eix_sites::C_WORLD_NIGHT_LEVEL__58,
        [0x90; 4],
    ),
    (
        "visionmulpowbetween",
        eix_sites::C_WORLD_NIGHT_LEVEL__5C,
        [0x90; 4],
    ),
];
/// game.exe's 1.0 and 0.0.
const ONE: Site = eix_sites::RDATA_73B84C;
const ZERO: Site = eix_sites::RDATA_73B858;

/// The two ramps, entered with the hour in ST0 and leaving `NightLevel` with the level there: after the evening
/// (`arg` 0) `(hour - evening) * slope` until the night hour, then 1; from the morning hour (`arg` 1)
/// `(day - hour) * slope` until the day hour, then 0.
extern "C" fn night_ramp(arg: usize, ctx: &mut FpuContext) {
    let [evening, night, day, _, slope] = NIGHT_PARAMS.map(|(_, at, _)| {
        // SAFETY: the parameters the build wrote into `NightLevel`'s bytes.
        f64::from(unsafe { read::<f32>(at.here32()) })
    });
    // The hour `NightLevel` computed, in ST0 at both jumps.
    let hour = ctx.fpu.pop().unwrap_or(Ext::new(0.0));
    let (end, past) = if arg == 0 { (night, ONE) } else { (day, ZERO) };
    let level = if hour.fcom(end) & C0 != 0 {
        if arg == 0 {
            hour.fsub(evening).fmul(slope)
        } else {
            Ext::new(day).fsub_ext(hour).fmul(slope)
        }
    } else {
        // SAFETY: a constant of game.exe's `.rdata`.
        Ext::new(f64::from(unsafe { read::<f32>(past.here32()) }))
    };
    // `NightLevel` returns its level in ST0; the pop above left room.
    let _ = ctx.fpu.push(level);
}

/// `nightpowervisionpatch` = 1: the night level ramps up in the evening and down in the morning.
pub(crate) fn night_power_vision(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    // pop ecx; ret: the end of `NightLevel`.
    const RETURN: [u8; 2] = [0x59, 0xC3];
    let mut code = NIGHT_CODE_NOW;
    for (at, arg) in [(NIGHT_EVENING_JUMP, 0), (NIGHT_MORNING_JUMP, 1)] {
        let stub = hook::fpu_stub(NOWHERE, &[], &RETURN, false, night_ramp, arg, NOWHERE)?;
        code[at..at + 5].copy_from_slice(&patch::jmp_rel32(NIGHT_CODE.offset(at), stub));
    }
    let mut writes = vec![
        SiteWrite::pointer(
            NIGHT_EVENING_OPERAND,
            eix_sites::RDATA_73CCF8.ru(),
            NIGHT_PARAMS[0].1.ru(),
        ),
        SiteWrite::bytes(NIGHT_CODE, &NIGHT_CODE_WAS, &code)?,
    ];
    for (key, at, was) in NIGHT_PARAMS {
        writes.push(common::float_over(ctx, key, at, was)?);
    }
    Ok(writes)
}

#[cfg(test)]
mod tests {
    use super::super::testing::{Original, assert_original, no_log, plain, values};
    use super::*;

    /// The bytes of game.exe's code and constants the original changes for each INI, every other key at its
    /// built-in default.
    // One row per INI: rustfmt would spread each over dozens of lines.
    #[rustfmt::skip]
    const ORIGINAL: &[Original] = &[
        ("aicastertable", "rebuildedaicastertable", 1, &[], &[(eix_sites::AI_ATTENTION_SPELL_KIND__13, "36"), (eix_sites::AI_ATTENTION_SPELL_KIND__1A, "48"), (eix_sites::AI_ATTENTION_SPELL_KIND__88, "08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08"), (eix_sites::AI_ATTENTION_SPELL_KIND__A4, "08 08 08"), (eix_sites::AI_ATTENTION_SPELL_KIND__A9, "08 08 08 08 08 08"), (eix_sites::AI_ATTENTION_SPELL_KIND__B0, "08 08 08 08 08 08 08 08 08 08 08 08 08 08")]),
        ("aioffendtable", "rebuildedaioffendtable", 1, &[], &[(eix_sites::C_SPELL_IS_OFFENSIVE__28, "08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08 08")]),
        ("gameplay", "visionfix", 1, &[], &[(eix_sites::AI_VIEWER_UPDATE__2A0, "90 90 90 90 90 90"), (eix_sites::AI_VIEWER_UPDATE__2FC, "90 90 90 90 90 90")]),
        ("gameplay", "visionfix", 1, &[("gameplay", "aggrmod", "6")], &[(eix_sites::AI_VIEWER_UPDATE__280, "EB"), (eix_sites::AI_VIEWER_UPDATE__2A0, "90 90 90 90 90 90"), (eix_sites::AI_VIEWER_UPDATE__2CC, "74 1E D9 81 D4 02 00 00 D8 8A A4 02 00 00 D8 4D F4 D8 5D F8 DF E0 F6 C4 41 74 05 E9 ?? ?? ?? ?? 90 90 90 90 90 90 90 E9 0B 01 00 00 90 90 90 90 90 90 90 90 90 90")]),
        ("gameplay", "spellsprioritys", 1, &[], &[(eix_sites::RDATA_73FF0A, "20 41"), (eix_sites::RDATA_73FF0E, "20 41 00 00 20 41"), (eix_sites::RDATA_73FF16, "20 41"), (eix_sites::RDATA_73FF1A, "20"), (eix_sites::RDATA_73FF1E, "20 41")]),
        ("gameplay", "otherspellcastingai", 1, &[], &[(eix_sites::AI_ATTENTION_IS_USABLE__192, "B0 01")]),
        ("gameplay", "stupidenemies", 1, &[], &[(eix_sites::C_UNIT_SERVER_WITNESS_MURDER__16D, "90 90 90 90 90 90 90 90 90 90 90 90 90 90 90")]),
        ("gameplay", "fearpatch", 1, &[], &[]),
        ("gameplay", "newplayersafetyconditions", 1, &[], &[(eix_sites::C_PLAYER_SERVER_SEND_PARTY_STATE__7F, "9C E7 66"), (eix_sites::C_PLAYER_SERVER_SEND_PARTY_STATE__92, "9C E7 66"), (eix_sites::C_PLAYER_SERVER_SEND_PARTY_STATE__A5, "9C E7 66"), (eix_sites::C_PLAYER_SERVER_SEND_PARTY_STATE__B8, "9C E7 66"), (eix_sites::C_PLAYER_SERVER_SEND_PARTY_STATE__CB, "9C E7 66"), (eix_sites::C_PLAYER_SERVER_SEND_PARTY_STATE__36C, "00 00 00 40")]),
        ("gameplay", "removenpcsblock", 1, &[], &[(eix_sites::UNIT_STATS_RECOMPUTE_HIT_POINTS__18, "90 90 90 90 90 90"), (eix_sites::UNIT_STATS_RECOMPUTE__5BB, "90 90 90 90 90 90"), (eix_sites::UNIT_STATS_RECOMPUTE__5CA, "90 90 90 90 90 90 90"), (eix_sites::UNIT_STATS_RECOMPUTE__6F3, "90 90"), (eix_sites::ATTACK_PARAMS_SCALE_FOR_DIFFICULTY__9, "90 90"), (eix_sites::AI_ATTENTION_BUILD__58, "90 90 90 90 90 90"), (eix_sites::C_BATTLE_SCREEN_UNIT_NAME__63, "EB")]),
        ("gameplay", "removenpcsblock", 2, &[], &[(eix_sites::UNIT_STATS_RECOMPUTE_HIT_POINTS__18, "90 90 90 90 90 90"), (eix_sites::UNIT_STATS_RECOMPUTE__5BB, "90 90 90 90 90 90"), (eix_sites::UNIT_STATS_RECOMPUTE__5CA, "90 90 90 90 90 90 90"), (eix_sites::UNIT_STATS_RECOMPUTE__6F3, "90 90"), (eix_sites::AI_ATTENTION_BUILD__58, "90 90 90 90 90 90"), (eix_sites::C_BATTLE_SCREEN_UNIT_NAME__63, "EB")]),
        ("gameplay", "everyoneishero", 1, &[], &[(eix_sites::IS_TEXT_KEY__3, "00 00 00")]),
        ("gameplay", "everyoneishero", 2, &[], &[(eix_sites::HOOVER_COLLECT_LOOT__D, "B8 01 00 00 00"), (eix_sites::UNIT_STATS_PACK_BODY_PROTECTION__F, "B8 01 00 00 00"), (eix_sites::UNIT_STATS_RECOMPUTE_HIT_POINTS__11, "B8 01 00 00 00"), (eix_sites::UNIT_STATS_RECOMPUTE__14, "B8 01 00 00 00"), (eix_sites::TEXT_525083, "B8 01 00 00 00"), (eix_sites::UNIT_STATS_ATTRIBUTE_WITH_ITEMS__13, "B8 01 00 00 00"), (eix_sites::UNIT_STATS_WEAPON_SKILL_BONUS__86, "B8 01 00 00 00"), (eix_sites::UNIT_STATS_WEAPON_SKILL_BONUS__19B, "B8 01 00 00 00"), (eix_sites::UNIT_STATS_WEAPON_SKILL_BONUS__285, "B8 01 00 00 00"), (eix_sites::C_UNIT_LEVEL__6, "B8 01 00 00 00"), (eix_sites::C_UNIT_CLIENT_SPECIFIC_GET_ATTACK_PARAMS__C, "B8 01 00 00 00"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__14, "B8 01 00 00 00"), (eix_sites::C_UNIT_SERVER_GET_ATTACK_PARAMS__F, "B8 01 00 00 00"), (eix_sites::C_HUD_UNIT_PANEL_DRAW_UNIT_INFO__3F1, "B8 01 00 00 00")]),
        ("gameplay", "everyoneishero", 3, &[], &[(eix_sites::HOOVER_COLLECT_LOOT__D, "B8 01 00 00 00"), (eix_sites::UNIT_STATS_PACK_BODY_PROTECTION__F, "B8 01 00 00 00"), (eix_sites::UNIT_STATS_RECOMPUTE_HIT_POINTS__11, "B8 01 00 00 00"), (eix_sites::UNIT_STATS_RECOMPUTE__14, "B8 01 00 00 00"), (eix_sites::TEXT_525083, "B8 01 00 00 00"), (eix_sites::UNIT_STATS_ATTRIBUTE_WITH_ITEMS__13, "B8 01 00 00 00"), (eix_sites::UNIT_STATS_WEAPON_SKILL_BONUS__86, "B8 01 00 00 00"), (eix_sites::UNIT_STATS_WEAPON_SKILL_BONUS__19B, "B8 01 00 00 00"), (eix_sites::UNIT_STATS_WEAPON_SKILL_BONUS__285, "B8 01 00 00 00"), (eix_sites::C_UNIT_LEVEL__6, "B8 01 00 00 00"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__14, "B8 01 00 00 00"), (eix_sites::C_HUD_UNIT_PANEL_DRAW_UNIT_INFO__3F1, "B8 01 00 00 00")]),
        ("gameplay", "allysseeseachothers", 1, &[], &[(eix_sites::AI_VIEWER_SEE__21, "B8 01 00 00 00")]),
        ("gameplay", "allysseeseachothers", 2, &[], &[(eix_sites::AI_VIEWER_SEE__21, "B8 01 00 00 00"), (eix_sites::AI_VIEWER_UPDATE__264, "0E"), (eix_sites::AI_VIEWER_UPDATE__271, "90 90 90 90 90")]),
        ("gameplay", "allysseeseachothers", 3, &[], &[(eix_sites::AI_VIEWER_SEE__21, "B8 01 00 00 00"), (eix_sites::AI_VIEWER_UPDATE__264, "0E"), (eix_sites::AI_VIEWER_UPDATE__271, "90 90 90 90 90")]),
        ("gameplay", "rewritenpcvisionarc", 1, &[], &[]),
        ("gameplay", "nightvisionincaves", 1, &[], &[(eix_sites::UNIT_PARAMS_PERIOD__16, "C2 04 00"), (eix_sites::UNIT_PARAMS_VISION_RANGE__13, "8B 0D 98 FC 7A 00 85 C9 D8 0D 9C D0 73 00 75 10"), (eix_sites::UNIT_PARAMS_VISION_RANGE__24, "4C"), (eix_sites::UNIT_PARAMS_VISION_RANGE__27, "D9 05 4C B8 73 00 D8 64"), (eix_sites::UNIT_PARAMS_VISION_RANGE__32, "C1 89 54 24 08 DB 44 24 08 DE C9 59 EB B6")]),
        ("gameplay", "visioninweather", 1, &[], &[]),
        ("gameplay", "hyperreactions", 1, &[], &[(eix_sites::AI_VIEWER_SEE__A9, "90 90 90 90 90 90"), (eix_sites::AI_MEMORY_SEE_CORPSE__B7, "90 E9")]),
        ("gameplay", "stenchremaster", 1, &[], &[(eix_sites::UNIT_STATS_RATE_VALUE__A8, "CD CC CC 3D"), (eix_sites::UNIT_STATS_RECOMPUTE__35A, "E9 ?? ?? ?? ?? 90"), (eix_sites::UNIT_STATS_RECOMPUTE__365, "DD D8 90 90 90 90")]),
        ("gameplay", "aggrmod", 1, &[], &[(eix_sites::AI_VIEWER_UPDATE__403, "E9 ?? ?? ?? ??")]),
        ("gameplay", "aggrmod", 1, &[("gameplay", "follow", "1")], &[(eix_sites::AI_VIEWER_UPDATE__353, "EB"), (eix_sites::AI_VIEWER_UPDATE__403, "E9 ?? ?? ?? ??")]),
        ("gameplay", "aggrmod", 2, &[], &[(eix_sites::AI_VIEWER_UPDATE__280, "EB"), (eix_sites::AI_VIEWER_UPDATE__2BB, "E9 ?? ?? ?? ?? 90")]),
        ("gameplay", "aggrmod", 4, &[], &[(eix_sites::AI_VIEWER_UPDATE__280, "EB"), (eix_sites::AI_VIEWER_UPDATE__2BB, "E9 ?? ?? ?? ?? 90")]),
        ("gameplay", "aggrmod", 5, &[], &[(eix_sites::AI_VIEWER_UPDATE__280, "EB"), (eix_sites::AI_VIEWER_UPDATE__2BB, "E9 ?? ?? ?? ?? 90")]),
        ("gameplay", "aggrmod", 6, &[], &[(eix_sites::AI_VIEWER_UPDATE__280, "EB"), (eix_sites::AI_VIEWER_UPDATE__2CC, "74 1E D9 81 D4 02 00 00 D8 8A A4 02 00 00 D8 4D F4 D8 5D F8 DF E0 F6 C4 41 74 05 E9 ?? ?? ?? ?? 90 90 90 90 90 90 90 E9 0B 01 00 00 90 90 90 90 90 90 90 90 90 90")]),
        ("gameplay", "aggrmod", 7, &[], &[(eix_sites::AI_VIEWER_UPDATE__280, "EB"), (eix_sites::AI_VIEWER_UPDATE__2CC, "74 1E D9 81 D4 02 00 00 D8 8A A4 02 00 00 D8 4D F4 D8 5D F8 DF E0 F6 C4 41 74 05 E9 ?? ?? ?? ?? 90 90 90 90 90 90 90 E9 0B 01 00 00 90 90 90 90 90 90 90 90")]),
        ("gameplay", "aggrmod", 6, &[("gameplay", "loseallyfast", "1"), ("gameplay", "follow", "1")], &[(eix_sites::AI_VIEWER_UPDATE__27E, "50 90 EB"), (eix_sites::AI_VIEWER_UPDATE__2A0, "90 90 90 90 90 90"), (eix_sites::AI_VIEWER_UPDATE__2CC, "74 27 D9 81 D4 02 00 00 D8 8A A4 02 00 00 D8 4D F4 D8 5D F8 DF E0 F6 C4 41 74 0E 58 84 C0 0F 84 12"), (eix_sites::AI_VIEWER_UPDATE__2EF, "00 E9 ?? ?? ?? ?? 58 E9 08 01"), (eix_sites::AI_VIEWER_UPDATE__2FA, "00 90 90 90 90 90 90 90"), (eix_sites::AI_VIEWER_UPDATE__353, "EB")]),
        ("gameplay", "aggrmod", 7, &[("gameplay", "loseallyfast", "1")], &[(eix_sites::AI_VIEWER_UPDATE__27E, "50 90 EB"), (eix_sites::AI_VIEWER_UPDATE__2A0, "90 90 90 90 90 90"), (eix_sites::AI_VIEWER_UPDATE__2CC, "74 27 D9 81 D4 02 00 00 D8 8A A4 02 00 00 D8 4D F4 D8 5D F8 DF E0 F6 C4 41 74 0E 58 84 C0 0F 84 12"), (eix_sites::AI_VIEWER_UPDATE__2EF, "00 E9 ?? ?? ?? ?? 58 E9 08 01"), (eix_sites::AI_VIEWER_UPDATE__2FA, "00 90 90 90 90 90")]),
        ("gameplay", "hearingreactionspatch", 1, &[], &[(eix_sites::AI_VIEWER_NOISE__13F, "5E"), (eix_sites::AI_VIEWER_NOISE__160, "53"), (eix_sites::AI_VIEWER_NOISE__17E, "9F"), (eix_sites::AI_VIEWER_NOISE__18E, "53"), (eix_sites::AI_VIEWER_NOISE__1AB, "89 D9"), (eix_sites::AI_VIEWER_NOISE__1B7, "43"), (eix_sites::AI_VIEWER_NOISE__1C4, "56"), (eix_sites::AI_VIEWER_NOISE__1F8, "89 D9")]),
        ("gameplay", "hearingreactionspatch", 2, &[], &[(eix_sites::AI_VIEWER_NOISE__13F, "5E"), (eix_sites::AI_VIEWER_NOISE__160, "53"), (eix_sites::AI_VIEWER_NOISE__17E, "9F"), (eix_sites::AI_VIEWER_NOISE__18E, "53"), (eix_sites::AI_VIEWER_NOISE__1AB, "89 D9"), (eix_sites::AI_VIEWER_NOISE__1B7, "43"), (eix_sites::AI_VIEWER_NOISE__1C4, "56"), (eix_sites::AI_VIEWER_NOISE__1CB, "44 24 64 89 44 24 24 E8 ?? ?? ?? ?? 90 90 90 90 90 90 90 90"), (eix_sites::AI_VIEWER_NOISE__1F8, "89 D9"), (eix_sites::AI_VIEWER_NOISE__278, "00 00 80 3F 00 00 80 3F")]),
        ("gameplay", "npcsspellcastingtactics", 1, &[], &[(eix_sites::C_UNIT_SERVER_CAN_CAST__D7, "E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_UPDATE_SPELL_BLOCK_FLAG__D, "E9 ?? ?? ?? ??")]),
        ("gameplay", "nightpowervisionpatch", 1, &[], &[(eix_sites::C_WORLD_NIGHT_LEVEL__17, "9C FC 55"), (eix_sites::C_WORLD_NIGHT_LEVEL__29, "05 E9 ?? ?? ?? ?? D8 15 A8 FC 55 00 DF E0 F6 C4 01 74 0A DD D8 D9 05 4C B8 73 00 59 C3 E9 ?? ?? ?? ?? 90 00 00 90 41"), (eix_sites::C_WORLD_NIGHT_LEVEL__51, "00 B0 41 00 00 00 41 00 00 80 40 00 00 80 3E")]),
    ];

    #[test]
    fn the_sites_match_the_original() {
        assert_original(ORIGINAL);
    }

    #[test]
    fn the_parameters_land_where_the_original_keeps_them() {
        let v = values(&[
            ("gameplay", "losehardcore", "70000"),
            ("gameplay", "losefactor1", "300"),
            ("gameplay", "losefactor3", "-2"),
            ("gameplay", "aggrrangemulmodifier", "7.5"),
        ]);
        let ctx = Ctx::new(&v, no_log);
        let at = |value: i8, site: Site| {
            plain(&aggrmod(&ctx, value).unwrap())
                .into_iter()
                .find(|w| w.0 == site)
                .map(|w| w.2)
                .unwrap()
        };
        assert_eq!(at(1, eix_sites::DATA_799CF0), 7.5f32.to_le_bytes());
        assert_eq!(at(2, eix_sites::DATA_799D8A), 70000i32.to_le_bytes());
        assert_eq!(at(4, eix_sites::DATA_799D70), 300i32.to_le_bytes());
        assert_eq!(
            at(6, eix_sites::DATA_799D64),
            (70000u32 as u16).to_le_bytes()
        );
        assert_eq!(at(6, eix_sites::DATA_799D91), [44]);
        assert_eq!(at(7, eix_sites::DATA_799D97), [0xFE]);
        assert_eq!(fistp_word(Ext::new(40000.0)), 0x8000);
        assert_eq!(fistp_word(Ext::new(-3.0)), 0xFFFD);
    }

    #[test]
    fn the_registry_skips_the_defective_values() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        assert!(aggrmod(&ctx, 8).unwrap().is_empty());
        let skips = |key: &str, value: i8| {
            let f = crate::registry::FEATURES
                .iter()
                .find(|f| f.key == key)
                .unwrap();
            crate::registry::skipped(f, value, &v).is_some()
        };
        assert!(skips("aggrmod", 3) && !skips("aggrmod", 6));
        assert!(skips("unitsrunfromdangers", 1));
        assert!(
            skips("sizerebalance", 1) && skips("sizerebalance", 3) && !skips("sizerebalance", 4)
        );
    }

    #[test]
    fn the_caster_table_comes_from_the_ini() {
        let v = values(&[("aicastertable", "0", "5"), ("aicastertable", "53", "263")]);
        let ctx = Ctx::new(&v, no_log);
        let writes = plain(&caster_table(&ctx, 1).unwrap());
        let mut table = vec![8u8; 54];
        table[0] = 5;
        table[53] = 7;
        assert_eq!(
            writes,
            [
                (
                    eix_sites::AI_ATTENTION_SPELL_KIND__13,
                    vec![0x29],
                    vec![0x36]
                ),
                (
                    eix_sites::AI_ATTENTION_SPELL_KIND__1A.back(2),
                    SPELL_KIND_LOOKUP_WAS.to_vec(),
                    vec![0x8A, 0x88, 0x48, 0xEE, 0x5C, 0x00]
                ),
                (
                    eix_sites::AI_ATTENTION_SPELL_KIND__88,
                    SPELL_KIND_TABLE_BYTES.to_vec(),
                    table
                )
            ]
        );
        assert!(caster_table(&ctx, 2).unwrap().is_empty());
        assert!(caster_table(&ctx, 0).unwrap().is_empty());
    }

    #[test]
    fn new_big_tables_changes_neither_table() {
        let big = values(&[
            ("gameplay", "newbigtables", "1"),
            ("aioffendtable", "1", "1"),
        ]);
        let plain_ini = values(&[("aioffendtable", "1", "1")]);
        let big = Ctx::new(&big, no_log);
        let plain_ini = Ctx::new(&plain_ini, no_log);
        assert_eq!(
            plain(&caster_table(&big, 1).unwrap()),
            plain(&caster_table(&plain_ini, 1).unwrap())
        );
        assert_eq!(
            plain(&offend_table(&big, 1).unwrap()),
            plain(&offend_table(&plain_ini, 1).unwrap())
        );
        assert!(new_big_tables(&big, 1).unwrap().is_empty());
    }
}
