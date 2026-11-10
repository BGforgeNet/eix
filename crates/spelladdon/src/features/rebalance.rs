//! Rebalance features.
#![expect(
    unsafe_code,
    reason = "the hook handlers read and write the game objects of the hooked functions"
)]

use core::ffi::CStr;
use std::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

use eix_engine::game::crt;
use eix_engine::game::mfc::{self, GameString};
use eix_engine::game::net::DeltaStream;
use eix_engine::game::ui::{self, DrawTextIn, Rect, Surface, UiMesh, UiWindow};
use eix_engine::game::unit::UnitStats;
use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context};
use eix_engine::x87::Ext;

use super::common::{self, Bytes, FpuSlot, read};
use super::{Build, BuildError, Ctx};

/// A `.rdata` or `.data` float of the game's.
fn game_float(address: u32) -> f32 {
    // SAFETY: the addresses passed are game.exe's own constants and the parameters SpellAddon writes into `.data`.
    unsafe { read::<f32>(address) }
}

/// The implemented features: `(section, key, build)`.
pub(crate) const BUILDS: &[(&str, &str, Build)] = &[
    ("rebalanceengine", "rewriteskillscounting", skills_counting),
    ("rebalanceengine", "rewriterunabilitytime", run_ability_time),
    ("rebalanceengine", "newrunsystem", new_run_system),
    ("animationsspeed", "rewriteactionsspeed", actions_speed),
    ("gameplay", "diffchange", difficulty_change),
    (
        "rebalanceengine",
        "rewriteherolevelcounting",
        level_counting,
    ),
    ("gameplay", "changeexpforint", experience_for_intelligence),
    (
        "rebalanceengine",
        "backstabvisualbase",
        backstab_visual_base,
    ),
    ("rebalanceengine", "difficultyreset", difficulty_reset),
    ("gameplay", "nhits", n_hits),
    ("gameplay", "actionspatch", actions_patch),
    ("gameplay", "speedboost", speed_boost),
    (
        "gameplay",
        "actionsdexteritymovespeed",
        dexterity_move_speed,
    ),
    ("gameplay", "cleveranimationsspeedup", clever_animations),
    ("gameplay", "newperks", new_perks),
    ("gameplay", "newdifficultylevels", new_difficulty_levels),
    ("gameplay", "automaticruncontrol", automatic_run_control),
];

/// `CUnitServer::SyncMoveModes` 0x54A6AD (ECX the unit server, DL the run bit), in place of `or [ecx + 0x175],
/// dl`: when the unit's companion holds 0 (or NaN, which the x87 compare also reports as equal) at +0x34, the low
/// byte of `[ecx + 0x1B0]` is cleared instead; without a companion the bit is set as before.
extern "C" fn run_control(_: usize, ctx: &mut Context) {
    // SAFETY: ECX is the unit server; +0x1C its unit, whose +0x170 is the companion the original reads.
    let unit = unsafe { read::<u32>(ctx.ecx + 0x1C) };
    // SAFETY: as above.
    let companion = unsafe { read::<u32>(unit + COMPANION) };
    let exhausted = companion != 0 && {
        // SAFETY: a live companion's +0x34, as the original reads it.
        let stamina = unsafe { read::<f32>(companion + 0x34) };
        stamina == game_float(ZERO) || stamina.is_nan()
    };
    if exhausted {
        // SAFETY: the unit server's field the original masks.
        unsafe { common::write::<u8>(ctx.ecx + 0x1B0, 0) };
    } else {
        let at = ctx.ecx + 0x175;
        // SAFETY: the byte the replaced instruction sets bits in.
        let modes = unsafe { read::<u8>(at) };
        // SAFETY: as above.
        unsafe { common::write(at, modes | ctx.edx as u8) };
    }
}

/// A map object's companion (`MapObject::companion`).
const COMPANION: u32 = core::mem::offset_of!(eix_engine::game::unit::MapObject, companion) as u32;
/// 0.0 in `.rdata`.
const ZERO: u32 = eix_sites::RDATA_73B858 as u32;
const SYNC_RUN: usize = eix_sites::C_UNIT_SERVER_SYNC_MOVE_MODES__8D;
const SYNC_RUN_WAS: [u8; 6] = [0x08, 0x91, 0x75, 0x01, 0x00, 0x00];

/// `automaticruncontrol` = 1: a unit stops running when its stamina is 0.
fn automatic_run_control(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let stub = common::call_stub(run_control, 0, SYNC_RUN + SYNC_RUN_WAS.len())?;
    Ok(vec![common::branch(
        SYNC_RUN,
        &SYNC_RUN_WAS,
        &[0x90, 0xE8, 0, 0, 0, 0],
        1,
        stub,
    )?])
}

/// A unit's animation speed (`MapObject::animation_speed`), which the net state now carries.
const ANIMATION_SPEED: u32 =
    core::mem::offset_of!(eix_engine::game::unit::MapObject, animation_speed) as u32;

/// The animation speed as a 4-byte field of the stream EDI, for the unit ESI.
fn speed_field(ctx: &Context) -> (*mut DeltaStream, *mut core::ffi::c_void) {
    (
        core::ptr::with_exposed_provenance_mut(ctx.edi as usize),
        core::ptr::with_exposed_provenance_mut((ctx.esi + ANIMATION_SPEED) as usize),
    )
}

/// `UnitWriteNetState` 0x52EE71, after the field write the stub repeats: the unit's animation speed is written too.
extern "C" fn write_speed(_: usize, ctx: &mut Context) {
    let (stream, field) = speed_field(ctx);
    // SAFETY: EDI is the state's delta stream and ESI the unit, as at the original call.
    unsafe { DeltaStream::write_delta(stream, field, 4) };
}

/// `UnitReadNetState` 0x52F3A9: the speed is read back before the replaced `cmp [esi + 0x25C], 1`.
extern "C" fn read_speed(_: usize, ctx: &mut Context) {
    let (stream, field) = speed_field(ctx);
    // SAFETY: as in `write_speed`.
    unsafe { DeltaStream::read_delta(stream, field, 4) };
}

/// `CUnitServer::BuildCastSteps` 0x55071B, after the replaced `fild qword [esp + 0x10]; push eax` (the frame count
/// F stays in ST0): the caster's animation speed and cast factor (`[unit + 0x178]`, `[unit + 0x4B4]`) become
/// `(F / [esp + 0x30] + 1) * 0.5`; EDX is left holding the unit.
extern "C" fn cast_speed(_: usize, ctx: &mut Context) {
    let stack = ctx.stack() as u32;
    // SAFETY: the 64-bit integer the stub just loaded and the divisor the original reads, on the game's stack.
    let frames = unsafe { read::<i64>(stack + 0x14) };
    // SAFETY: as above.
    let divisor = unsafe { read::<i32>(stack + 0x30) };
    // SAFETY: EDI is the unit server; +0x1C its unit.
    let unit = unsafe { read::<u32>(ctx.edi + 0x1C) };
    let speed = Ext::new(frames as f64)
        .fdiv(divisor.into())
        .fadd(1.0)
        .fmul(game_float(HALF).into())
        .to_f32();
    // SAFETY: the unit's two floats the original stores.
    unsafe { common::write(unit + ANIMATION_SPEED, speed) };
    // SAFETY: as above.
    unsafe { common::write(unit + 0x4B4, speed) };
    ctx.edx = unit;
}

/// 0.5 in `.rdata`.
const HALF: u32 = eix_sites::RDATA_73B86C as u32;
const WRITE_STATE: usize = eix_sites::C_UNIT_WRITE_NET_STATE__201;
/// `call DeltaField::WriteDelta`.
const WRITE_STATE_WAS: [u8; 5] = [0xE8, 0x2A, 0xEE, 0xF4, 0xFF];
const READ_STATE: usize = eix_sites::C_UNIT_READ_NET_STATE__419;
/// `cmp dword [esi + 0x25C], 1`.
const READ_STATE_WAS: [u8; 7] = [0x83, 0xBE, 0x5C, 0x02, 0x00, 0x00, 0x01];
const CAST_STEPS: usize = eix_sites::C_UNIT_SERVER_BUILD_CAST_STEPS__FB;
/// `fild qword [esp + 0x10]; push eax`.
const CAST_STEPS_WAS: [u8; 5] = [0xDF, 0x6C, 0x24, 0x10, 0x50];
const CAST_STEPS_GO_ON: usize = eix_sites::C_UNIT_SERVER_BUILD_CAST_STEPS__100;

/// `cleveranimationsspeedup` = 1: units send their animation speed with their state, and casting sets it from the
/// cast's frames; the client applies its time factor for motion mode 1 (as `rewriteactionsspeed`).
fn clever_animations(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let write = common::jump_stub(
        WRITE_STATE,
        &WRITE_STATE_WAS,
        write_speed,
        0,
        WRITE_STATE + 5,
    )?;
    let read = hook::stub(
        READ_STATE,
        &[],
        &READ_STATE_WAS,
        false,
        read_speed,
        0,
        READ_STATE + READ_STATE_WAS.len(),
    )?;
    let cast = common::jump_stub(CAST_STEPS, &CAST_STEPS_WAS, cast_speed, 0, CAST_STEPS_GO_ON)?;
    Ok(vec![
        common::branch(WRITE_STATE, &WRITE_STATE_WAS, &[0xE9, 0, 0, 0, 0], 0, write)?,
        common::branch(
            READ_STATE,
            &READ_STATE_WAS,
            &[0xE9, 0, 0, 0, 0, 0x90, 0x90],
            0,
            read,
        )?,
        common::fixed(&[ADVANCE_TIME_MODE])?.remove(0),
        common::branch(CAST_STEPS, &CAST_STEPS_WAS, &[0xE9, 0, 0, 0, 0], 0, cast)?,
    ])
}

/// `actionsdexteritymovespeed`'s parameters in `CUnit::MoveSpeedFor`'s padding: `actmovemul`, `dexmovemul`.
const ACTIONS_MOVE_MUL: u32 = eix_sites::C_UNIT_MOVE_SPEED_FOR__C8 as u32;
const DEXTERITY_MOVE_MUL: u32 = eix_sites::C_UNIT_MOVE_SPEED_FOR__CC as u32;
/// The stats refresh's value in ST0 at 0x524C79 (the armour term it is about to add 1 to).
static ARMOUR_TERM: FpuSlot = FpuSlot::new();

/// The stats refresh 0x524C79 (EBX the unit stats): the move-speed factor `[ebx + 0x28]` becomes
/// `(term * actmovemul + 1) * factor + (dexterity - 25) * dexmovemul`; then the replaced `mov [ebp + 0xC], 1.0`.
extern "C" fn dexterity_move_factor(_: usize, ctx: &mut Context) {
    let at = ctx.ebx + 0x28;
    // SAFETY: EBX is the `UnitStats` the refresh recomputes; +0x28 its move-speed factor, +0x18 its dexterity.
    let factor = unsafe { read::<f32>(at) };
    // SAFETY: as above.
    let dexterity = unsafe { read::<f32>(ctx.ebx + 0x18) };
    let dex = Ext::new(dexterity.into())
        .fsub(game_float(TWENTY_FIVE).into())
        .fmul(game_float(DEXTERITY_MOVE_MUL).into())
        .to_f64();
    let speed = Ext::new(ARMOUR_TERM.get())
        .fmul(game_float(ACTIONS_MOVE_MUL).into())
        .fadd(game_float(ONE).into())
        .fmul(factor.into())
        .fadd(dex)
        .to_f32();
    // SAFETY: as above; the original stores it there (`fst [ebx + 0x28]`).
    unsafe { common::write(at, speed) };
    // SAFETY: the refresh's local the replaced instruction sets.
    unsafe { common::write(ctx.ebp + 0xC, 1.0f32) };
}

const MOVE_FACTOR: usize = eix_sites::UNIT_STATS_RECOMPUTE__719;
const MOVE_FACTOR_WAS: [u8; 7] = [0xC7, 0x45, 0x0C, 0x00, 0x00, 0x80, 0x3F];
const MOVE_FACTOR_GO_ON: usize = eix_sites::UNIT_STATS_RECOMPUTE__720;
/// game.exe's 25.0 and 1.0 in `.rdata`.
const TWENTY_FIVE: u32 = eix_sites::RDATA_73E124 as u32;
const ONE: u32 = eix_sites::RDATA_73B84C as u32;

/// `actionsdexteritymovespeed` >= 1: actions and dexterity raise the move speed.
fn dexterity_move_speed(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    let f = |key| ctx.values.float("gameplay", key);
    let padding = f32::from_le_bytes([0x90; 4]);
    let stub = common::jump_stub(
        MOVE_FACTOR,
        &ARMOUR_TERM.copy(),
        dexterity_move_factor,
        0,
        MOVE_FACTOR_GO_ON,
    )?;
    Ok(vec![
        common::branch(
            MOVE_FACTOR,
            &MOVE_FACTOR_WAS,
            &[0x90, 0x90, 0xE9, 0, 0, 0, 0],
            2,
            stub,
        )?,
        common::float_at(ACTIONS_MOVE_MUL as usize, padding, f("actmovemul")?)?,
        common::float_at(DEXTERITY_MOVE_MUL as usize, padding, f("dexmovemul")?)?,
    ])
}

/// `speedboost`'s parameters, inside the `.data` region its routine shares with the shops' template code:
/// `speedbase`, `speedadd`.
const SPEED_BASE: u32 = eix_sites::DATA_79A3B5 as u32;
const SPEED_ADD: u32 = eix_sites::DATA_79A3B9 as u32;
/// The speed the site multiplies, moved through memory around the handler.
static SPEED: FpuSlot = FpuSlot::new();
/// `CUnit::GetMoveSpeeds`' factor (1/25.5) and the divisor 15.0, from `.rdata`.
const MOVE_SPEED_FACTOR: u32 = eix_sites::RDATA_73EBC0 as u32;
const FIFTEEN: u32 = eix_sites::RDATA_73C518 as u32;

/// `CUnit::GetMoveSpeeds` 0x52F93A (ECX the unit), in place of `fmul [0x73EBC0]`: the speed times
/// `((counter + speedadd) / 15) ^ speedbase`, where the counter is the unit's byte +0x345, then times the factor.
/// The original loads the counter as a 16-bit word with the byte after it zeroed for the load.
extern "C" fn speed_boost_factor(_: usize, ctx: &mut Context) {
    // SAFETY: ECX is the unit whose speeds the function computes; the original reads its byte +0x345.
    let counter = unsafe { read::<u8>(ctx.ecx + 0x345) };
    let x = Ext::new(counter.into())
        .fadd(game_float(SPEED_ADD).into())
        .fdiv(game_float(FIFTEEN).into())
        .to_f64();
    // SAFETY: the game's `_CIpow`, at a site whose x87 stack holds only the speed, which the stub stored.
    let power = unsafe { crt::ci_pow(x, game_float(SPEED_BASE).into()) };
    SPEED.set(
        Ext::new(SPEED.get())
            .fmul(power)
            .fmul(game_float(MOVE_SPEED_FACTOR).into())
            .to_f64(),
    );
}

const MOVE_SPEEDS: usize = eix_sites::C_UNIT_GET_MOVE_SPEEDS__1A;
const MOVE_SPEEDS_WAS: [u8; 6] = [0xD8, 0x0D, 0xC0, 0xEB, 0x73, 0x00];

/// `speedboost` = 1: units speed up with their counter.
fn speed_boost(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let f = |key| ctx.values.float("gameplay", key);
    let stub = hook::stub(
        MOVE_SPEEDS,
        &SPEED.store(),
        &SPEED.load(),
        true,
        speed_boost_factor,
        0,
        MOVE_SPEEDS + MOVE_SPEEDS_WAS.len(),
    )?;
    Ok(vec![
        common::float_at(SPEED_BASE as usize, 0.0, f("speedbase")?)?,
        common::float_at(SPEED_ADD as usize, 0.0, f("speedadd")?)?,
        common::branch(
            MOVE_SPEEDS,
            &MOVE_SPEEDS_WAS,
            &[0x90, 0xE8, 0, 0, 0, 0],
            1,
            stub,
        )?,
    ])
}

/// `SchoolPrice` 0x527FF0, the price of the next skill level from the level in ECX: the original's two powers of
/// 1.1 (`_CIpow` 0x6E81A0) and their difference.
const SCHOOL_PRICE: usize = eix_sites::SCHOOL_PRICE;
const SCHOOL_PRICE_WAS: [u8; 96] = [
    0x83, 0xEC, 0x08, 0xDD, 0x05, 0x40, 0xE1, 0x73, 0x00, 0x89, 0x54, 0x24, 0x00, 0x89, 0x4C, 0x24,
    0x04, 0xDB, 0x44, 0x24, 0x00, 0xD8, 0x35, 0x80, 0x54, 0x7C, 0x00, 0xE8, 0x90, 0x01, 0x1C, 0x00,
    0xD8, 0x0D, 0x7C, 0x54, 0x7C, 0x00, 0xD9, 0x54, 0x24, 0x00, 0xDD, 0x05, 0x40, 0xE1, 0x73, 0x00,
    0xDB, 0x44, 0x24, 0x04, 0xD8, 0x35, 0x80, 0x54, 0x7C, 0x00, 0xE8, 0x71, 0x01, 0x1C, 0x00, 0xD8,
    0x0D, 0x7C, 0x54, 0x7C, 0x00, 0xD9, 0x54, 0x24, 0x04, 0xDE, 0xE9, 0xE8, 0xFC, 0x04, 0x1C, 0x00,
    0x83, 0xC4, 0x08, 0xC3, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
];
/// Where the rewritten price keeps `pow` (an `f64`), `mul` and `add`: the 16 bytes it jumps over.
const SCHOOL_PRICE_DATA: usize = 0x10;

/// The rewritten `SchoolPrice`: a new head up to 0x52802B, NOPs up to the original's `fst [esp + 4]` at 0x528035,
/// its `fsubp` 0x528039 dropped, then the original's `_ftol` call and return.
fn school_price(pow: f64, mul: f32, add: f32) -> [u8; 96] {
    let mut code = SCHOOL_PRICE_WAS;
    code[..0x45].fill(0x90);
    code[0x49..0x4B].fill(0x90);
    let parts: [&[u8]; 9] = [
        &[0x83, 0xEC, 0x08],             // sub esp, 8
        &[0x89, 0x4C, 0x24, 0x04],       // mov [esp + 4], ecx
        &[0x90; 4],                      //
        &[0xE9, 0x10, 0x00, 0x00, 0x00], // jmp 0x528010, over the data
        &[0; 16],
        &[0xDD, 0x05, 0x00, 0x80, 0x52, 0x00], // fld qword [0x528000]: pow
        &[0xDB, 0x44, 0x24, 0x04],             // fild dword [esp + 4]: the level
        &[0xE8, 0x81, 0x01, 0x1C, 0x00],       // call _CIpow: pow ^ level
        &[
            0xD8, 0x0D, 0x08, 0x80, 0x52, 0x00, // fmul [0x528008]: mul
            0xD8, 0x05, 0x0C, 0x80, 0x52, 0x00, // fadd [0x52800C]: add
        ],
    ];
    let mut at = 0;
    for p in parts {
        code[at..at + p.len()].copy_from_slice(p);
        at += p.len();
    }
    let data = SCHOOL_PRICE_DATA;
    code[data..data + 8].copy_from_slice(&pow.to_le_bytes());
    code[data + 8..data + 12].copy_from_slice(&mul.to_le_bytes());
    code[data + 12..data + 16].copy_from_slice(&add.to_le_bytes());
    code
}

/// `rewriteskillscounting` = 1: a skill level costs `pow ^ level * mul + add`.
fn skills_counting(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let v = ctx.values;
    let code = school_price(
        v.double("rebalanceengine", "pow")?,
        v.float("rebalanceengine", "mul")?,
        v.float("rebalanceengine", "add")?,
    );
    Ok(vec![SiteWrite::bytes(
        SCHOOL_PRICE,
        &SCHOOL_PRICE_WAS,
        &code,
    )?])
}

/// The running stamina constant `UpdateMotion` multiplies by (1/150), an `f64` in `.rdata`.
const RUN_CONSTANT: usize = eix_sites::RDATA_73F088;
const RUN_CONSTANT_WAS: f64 = f64::from_bits(0x3F7B_4E81_B4E8_1B4E);
/// The padding after `CArray<AnimationStep>`'s assignment that holds `runconstex` (an `f64`).
const RUN_CONSTANT_EX: usize = eix_sites::TEXT_548518;

fn run_constant_ex(ctx: &Ctx<'_>) -> Result<SiteWrite, BuildError> {
    Ok(SiteWrite::bytes(
        RUN_CONSTANT_EX,
        &[0x90; 8],
        &ctx.values
            .double("rebalanceengine", "runconstex")?
            .to_le_bytes(),
    )?)
}

/// `rewriterunabilitytime` = 1: the stamina constant becomes `runconst`; `runconstex` goes to its padding.
fn run_ability_time(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![
        SiteWrite::bytes(
            RUN_CONSTANT,
            &RUN_CONSTANT_WAS.to_le_bytes(),
            &ctx.values
                .double("rebalanceengine", "runconst")?
                .to_le_bytes(),
        )?,
        run_constant_ex(ctx)?,
    ])
}

/// `UpdateMotion`'s stamina drain `fld [edi + 0x14]; fld [edi + 0x18]; fmul qword [0x73F088]; fsubp st(1)`: value 1
/// drops the maximum and its product (`stamina - c`).
const DRAIN_ABSOLUTE: Bytes = (
    eix_sites::UPDATE_MOTION__A85,
    &[
        0xD9, 0x47, 0x14, 0xD9, 0x47, 0x18, 0xDC, 0x0D, 0x88, 0xF0, 0x73, 0x00, 0xDE, 0xE9,
    ],
    &[
        0xD9, 0x47, 0x14, 0x90, 0x90, 0x90, 0x90, 0x90, 0xDC, 0x25, 0x88, 0xF0, 0x73, 0x00,
    ],
);
/// Value 2: the multiply becomes `nop; call 0x5483B2`, a helper in `UpdateMotion`'s padding that multiplies and
/// adds `runconstex` (`fmul qword [0x73F088]; fadd qword [0x548518]; ret`).
const DRAIN_HELPER_CALL: Bytes = (
    eix_sites::UPDATE_MOTION__A8B,
    &[0xDC, 0x0D, 0x88, 0xF0, 0x73, 0x00],
    &[0x90, 0xE8, 0x91, 0x00, 0x00, 0x00],
);
const DRAIN_HELPER: Bytes = (
    eix_sites::UPDATE_MOTION__B22,
    &[0x90; 14],
    &[
        0xDC, 0x0D, 0x88, 0xF0, 0x73, 0x00, 0xDC, 0x05, 0x18, 0x85, 0x54, 0x00, 0xC3, 0x90,
    ],
);

/// `newrunsystem` 1 or 2: running drains a fixed amount, or the original amount plus `runconstex`.
fn new_run_system(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    match value {
        1 => common::fixed(&[DRAIN_ABSOLUTE]),
        2 => {
            let mut writes = common::fixed(&[DRAIN_HELPER, DRAIN_HELPER_CALL])?;
            writes.push(run_constant_ex(ctx)?);
            Ok(writes)
        }
        _ => Ok(Vec::new()),
    }
}

/// `CUnit::AnimDurationFor`'s six factor loads, each `fld dword [...]` becoming a load from the table.
const ANIMATION_LOADS: [(usize, [u8; 6]); 6] = [
    (
        eix_sites::C_UNIT_ACTION_SPEED__13,
        [0xD9, 0x81, 0xB0, 0x04, 0x00, 0x00],
    ), // attack, [ecx + 0x4B0]
    (
        eix_sites::C_UNIT_ACTION_SPEED__23,
        [0xD9, 0x81, 0xB4, 0x04, 0x00, 0x00],
    ), // cast
    (
        eix_sites::C_UNIT_ACTION_SPEED__33,
        [0xD9, 0x81, 0xB8, 0x04, 0x00, 0x00],
    ), // hit
    (
        eix_sites::C_UNIT_ACTION_SPEED__43,
        [0xD9, 0x81, 0xBC, 0x04, 0x00, 0x00],
    ), // death
    (
        eix_sites::C_UNIT_ACTION_SPEED__72,
        [0xD9, 0x81, 0xC4, 0x04, 0x00, 0x00],
    ), // special
    (
        eix_sites::C_UNIT_ACTION_SPEED__7B,
        [0xD9, 0x05, 0x4C, 0xB8, 0x73, 0x00],
    ), // idle, the constant 1.0
];
/// The `.data` table of the six INI factors, in that order.
const ANIMATION_TABLE: usize = eix_sites::DATA_799C50;
const ANIMATION_KEYS: [&str; 6] = [
    "attackspeed",
    "castspeed",
    "hitspeed",
    "deathspeed",
    "specialspeed",
    "idlespeed",
];
/// `CUnitClientSpecific::AdvanceTime`'s `cmp eax, 2; jne +0x12` becomes `cmp eax, 1; je +0x12`.
const ADVANCE_TIME_MODE: Bytes = (
    eix_sites::C_UNIT_CLIENT_SPECIFIC_ADVANCE_TIME__141,
    &[0x83, 0xF8, 0x02, 0x75, 0x12],
    &[0x83, 0xF8, 0x01, 0x74, 0x12],
);

/// The site of `ANIMATION_LOADS[i]` and the load there with `rewriteactionsspeed` = `value`: the table's entry for 1,
/// else the game's own.
pub(super) fn animation_load(value: i8, i: usize) -> (usize, [u8; 6]) {
    let (site, was) = ANIMATION_LOADS[i];
    if value != 1 {
        return (site, was);
    }
    let mut now = [0xD9, 0x05, 0, 0, 0, 0];
    now[2..].copy_from_slice(&((ANIMATION_TABLE + 4 * i) as u32).to_le_bytes());
    (site, now)
}

/// `rewriteactionsspeed` = 1: every unit's animation factors come from the INI.
fn actions_speed(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let mut writes = common::fixed(&[ADVANCE_TIME_MODE])?;
    for (i, (site, was)) in ANIMATION_LOADS.iter().enumerate() {
        writes.push(SiteWrite::bytes(*site, was, &animation_load(value, i).1)?);
    }
    for (i, key) in ANIMATION_KEYS.iter().enumerate() {
        writes.push(common::float_at(
            ANIMATION_TABLE + 4 * i,
            0.0,
            ctx.values.float("animationsspeed", key)?,
        )?);
    }
    Ok(writes)
}

/// `SpinLeftDown`'s `lea ecx, [esi - 0x94]`, before the spinner's `inc eax; push eax; call CUiSpin::SetValue`.
const SPIN_LEFT_DOWN: usize = eix_sites::SPIN_LEFT_DOWN__18;
const SPIN_LEFT_DOWN_WAS: [u8; 6] = [0x8D, 0x8E, 0x6C, 0xFF, 0xFF, 0xFF];
/// `nop`, then a jump whose displacement fills the rest (the bytes after `0xE9` are a placeholder).
const SPIN_LEFT_DOWN_NOW: [u8; 6] = [0x90, 0xE9, 0, 0, 0, 0];
/// After the `lea`, and `SpinLeftDown`'s `pop esi; ret 0xC`, which skips the click.
const SPIN_LEFT_DOWN_GO_ON: usize = eix_sites::SPIN_LEFT_DOWN__1E;
const SPIN_LEFT_DOWN_SKIP: usize = eix_sites::SPIN_LEFT_DOWN__25;
/// The difficulty spinner's control id.
const DIFFICULTY_SPINNER: u32 = 0x19;

extern "C" fn spin_left_down(_: usize, ctx: &mut Context) {
    ctx.ecx = ctx.esi.wrapping_sub(0x94);
    ctx.resume = if ctx.edi == DIFFICULTY_SPINNER {
        SPIN_LEFT_DOWN_SKIP
    } else {
        SPIN_LEFT_DOWN_GO_ON
    };
}

/// `diffchange` = 1: a click on the difficulty spinner of the game options does nothing (the original's code in
/// game.exe's header, 0x400380).
fn difficulty_change(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let stub = common::jump_stub(SPIN_LEFT_DOWN, &[], spin_left_down, 0, SPIN_LEFT_DOWN_GO_ON)?;
    Ok(vec![common::branch(
        SPIN_LEFT_DOWN,
        &SPIN_LEFT_DOWN_WAS,
        &SPIN_LEFT_DOWN_NOW,
        1,
        stub,
    )?])
}

/// `CUnitClientSpecific::Level` from 0x539170: `round(ln(exp / 200) / ln(1.61))` of the experience `[eax + 0x334]`
/// loaded as a 64-bit integer.
const CLIENT_LEVEL: usize = eix_sites::C_UNIT_CLIENT_SPECIFIC_LEVEL__10;
const CLIENT_LEVEL_WAS: [u8; 48] = [
    0x8B, 0x88, 0x34, 0x03, 0x00, 0x00, 0x89, 0x4D, 0xF4, 0xDF, 0x6D, 0xF4, 0xD8, 0x35, 0x88, 0x54,
    0x7C, 0x00, 0xD9, 0xED, 0xD9, 0xC9, 0xD9, 0xF1, 0xD9, 0xED, 0xD9, 0x05, 0x8C, 0x54, 0x7C, 0x00,
    0xD9, 0xF1, 0xDE, 0xF9, 0xD9, 0x5D, 0xFC, 0xD9, 0x45, 0xFC, 0xDB, 0x5D, 0xF8, 0x8B, 0x45, 0xF8,
];
/// `CUnitServer::Level` 0x556760, the same of the experience `[ecx + 0x1E4]`.
const SERVER_LEVEL: usize = eix_sites::C_UNIT_SERVER_LEVEL;
const SERVER_LEVEL_WAS: [u8; 64] = [
    0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x08, 0xD9, 0x81, 0xE4, 0x01, 0x00, 0x00, 0xD8, 0x35, 0x88, 0x54,
    0x7C, 0x00, 0xD9, 0xED, 0xD9, 0xC9, 0xD9, 0xF1, 0xD9, 0xED, 0xD9, 0x05, 0x8C, 0x54, 0x7C, 0x00,
    0xD9, 0xF1, 0xDE, 0xF9, 0xD9, 0x5D, 0xFC, 0xD9, 0x45, 0xFC, 0xDB, 0x5D, 0xF8, 0x8B, 0x45, 0xF8,
    0x8B, 0xE5, 0x5D, 0xC3, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
];
/// The new formula's `fdiv [0x79A910]; fadd [0x79A91C]; fyl2x; fmul [0x79A914]; fadd [0x79A918]; fst [ebp - 4];
/// fistp [ebp - 8]`, after `fld1` and the experience load.
const LEVEL_FORMULA: [u8; 32] = [
    0xD8, 0x35, 0x10, 0xA9, 0x79, 0x00, 0xD8, 0x05, 0x1C, 0xA9, 0x79, 0x00, 0xD9, 0xF1, 0xD8, 0x0D,
    0x14, 0xA9, 0x79, 0x00, 0xD8, 0x05, 0x18, 0xA9, 0x79, 0x00, 0xD9, 0x55, 0xFC, 0xDB, 0x5D, 0xF8,
];
/// The `.data` words the formula reads: `divlvla`, `mullvla`, `addlvlb`, `addlvla`.
const LEVEL_PARAMETERS: usize = eix_sites::DATA_79A910;

/// The client copy: `fld1; fild dword [eax + 0x334]`, the formula, NOPs, the original's `mov eax, [ebp - 8]`.
fn client_level() -> [u8; 48] {
    let mut code = [0x90; 48];
    code[..8].copy_from_slice(&[0xD9, 0xE8, 0xDB, 0x80, 0x34, 0x03, 0x00, 0x00]);
    code[8..40].copy_from_slice(&LEVEL_FORMULA);
    code[45..].copy_from_slice(&[0x8B, 0x45, 0xF8]);
    code
}

/// The server copy: the original's frame setup, `fld1; fld dword [ecx + 0x1E4]`, the formula, `mov eax, [ebp - 8];
/// mov esp, ebp; pop ebp; ret`, NOPs.
fn server_level() -> [u8; 64] {
    let mut code = [0x90; 64];
    code[..6].copy_from_slice(&SERVER_LEVEL_WAS[..6]);
    code[6..14].copy_from_slice(&[0xD9, 0xE8, 0xD9, 0x81, 0xE4, 0x01, 0x00, 0x00]);
    code[14..46].copy_from_slice(&LEVEL_FORMULA);
    code[46..53].copy_from_slice(&[0x8B, 0x45, 0xF8, 0x8B, 0xE5, 0x5D, 0xC3]);
    code
}

/// `rewriteherolevelcounting` = 1: `level = round(mullvla * log2(exp / divlvla + addlvla) + addlvlb)` in both
/// copies. Value 2 is skipped by the registry: its original drops the client copy's store and leaves the FPU stack
/// one deeper per call.
fn level_counting(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let f = |key| ctx.values.float("rebalanceengine", key);
    let mut writes = vec![
        SiteWrite::bytes(CLIENT_LEVEL, &CLIENT_LEVEL_WAS, &client_level())?,
        SiteWrite::bytes(SERVER_LEVEL, &SERVER_LEVEL_WAS, &server_level())?,
    ];
    for (i, key) in ["divlvla", "mullvla", "addlvlb", "addlvla"]
        .into_iter()
        .enumerate()
    {
        writes.push(common::float_at(LEVEL_PARAMETERS + 4 * i, 0.0, f(key)?)?);
    }
    Ok(writes)
}

/// The experience factor per point of intelligence, 1.04 as an `f64` in `.rdata`.
const EXPERIENCE_FACTOR: usize = eix_sites::RDATA_73E128;

/// `changeexpforint` = 1: the factor becomes `expforint`.
fn experience_for_intelligence(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![SiteWrite::bytes(
        EXPERIENCE_FACTOR,
        &1.04f64.to_le_bytes(),
        &ctx.values.double("gameplay", "expforint")?.to_le_bytes(),
    )?])
}

/// The backstab percentage `CItemInfoPanel::DescribeStats` shows: the immediate 300 at 0x5F1EB0.
const BACKSTAB_BASE: usize = eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_STATS__920;

/// `backstabvisualbase` = 1: the panel shows `bsbase`.
fn backstab_visual_base(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![SiteWrite::bytes(
        BACKSTAB_BASE,
        &300i32.to_le_bytes(),
        &ctx.values.int("rebalanceengine", "bsbase")?.to_le_bytes(),
    )?])
}

/// The `je` of both `GetAttackParams` copies that skips the difficulty scaling for units with a text key.
const DIFFICULTY_FOR_ALL: &[Bytes] = &[
    (
        eix_sites::C_UNIT_SERVER_GET_ATTACK_PARAMS__2D,
        &[0x0F, 0x84, 0xA7, 0x00, 0x00, 0x00],
        &[0x90; 6],
    ),
    (
        eix_sites::C_UNIT_CLIENT_SPECIFIC_GET_ATTACK_PARAMS__17,
        &[0x0F, 0x84, 0xED, 0x00, 0x00, 0x00],
        &[0x90; 6],
    ),
];
/// `AttackParams::ScaleForDifficulty`'s two `fmul [reg*4 + table]` become `fadd`s (the ModR/M byte).
const DIFFICULTY_ADDS: &[Bytes] = &[
    (
        eix_sites::ATTACK_PARAMS_SCALE_FOR_DIFFICULTY__14,
        &[0x0C],
        &[0x04],
    ),
    (
        eix_sites::ATTACK_PARAMS_SCALE_FOR_DIFFICULTY__2C,
        &[0x0C],
        &[0x04],
    ),
];

/// `difficultyreset` 1 or 2: every unit is scaled for difficulty; 2 adds the tables instead of multiplying.
///
/// Both values of the original also write a text-key test function (`1000000000 <= ecx < 2000000000`) to 0x79A280
/// that no code calls; it is left out.
fn difficulty_reset(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    match value {
        1 => common::fixed(DIFFICULTY_FOR_ALL),
        2 => common::fixed(&[DIFFICULTY_FOR_ALL, DIFFICULTY_ADDS].concat()),
        _ => Ok(Vec::new()),
    }
}

/// `CUnitServer::PlanAction`: the hit count from `[ebp - 0x10]` uses 5 instead (`mov ecx, 5`), and the following
/// `fld [0x73EEE4]` becomes `fld1`.
const HITS_PLAN: Bytes = (
    eix_sites::C_UNIT_SERVER_PLAN_ACTION__308,
    &[
        0x8B, 0x4D, 0xF0, 0x99, 0x2B, 0xC2, 0xD1, 0xF8, 0x03, 0xC1, 0x89, 0x45, 0xC8, 0xDB, 0x45,
        0xC8, 0xD9, 0x05, 0xE4, 0xEE, 0x73, 0x00,
    ],
    &[
        0xB9, 0x05, 0x00, 0x00, 0x00, 0x99, 0x2B, 0xC2, 0xD1, 0xF8, 0x03, 0xC1, 0x89, 0x45, 0xC8,
        0xDB, 0x45, 0xC8, 0xD9, 0xE8, 0x90, 0x90,
    ],
);
/// The second use: `mov ecx, [ebp - 0x10]; mov edx, [ebx]; add eax, ecx` becomes `add eax, 5; mov edx, [ebx]`.
const HITS_SECOND: Bytes = (
    eix_sites::C_UNIT_SERVER_PLAN_ACTION__441,
    &[0x8B, 0x4D, 0xF0, 0x8B, 0x13, 0x03, 0xC1],
    &[0x83, 0xC0, 0x05, 0x8B, 0x13, 0x90, 0x90],
);

/// `nhits` = 1: a fixed hit count for the Zhamevu animations.
fn n_hits(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    common::fixed(&[HITS_PLAN, HITS_SECOND])
}

/// The header slack that holds `actionsmul` and `actionsadd`.
const ACTIONS_MUL: usize = 0x40_02D0;
const ACTIONS_ADD: usize = 0x40_02D4;
/// The stats refresh's `fmul [0x73BAD4]; fadd [0x73BAAC]` (0.2 and 10) point at the header slack.
const ACTIONS_FORMULA: Bytes = (
    eix_sites::UNIT_STATS_RECOMPUTE__698,
    &[
        0xD8, 0x0D, 0xD4, 0xBA, 0x73, 0x00, 0xD8, 0x05, 0xAC, 0xBA, 0x73, 0x00,
    ],
    &[
        0xD8, 0x0D, 0xD0, 0x02, 0x40, 0x00, 0xD8, 0x05, 0xD4, 0x02, 0x40, 0x00,
    ],
);

/// The site of [`ACTIONS_FORMULA`]'s `fadd` and its bytes with `actionspatch` = `value`.
pub(super) fn actions_add(value: i8) -> (usize, [u8; 6]) {
    let (site, was, now) = ACTIONS_FORMULA;
    let mut add = [0; 6];
    add.copy_from_slice(&if value == 1 { now } else { was }[6..]);
    (site + 6, add)
}

/// `actionspatch` = 1: actions are `dexterity * actionsmul + actionsadd`.
fn actions_patch(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let f = |key| ctx.values.float("gameplay", key);
    let mut writes = vec![
        common::float_at(ACTIONS_MUL, 0.0, f("actionsmul")?)?,
        common::float_at(ACTIONS_ADD, 0.0, f("actionsadd")?)?,
    ];
    writes.extend(common::fixed(&[ACTIONS_FORMULA])?);
    Ok(writes)
}

/// The two perks `newperks` adds, in the ids the game's own perks 0x17-0x19 leave free when they move up by two.
const ENDURANCE: u32 = 0x17;
const GHOSTLINESS: u32 = 0x18;
/// `UnitStats::conspicuity[0].current` (ei-rebuild's `UnitStats`): how visible the unit is.
const VISIBILITY: u32 = 0xE4;
/// 0.01 in `.rdata`, the factor Ghostliness's level value goes through.
const PERCENT: u32 = eix_sites::RDATA_73D09C as u32;

/// The value of the unit's level of `perk` as the original reads it, the level record's +0x34 as an unsigned integer
/// (`fild qword` with a zero high half), or `None` without a level or with a value of 0.
fn perk_level(stats: u32, perk: u32) -> Option<f64> {
    let stats = core::ptr::with_exposed_provenance_mut::<UnitStats>(stats as usize);
    // SAFETY: EBX is the unit's statistics in the refresh, on the game's thread; the perk ids index its levels.
    let record = unsafe { UnitStats::skill_level_record(stats, perk) };
    if record.is_null() {
        return None;
    }
    // SAFETY: a level record of the game's database, whose +0x34 is its value.
    let value = unsafe { read::<u32>(record as u32 + 0x34) };
    (value != 0).then_some(f64::from(value))
}

/// `UnitStats::Recompute` 0x52483F, after its store of the visibility (the stub repeats it): Endurance's level
/// value is added to the current protection against each of the seven damage types (ESI the vitals).
extern "C" fn endurance(_: usize, ctx: &mut Context) {
    let Some(value) = perk_level(ctx.ebx, ENDURANCE) else {
        return;
    };
    for i in 0..7u32 {
        let at = ctx.esi + 0x24 + 8 * i;
        // SAFETY: the vitals' protection pairs (`UnitVitals::protection[i].current`), which the refresh just set.
        let current = unsafe { read::<f32>(at) };
        // SAFETY: as above.
        unsafe { common::write(at, Ext::new(current.into()).fadd(value).to_f32()) };
    }
}

/// `UnitStats::Recompute` 0x52484E, after the replaced `mov [ebp - 4], 0`: Ghostliness's level value times 0.01 is
/// taken from the visibility, held at 0 as the refresh holds it, and from the loudness the refresh is computing (on
/// the x87 stack, before its own hold at 0 and its store). The original takes it from the stored loudness, which the
/// refresh then overwrites, and leaves the visibility below 0.
extern "C" fn ghostliness(_: usize, ctx: &mut Context) {
    let Some(value) = perk_level(ctx.ebx, GHOSTLINESS) else {
        return;
    };
    let less = Ext::new(value).fmul(game_float(PERCENT).into());
    common::set_st0(Ext::new(common::st0().into()).fsub_ext(less).to_f32());
    let at = ctx.ebx + VISIBILITY;
    // SAFETY: the statistics' visibility, which the refresh stored just before.
    let visible = Ext::new(unsafe { read::<f32>(at) }.into())
        .fsub_ext(less)
        .to_f32();
    // SAFETY: as above.
    unsafe { common::write(at, if 0.0 < visible { visible } else { 0.0 }) };
}

/// The refresh's store of the visibility, `fstp dword [ebx + 0xE4]`, and the clear after the loudness.
const ENDURANCE_SITE: usize = eix_sites::UNIT_STATS_RECOMPUTE__2DF;
const ENDURANCE_WAS: [u8; 6] = [0xD9, 0x9B, 0xE4, 0x00, 0x00, 0x00];
const GHOSTLINESS_SITE: usize = eix_sites::UNIT_STATS_RECOMPUTE__2EE;
const GHOSTLINESS_WAS: [u8; 7] = [0xC7, 0x45, 0xFC, 0x00, 0x00, 0x00, 0x00];

/// `newperks` >= 1: the game's perk ids from 0x17 up move up by two, in the stats refresh's lookups, the table
/// offsets of `UnitStats_Recompute`, the learnable range, the info panel's ranges and its byte switch (two more
/// entries in the padding after it). `SpellSchoolSkill` no longer counts schools 4, 6 and 7 with the skills of 3
/// and 5, nor does the panel set them apart.
const PERKS: &[Bytes] = &[
    (eix_sites::UNIT_STATS_RECOMPUTE__8F, &[0x17], &[0x19]),
    (eix_sites::UNIT_STATS_RECOMPUTE__B9, &[0x18], &[0x1A]),
    (eix_sites::UNIT_STATS_RECOMPUTE__E0, &[0x19], &[0x1B]),
    (eix_sites::UNIT_STATS_RESET_TO_BASE__84, &[0xEA], &[0xEC]),
    (eix_sites::UNIT_STATS_RESET_TO_BASE__98, &[0x44], &[0x4A]),
    (eix_sites::UNIT_STATS_RESET_TO_BASE__BF, &[0xEB], &[0xED]),
    (eix_sites::UNIT_STATS_RESET_TO_BASE__D3, &[0x47], &[0x4D]),
    (eix_sites::UNIT_STATS_RESET_TO_BASE__FA, &[0xEC], &[0xEE]),
    (eix_sites::UNIT_STATS_RESET_TO_BASE__10E, &[0x4A], &[0x50]),
    (eix_sites::UNIT_STATS_CAN_LEARN_PERK__1A2, &[0x1A], &[0x1C]),
    (eix_sites::UNIT_STATS_CAN_LEARN_PERK__34A, &[0x1A], &[0x1C]),
    (
        eix_sites::UNIT_STATS_SPELL_SCHOOL_SKILL__7,
        &[
            0x83, 0xF8, 0x04, 0x75, 0x07, 0xB8, 0x03, 0x00, 0x00, 0x00, 0xEB, 0x0F, 0x83, 0xF8,
            0x06, 0x74, 0x05, 0x83, 0xF8, 0x07, 0x75, 0x05, 0xB8, 0x05, 0x00, 0x00, 0x00,
        ],
        &[0x90; 27],
    ),
    (eix_sites::C_ITEM_INFO_PANEL_BUILD__EB, &[0x2D], &[0x33]),
    (
        eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_STATS__7EB,
        &[0x19],
        &[0x1B],
    ),
    (
        eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_STATS__116A,
        &[
            0x83, 0xF8, 0x04, 0x0F, 0x84, 0xF2, 0x00, 0x00, 0x00, 0x83, 0xF8, 0x06, 0x0F, 0x84,
            0xE9, 0x00, 0x00, 0x00, 0x83, 0xF8, 0x07, 0x0F, 0x84, 0xE0, 0x00, 0x00, 0x00,
        ],
        &[0x90; 27],
    ),
    (
        eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_STATS__137E,
        &[0x90, 0x90],
        &[0x0A, 0x0A],
    ),
];
/// Value 2 also reads a spell's other school field in the info panel and the spell column (`+0x14` for `+0x10`).
const PERKS_SCHOOLS: &[Bytes] = &[
    (
        eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_CARRIED_SPELL__25F,
        &[0x10],
        &[0x14],
    ),
    (
        eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_SPELL_RECORD__1E5,
        &[0x10],
        &[0x14],
    ),
    (
        eix_sites::C_HUD_SPELL_COLUMN_SET_UNIT__15C,
        &[0x10],
        &[0x14],
    ),
];

/// `newperks` >= 1: the perks Endurance (armour against every damage type) and Ghostliness (less visible); value 2
/// renames the magic schools in the info text.
fn new_perks(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    let mut writes = common::fixed(PERKS)?;
    writes.push(common::branch_before(
        ENDURANCE_SITE,
        &ENDURANCE_WAS,
        &[0x90, 0xE8, 0, 0, 0, 0],
        1,
        endurance,
        0,
    )?);
    writes.push(common::branch_before(
        GHOSTLINESS_SITE,
        &GHOSTLINESS_WAS,
        &[0x90, 0x90, 0xE8, 0, 0, 0, 0],
        2,
        ghostliness,
        0,
    )?);
    if value == 2 {
        writes.extend(common::fixed(PERKS_SCHOOLS)?);
    }
    Ok(writes)
}

/// `CDifficultyBox::Build` 0x6337B0 (the new game's difficulty question): its `CString::TrimRight("\r\n")` of the
/// explanation before the explanation is measured, which value 1 drops.
const EXPLANATION_TRIM: (usize, &[u8]) = (
    eix_sites::C_DIFFICULTY_BOX_BUILD__FD,
    &[
        0x68, 0x00, 0xDD, 0x78, 0x00, 0x8D, 0x4C, 0x24, 0x18, 0xE8, 0xCF, 0xA7, 0x0C, 0x00,
    ],
);
/// The build's `lea ecx, [edi + ebx + 0x6C]; mov eax, 0x226`: ECX becomes the box's height `B`, EAX the 550 it is
/// centred in. Ours makes `B` a row taller for the hard row (the original's box keeps the game's two-row height);
/// the sum does not fit the `lea`'s byte displacement, so a stub adds the row.
const BOX_HEIGHT: usize = eix_sites::C_DIFFICULTY_BOX_BUILD__149;
const BOX_HEIGHT_WAS: [u8; 9] = [0x8D, 0x4C, 0x1F, 0x6C, 0xB8, 0x26, 0x02, 0x00, 0x00];
/// The displacement of the explanation's top, `lea edx, [ebx + edi - 7]` (title height plus the box's top, on the
/// box's surface): a row lower, below the hard row, where the original draws it over the hard label.
const EXPLANATION_TOP: Bytes = (eix_sites::C_DIFFICULTY_BOX_BUILD__1B7, &[0xF9], &[0x11]);
/// The build's `mov ecx, [0x7AF788]` (the text manager) before it reads the labels: the hard label is read there.
const LABELS_READ: usize = eix_sites::C_DIFFICULTY_BOX_BUILD__1E9;
const LABELS_READ_WAS: [u8; 6] = [0x8B, 0x0D, 0x88, 0xF7, 0x7A, 0x00];
/// The text manager, and its `GetString` 0x4E0020 (thiscall: the key and the `CString` it assigns, `ret 8`).
const TEXTS: u32 = eix_sites::DATA_7AF788 as u32;
const GET_STRING: usize = eix_sites::C_TEXT_MANAGER_GET_STRING;
/// The operand of the second label's `push 0x79427C` ("difficulty_hard"): the normal label takes its place, so the
/// build measures and draws it as its second label and the box's width counts easy and normal.
const SECOND_KEY: usize = eix_sites::C_DIFFICULTY_BOX_BUILD__20F;
const HARD_KEY: u32 = eix_sites::DATA_79427C as u32;
static NORMAL_KEY: &CStr = c"difficulty_norm";
/// The build's `mov eax, [esp + 0x4C]; push 1` after it draws the easy label: EAX becomes the labels' left edge, EBX
/// is the first check box's row, and the surface is the local the build reads at the site's `[esp + 0x10]`.
const LABELS_DRAW: usize = eix_sites::C_DIFFICULTY_BOX_BUILD__2C4;
const LABELS_DRAW_WAS: [u8; 6] = [0x8B, 0x44, 0x24, 0x4C, 0x6A, 0x01];
/// The labels' font, colour, format and last argument, as the build draws its own.
const LABEL_FONT: i32 = 2;
const LABEL_COLOUR: u32 = 0xA7_D7E4;
/// `mov [esi + 0x180], 0`, the selection the box starts with (0, hard), becomes 1 (normal).
const START_SELECTION: Bytes = (eix_sites::C_DIFFICULTY_BOX_BUILD__314, &[0x00], &[0x01]);
/// The build's `lea edx, [ebx + 0x2D]; push 0x4C` after the second check box's `AddSprite`: EAX is that sprite, ESI
/// the box, EBP `half`, EBX the row; EDX becomes the bottom of the next box the build shapes.
const BOXES: usize = eix_sites::C_DIFFICULTY_BOX_BUILD__38C;
const BOXES_WAS: [u8; 5] = [0x8D, 0x53, 0x2D, 0x6A, 0x4C];
/// The build's next box is the hard one, a row lower (its top `lea eax, [ebx + 0x18]`), and no longer checked at
/// the start (`mov [ebp + 0xCC], 0xBDB80000`).
const HARD_BOX_TOP: Bytes = (eix_sites::C_DIFFICULTY_BOX_BUILD__3B5, &[0x18], &[0x30]);
const HARD_BOX_MARK: (usize, &[u8]) = (
    eix_sites::C_DIFFICULTY_BOX_BUILD__3D9,
    &[0xC7, 0x85, 0xCC, 0x00, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xBD],
);
/// The check boxes' sprite: depth, scale and the texels of the box texture, as the build shapes its own.
const BOX_DEPTH: f32 = 2.5;
const BOX_SCALE: f32 = 100.0;
const BOX_TEXELS: [i32; 4] = [209, 55, 230, 76];
/// The build's `mov edx, [esp + 0x3C]; mov ecx, [esp + 0x4C]` after the second check box's `AddRegion`: EAX is that
/// region, ESI the box; EBX, ECX, EDI and EDX become its rectangle, which the build then sets.
const REGIONS: usize = eix_sites::C_DIFFICULTY_BOX_BUILD__5BF;
const REGIONS_WAS: [u8; 8] = [0x8B, 0x54, 0x24, 0x3C, 0x8B, 0x4C, 0x24, 0x4C];
/// `CUiWindow::AddRegion` 0x5FF830 (thiscall, plain `ret`) and `CUiRegion::SetRect` 0x5FC9E0 (left, top, right,
/// bottom; `ret 0x10`).
const ADD_REGION: usize = eix_sites::C_UI_WINDOW_ADD_REGION;
const SET_RECT: usize = eix_sites::C_UI_REGION_SET_RECT;

/// The check boxes' sprites, top to bottom: the box adds them before its other sprites.
const EASY_BOX: i32 = 0;
const NORMAL_BOX: i32 = 1;
const HARD_BOX: i32 = 2;
/// The stored difficulty (0x79BEC0) the normal box stands for; the click handler's own bytes store easy's 2 and
/// hard's 0.
const NORMAL_LEVEL: u32 = 1;
/// A check box sprite's texture offset, `[sprite + 0xCC]`: the float with these bits shows it checked, 0 unchecked.
const MARK: u32 = 0xCC;
const CHECKED: u32 = 0xBDB8_0000;
/// The click handler's `cmp eax, 3; ja` and `jmp [eax*4 + 0x633EA4]`: five regions, the table 8 bytes on.
const CLICK_BOUND: Bytes = (
    eix_sites::TEXT_633DEA,
    &[0x03, 0x0F, 0x87, 0xAC],
    &[0x04, 0x0F, 0x87, 0xB7],
);
const CLICK_TABLE: Bytes = (eix_sites::TEXT_633DF4, &[0xA4], &[0xAC]);
/// The click handler from the easy box's case (ECX the sprite array) to its jump table and padding.
const CLICK_CASES: usize = eix_sites::TEXT_633E5F;
const CLICK_CASES_WAS: [u8; 97] = [
    0x8B, 0x51, 0x04, 0xC7, 0x82, 0xCC, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC7, 0x86, 0xEC,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x5E, 0xC2, 0x0C, 0x00, 0x8B, 0x46, 0x74, 0x8B, 0x48,
    0x04, 0x33, 0xC0, 0xC7, 0x81, 0xCC, 0x00, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xBD, 0x8B, 0x56, 0x74,
    0x8B, 0x0A, 0x89, 0x81, 0xCC, 0x00, 0x00, 0x00, 0x89, 0x86, 0xEC, 0x00, 0x00, 0x00, 0x5E, 0xC2,
    0x0C, 0x00, 0x8D, 0x49, 0x00, 0xF8, 0x3D, 0x63, 0x00, 0x28, 0x3E, 0x63, 0x00, 0x4D, 0x3E, 0x63,
    0x00, 0x7A, 0x3E, 0x63, 0x00, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90,
];
/// The original's rewrite of it: the easy case jumps out (to clear the hard box) and comes back to clear the normal
/// box and store 2; the hard case (0x633E7C) checks the third sprite, clears the other two and stores 0; then the
/// handler's end (0x633EA8) and the table of five cases, whose last entry (at [`NORMAL_CASE`]) is the normal box's.
const CLICK_CASES_NOW: [u8; 97] = [
    0xE9, 0x00, 0x00, 0x00, 0x00, 0x90, 0x90, 0xC7, 0x82, 0xCC, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0xC7, 0x86, 0xEC, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0xEB, 0x2C, 0x8B, 0x46, 0x74,
    0x8B, 0x48, 0x08, 0x33, 0xC0, 0xC7, 0x81, 0xCC, 0x00, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xBD, 0x8B,
    0x56, 0x74, 0x8B, 0x0A, 0x89, 0x81, 0xCC, 0x00, 0x00, 0x00, 0x8B, 0x4A, 0x04, 0x89, 0x81, 0xCC,
    0x00, 0x00, 0x00, 0x89, 0x86, 0xEC, 0x00, 0x00, 0x00, 0x5E, 0xC2, 0x0C, 0x00, 0xF8, 0x3D, 0x63,
    0x00, 0x28, 0x3E, 0x63, 0x00, 0x4D, 0x3E, 0x63, 0x00, 0x7C, 0x3E, 0x63, 0x00, 0x00, 0x00, 0x00,
    0x00,
];
/// The table entry of region 4, the normal box, in [`CLICK_CASES_NOW`].
const NORMAL_CASE: usize = 0x5D;
/// Where the easy case goes on after its jump out, and the handler's `pop esi; ret 0xC` in the rewrite.
const CLICK_EASY_GO_ON: usize = eix_sites::TEXT_633E66;
const CLICK_END: usize = eix_sites::TEXT_633EA8;
/// The click handler's ESI is the box + 0x94: its +0xEC is the selection, `[box + 0x180]`, and +0x74 the sprite
/// array, `[box + 0x108]`.
const CLICK_SELECTION: u32 = 0xEC;
const CLICK_SPRITES: u32 = 0x74;
/// `COptionsWindow::LoadSettings` from its store of the difficulty control's value, and `Apply` from its store of
/// the difficulty: both now go through `2 - x` (`neg eax; add eax, 2`), the code after moved down over the
/// padding, and Apply's branch past the stores follows its target.
const OPTIONS_LOAD: Bytes = (
    eix_sites::C_OPTIONS_WINDOW_LOAD_SETTINGS__25C,
    &[
        0x89, 0x82, 0xF8, 0x01, 0x00, 0x00, 0x8B, 0x83, 0x78, 0x03, 0x00, 0x00, 0x03, 0xC6, 0x8B,
        0x48, 0x08, 0x85, 0xC9, 0x74, 0x0B, 0x8B, 0x40, 0x04, 0x8B, 0x11, 0x50, 0xFF, 0x52, 0x64,
        0xEB, 0x10, 0x8B, 0x48, 0x0C, 0x85, 0xC9, 0x74, 0x09, 0x8B, 0x50, 0x04, 0x52, 0xE8, 0x54,
        0x9A, 0x00, 0x00, 0x83, 0xC6, 0x14, 0x81, 0xFE, 0x08, 0x02, 0x00, 0x00, 0x7C, 0xCB, 0x5F,
        0x5E, 0x5B, 0xC3, 0x90, 0x90, 0x90, 0x90, 0x90,
    ],
    &[
        0xF7, 0xD8, 0x83, 0xC0, 0x02, 0x89, 0x82, 0xF8, 0x01, 0x00, 0x00, 0x8B, 0x83, 0x78, 0x03,
        0x00, 0x00, 0x03, 0xC6, 0x8B, 0x48, 0x08, 0x85, 0xC9, 0x74, 0x0B, 0x8B, 0x40, 0x04, 0x8B,
        0x11, 0x50, 0xFF, 0x52, 0x64, 0xEB, 0x10, 0x8B, 0x48, 0x0C, 0x85, 0xC9, 0x74, 0x09, 0x8B,
        0x50, 0x04, 0x52, 0xE8, 0x4F, 0x9A, 0x00, 0x00, 0x83, 0xC6, 0x14, 0x81, 0xFE, 0x08, 0x02,
        0x00, 0x00, 0x7C, 0xCB, 0x5F, 0x5E, 0x5B, 0xC3,
    ],
);
const OPTIONS_APPLY_SKIP: Bytes = (eix_sites::C_OPTIONS_WINDOW_APPLY__253, &[0x16], &[0x23]);
const OPTIONS_APPLY: Bytes = (
    eix_sites::C_OPTIONS_WINDOW_APPLY__260,
    &[
        0xA3, 0xC4, 0xBE, 0x79, 0x00, 0xA3, 0xC0, 0xBE, 0x79, 0x00, 0x8A, 0x83, 0x10, 0x04, 0x00,
        0x00, 0x5F, 0x5E, 0x5B, 0x84, 0xC0, 0x74, 0x0A, 0xB9, 0xB8, 0xF5, 0x7A, 0x00, 0xE8, 0x7F,
        0xB0, 0xF1, 0xFF, 0x59, 0xC3, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
        0x90, 0x90, 0x90,
    ],
    &[
        0xF7, 0xD8, 0x83, 0xC0, 0x02, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0xA3, 0xC4,
        0xBE, 0x79, 0x00, 0xA3, 0xC0, 0xBE, 0x79, 0x00, 0x8A, 0x83, 0x10, 0x04, 0x00, 0x00, 0x5F,
        0x5E, 0x5B, 0x84, 0xC0, 0x74, 0x0A, 0xB9, 0xB8, 0xF5, 0x7A, 0x00, 0xE8, 0x72, 0xB0, 0xF1,
        0xFF, 0x59, 0xC3,
    ],
);
/// The low byte of the difficulty option's kind (its record at 0x787688: id 25, `difficulty`, kind 1, maximum 2):
/// kind 0 makes the options window build a slider instead of the two-text spinner.
const DIFFICULTY_KIND: Bytes = (eix_sites::DATA_787690, &[0x01], &[0x00]);

/// The vertical step between the box's rows of labels and check boxes.
const ROW_STEP: i32 = 24;

/// Check box `index` (0 at the top), in layout units: 21 by 21 at x `400 - half`, `ROW_STEP` per row below `row`.
fn check_box(half: i32, row: i32, index: i32) -> Rect {
    let top = row.wrapping_add(ROW_STEP * index);
    Rect {
        left: 400i32.wrapping_sub(half),
        top,
        right: 421i32.wrapping_sub(half),
        bottom: top.wrapping_add(21),
    }
}

/// Label `index`'s rectangle on the box's surface, from the labels' left edge `left`, 100 above its check box.
fn label(left: i32, row: i32, index: i32) -> Rect {
    Rect {
        left,
        top: row.wrapping_sub(100).wrapping_add(ROW_STEP * index),
        right: 385,
        bottom: 450,
    }
}

/// The three check boxes' texture offsets, top to bottom, with only box `checked` checked.
fn marks(checked: i32) -> [u32; 3] {
    [EASY_BOX, NORMAL_BOX, HARD_BOX].map(|index| if index == checked { CHECKED } else { 0 })
}

/// The hard label's text: a `CString` of ours, read where the build reads its labels and destroyed once drawn (the
/// original keeps it in a slot of the build's frame). The game runs the build on one thread.
static HARD_TEXT: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());

fn hard_text() -> *mut GameString {
    // A `GameString` is one pointer, as is the atomic.
    HARD_TEXT.as_ptr().cast()
}

/// At [`LABELS_READ`], after the replaced instruction: reads "difficulty_hard" from the texts.
extern "C" fn read_hard_label(_: usize, _: &mut Context) {
    type GetString = unsafe extern "thiscall" fn(texts: u32, key: *const u8, out: *mut GameString);
    let text = hard_text();
    // SAFETY: the slot holds no string: the draw after this destroys the one read here.
    unsafe { mfc::construct_empty(text) };
    // SAFETY: the game's text manager pointer, which the build reads at this site.
    let texts = unsafe { read::<u32>(TEXTS) };
    // SAFETY: `CTextManager::GetString`, see [`GET_STRING`].
    let get = unsafe { hook::as_fn::<GetString>(GET_STRING) };
    // SAFETY: the text manager, a key of game.exe's `.data` and our constructed string, on the game's thread.
    unsafe {
        get(
            texts,
            core::ptr::with_exposed_provenance(eix_sites::here(HARD_KEY as usize)),
            text,
        )
    };
}

/// At [`LABELS_DRAW`], after the replaced instructions: draws the hard label as the third, then frees its text.
extern "C" fn draw_hard_label(_: usize, ctx: &mut Context) {
    let text = hard_text();
    // The replaced `push 1` moved the build's surface local one word up.
    if let Some(surface) = ctx.stack_word(5) {
        let mut rect = label(ctx.eax as i32, ctx.ebx as i32, HARD_BOX);
        // SAFETY: the game's `UiSurface::DrawTextIn`.
        let draw = unsafe { hook::as_fn::<DrawTextIn>(ui::DRAW_TEXT_IN) };
        // SAFETY: the surface the build draws its labels on, inside its device context; the string read at
        // [`LABELS_READ`].
        unsafe {
            draw(
                core::ptr::with_exposed_provenance_mut::<Surface>(surface as usize),
                // The string's characters pointer, its only word.
                HARD_TEXT.load(Ordering::Relaxed),
                &raw mut rect,
                LABEL_FONT,
                LABEL_COLOUR,
                0,
                1,
            )
        };
    }
    // SAFETY: the string constructed at [`LABELS_READ`], not used again.
    unsafe { mfc::destroy(text) };
}

/// Shapes `sprite` as the box `rect`, as the build shapes its own check boxes.
///
/// # Safety
/// `sprite` must be a live sprite of the box, on the game's thread.
unsafe fn shape_box(sprite: *mut UiMesh, rect: Rect) {
    // Layout coordinates are small integers, exact in an `f32`, as the build's own `fild`s load them.
    let [left, top, right, bottom] =
        [rect.left, rect.top, rect.right, rect.bottom].map(|v| v as f32);
    // SAFETY: the caller guarantees the sprite.
    unsafe {
        UiMesh::set_rectangle(
            sprite, BOX_DEPTH, BOX_SCALE, left, top, right, bottom, BOX_TEXELS,
        )
    };
}

/// At [`BOX_HEIGHT`], after the replaced instructions: the box is a row taller.
extern "C" fn grow_box(_: usize, ctx: &mut Context) {
    ctx.ecx = ctx.ecx.wrapping_add(ROW_STEP as u32);
}

/// The first check box's row, kept from [`BOXES`] for [`REGIONS`], where the build no longer holds it. The game runs
/// the build on one thread.
static BOX_ROW: AtomicI32 = AtomicI32::new(0);

/// At [`BOXES`], after the replaced instructions: the build's second check box becomes the normal one, checked, and a
/// third sprite takes its place for the build to shape as the hard box.
extern "C" fn add_hard_box(_: usize, ctx: &mut Context) {
    let (half, row) = (ctx.ebp as i32, ctx.ebx as i32);
    BOX_ROW.store(row, Ordering::Relaxed);
    // SAFETY: EAX is the sprite the build just added to the box.
    unsafe {
        shape_box(
            core::ptr::with_exposed_provenance_mut(ctx.eax as usize),
            check_box(half, row, NORMAL_BOX),
        )
    };
    // SAFETY: that sprite's texture offset.
    unsafe { common::write(ctx.eax + MARK, CHECKED) };
    // SAFETY: ESI is the box being built, a window; a blank sprite as the build adds.
    let hard = unsafe {
        UiWindow::add_sprite(
            core::ptr::with_exposed_provenance_mut(ctx.esi as usize),
            core::ptr::null(),
        )
    };
    ctx.eax = hard.expose_provenance() as u32;
    ctx.edx = check_box(half, row, HARD_BOX).bottom as u32;
}

/// The click region of check box `index`: the build's left and right (`second`), the box's own rows.
fn region(second: Rect, row: i32, index: i32) -> Rect {
    let rows = check_box(0, row, index);
    Rect {
        top: rows.top,
        bottom: rows.bottom,
        ..second
    }
}

/// At [`REGIONS`], after the replaced instructions: the build's second check box region becomes the hard box's, and
/// a fifth region takes its place for the build to set as the normal box's, each over its own box's rows. The
/// original gives the normal region the build's second rectangle and the hard region that rectangle a row lower; a
/// click on its hard box then does not select hard.
extern "C" fn add_normal_region(_: usize, ctx: &mut Context) {
    type SetRect =
        unsafe extern "thiscall" fn(region: u32, left: i32, top: i32, right: i32, bottom: i32);
    type AddRegion = unsafe extern "thiscall" fn(window: u32) -> u32;
    let second = Rect {
        left: ctx.ebx as i32,
        top: ctx.ecx as i32,
        right: ctx.edi as i32,
        bottom: ctx.edx as i32,
    };
    let row = BOX_ROW.load(Ordering::Relaxed);
    let hard = region(second, row, HARD_BOX);
    let normal = region(second, row, NORMAL_BOX);
    // The build sets the region it holds in EAX from these two, its left and right in EBX and EDI.
    ctx.ecx = normal.top as u32;
    ctx.edx = normal.bottom as u32;
    // SAFETY: see [`SET_RECT`] and [`ADD_REGION`].
    let set_rect = unsafe { hook::as_fn::<SetRect>(SET_RECT) };
    // SAFETY: as above.
    let add_region = unsafe { hook::as_fn::<AddRegion>(ADD_REGION) };
    // SAFETY: EAX is the region the build just added, ESI the box, on the game's thread.
    unsafe { set_rect(ctx.eax, hard.left, hard.top, hard.right, hard.bottom) };
    // SAFETY: as above.
    ctx.eax = unsafe { add_region(ctx.esi) };
}

/// The click handler's easy case at [`CLICK_CASES`], after the replaced `mov edx, [ecx + 4]` (ECX the sprite array):
/// the hard box is cleared too.
extern "C" fn easy_clicked(_: usize, ctx: &mut Context) {
    // SAFETY: the box's sprite array, which holds the three check boxes first.
    let hard = unsafe { read::<u32>(ctx.ecx + 4 * HARD_BOX as u32) };
    // SAFETY: that sprite's texture offset.
    unsafe { common::write(hard + MARK, 0u32) };
}

/// The click handler's case for region 4, the normal box (ESI the box + 0x94): it is checked, the other two cleared,
/// and the selection becomes 1, which OK and Enter then store as the difficulty.
extern "C" fn normal_clicked(_: usize, ctx: &mut Context) {
    // SAFETY: the box's sprite array, see [`CLICK_SPRITES`].
    let sprites = unsafe { read::<u32>(ctx.esi + CLICK_SPRITES) };
    for (index, mark) in (0u32..).zip(marks(NORMAL_BOX)) {
        // SAFETY: the array's first three sprites, the check boxes.
        let sprite = unsafe { read::<u32>(sprites + 4 * index) };
        // SAFETY: that sprite's texture offset.
        unsafe { common::write(sprite + MARK, mark) };
    }
    // SAFETY: the box's selection, see [`CLICK_SELECTION`].
    unsafe { common::write(ctx.esi + CLICK_SELECTION, NORMAL_LEVEL) };
}

/// `newdifficultylevels` = 1: the new game's difficulty box offers easy (stored 2), normal (1, checked at the start)
/// and hard (0), and the options' difficulty becomes a slider of `2 - x`.
fn new_difficulty_levels(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let normal_key = (NORMAL_KEY.as_ptr().expose_provenance() as u32).to_le_bytes();
    let easy = common::jump_stub(
        CLICK_CASES,
        &CLICK_CASES_WAS[..3],
        easy_clicked,
        0,
        CLICK_EASY_GO_ON,
    )?;
    let normal = common::jump_stub(CLICK_END, &[], normal_clicked, 0, CLICK_END)?;
    let mut click = CLICK_CASES_NOW;
    click[NORMAL_CASE..NORMAL_CASE + 4].copy_from_slice(&(normal as u32).to_le_bytes());
    let mut writes = common::nops(&[EXPLANATION_TRIM]);
    writes.push(common::branch_before(
        BOX_HEIGHT,
        &BOX_HEIGHT_WAS,
        &[0xE9, 0, 0, 0, 0, 0x90, 0x90, 0x90, 0x90],
        0,
        grow_box,
        0,
    )?);
    writes.extend(common::fixed(&[EXPLANATION_TOP])?);
    writes.push(common::branch_before(
        LABELS_READ,
        &LABELS_READ_WAS,
        &[0x90, 0xE9, 0, 0, 0, 0],
        1,
        read_hard_label,
        0,
    )?);
    writes.push(SiteWrite::bytes(
        SECOND_KEY,
        &HARD_KEY.to_le_bytes(),
        &normal_key,
    )?);
    writes.push(common::branch_before(
        LABELS_DRAW,
        &LABELS_DRAW_WAS,
        &[0x90, 0xE9, 0, 0, 0, 0],
        1,
        draw_hard_label,
        0,
    )?);
    writes.extend(common::fixed(&[START_SELECTION])?);
    writes.push(common::branch_before(
        BOXES,
        &BOXES_WAS,
        &[0xE9, 0, 0, 0, 0],
        0,
        add_hard_box,
        0,
    )?);
    writes.extend(common::fixed(&[HARD_BOX_TOP])?);
    writes.extend(common::nops(&[HARD_BOX_MARK]));
    writes.push(common::branch_before(
        REGIONS,
        &REGIONS_WAS,
        &[0x90, 0x90, 0x90, 0xE9, 0, 0, 0, 0],
        3,
        add_normal_region,
        0,
    )?);
    writes.extend(common::fixed(&[CLICK_BOUND, CLICK_TABLE])?);
    writes.push(common::branch(
        CLICK_CASES,
        &CLICK_CASES_WAS,
        &click,
        0,
        easy,
    )?);
    writes.extend(common::fixed(&[
        OPTIONS_LOAD,
        OPTIONS_APPLY_SKIP,
        OPTIONS_APPLY,
        DIFFICULTY_KIND,
    ])?);
    Ok(writes)
}

#[cfg(test)]
mod tests {
    use super::super::testing::{Original, assert_original, no_log, plain, values};
    use super::*;

    fn sites(writes: &[SiteWrite]) -> Vec<(usize, usize)> {
        writes.iter().map(|w| (w.site(), w.size())).collect()
    }

    #[test]
    fn switches_write_only_at_their_values() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        for (_, key, build) in BUILDS {
            for value in [0, 3, -1] {
                // The blocks here that act on any value from 1 up.
                if matches!(*key, "actionsdexteritymovespeed" | "newperks") && value > 0 {
                    continue;
                }
                assert!(build(&ctx, value).unwrap().is_empty(), "{key} = {value}");
            }
        }
    }

    #[test]
    fn the_skill_price_keeps_its_parameters_in_its_own_bytes() {
        let v = values(&[
            ("rebalanceengine", "pow", "1.1"),
            ("rebalanceengine", "mul", "4.95321"),
            ("rebalanceengine", "add", "0.0"),
        ]);
        let ctx = Ctx::new(&v, no_log);
        let w = plain(&skills_counting(&ctx, 1).unwrap());
        assert_eq!(w.len(), 1);
        let (site, was, now) = &w[0];
        assert_eq!(
            (*site, was.as_slice()),
            (eix_sites::SCHOOL_PRICE, &SCHOOL_PRICE_WAS[..])
        );
        assert_eq!(now[0x10..0x18], 1.1f64.to_le_bytes());
        assert_eq!(now[0x18..0x1C], 4.95321f32.to_le_bytes());
        assert_eq!(now[0x1C..0x20], [0; 4]);
        // The call at 0x52801A reaches _CIpow 0x6E81A0; the tail from the original's `fst` on is kept.
        assert_eq!(now[0x2A..0x2F], [0xE8, 0x81, 0x01, 0x1C, 0x00]);
        assert_eq!(now[0x45..0x4B], [0xD9, 0x54, 0x24, 0x04, 0x90, 0x90]);
        assert_eq!(now[0x4B..], SCHOOL_PRICE_WAS[0x4B..]);
    }

    #[test]
    fn run_constants_are_digit_parsed_doubles() {
        let v = values(&[("rebalanceengine", "runconst", "0.00666666")]);
        let ctx = Ctx::new(&v, no_log);
        let w = plain(&run_ability_time(&ctx, 1).unwrap());
        assert_eq!(w[0].0, eix_sites::RDATA_73F088);
        assert_eq!(w[0].1, RUN_CONSTANT_WAS.to_le_bytes());
        assert_eq!(w[0].2, 0.00666666f64.to_le_bytes());
        // The built-in "0,5" counts its comma as a point.
        assert_eq!(w[1].2, 0.5f64.to_le_bytes());
        assert_eq!(
            sites(&new_run_system(&ctx, 2).unwrap()),
            [
                (eix_sites::UPDATE_MOTION__B22, 14),
                (eix_sites::UPDATE_MOTION__A8B, 6),
                (eix_sites::TEXT_548518, 8)
            ]
        );
        assert_eq!(
            sites(&new_run_system(&ctx, 1).unwrap()),
            [(eix_sites::UPDATE_MOTION__A85, 14)]
        );
    }

    #[test]
    fn the_level_formula_reads_its_four_words() {
        let v = values(&[("rebalanceengine", "divlvla", "295.0")]);
        let ctx = Ctx::new(&v, no_log);
        let w = plain(&level_counting(&ctx, 1).unwrap());
        assert_eq!(
            w.iter().map(|w| (w.0, w.1.len())).collect::<Vec<_>>(),
            [
                (eix_sites::C_UNIT_CLIENT_SPECIFIC_LEVEL__10, 48),
                (eix_sites::C_UNIT_SERVER_LEVEL, 64),
                (eix_sites::DATA_79A910, 4),
                (eix_sites::DATA_79A914, 4),
                (eix_sites::DATA_79A918, 4),
                (eix_sites::DATA_79A91C, 4)
            ]
        );
        assert_eq!(w[2].2, 295.0f32.to_le_bytes());
        assert_eq!(w[0].2[..3], [0xD9, 0xE8, 0xDB]);
        assert_eq!(
            w[1].2[46..54],
            [0x8B, 0x45, 0xF8, 0x8B, 0xE5, 0x5D, 0xC3, 0x90]
        );
        assert!(level_counting(&ctx, 2).unwrap().is_empty());
    }

    #[test]
    fn plain_features_write_the_originals_sites() {
        let v = values(&[("gameplay", "actionsmul", "0.2")]);
        let ctx = Ctx::new(&v, no_log);
        assert_eq!(sites(&actions_speed(&ctx, 1).unwrap()).len(), 1 + 6 + 6);
        assert_eq!(
            plain(&actions_speed(&ctx, 1).unwrap())[6].2,
            [0xD9, 0x05, 0x64, 0x9C, 0x79, 0x00]
        );
        assert_eq!(
            sites(&difficulty_reset(&ctx, 2).unwrap()),
            [
                (eix_sites::C_UNIT_SERVER_GET_ATTACK_PARAMS__2D, 6),
                (eix_sites::C_UNIT_CLIENT_SPECIFIC_GET_ATTACK_PARAMS__17, 6),
                (eix_sites::ATTACK_PARAMS_SCALE_FOR_DIFFICULTY__14, 1),
                (eix_sites::ATTACK_PARAMS_SCALE_FOR_DIFFICULTY__2C, 1)
            ]
        );
        assert_eq!(
            sites(&n_hits(&ctx, 1).unwrap()),
            [
                (eix_sites::C_UNIT_SERVER_PLAN_ACTION__308, 22),
                (eix_sites::C_UNIT_SERVER_PLAN_ACTION__441, 7)
            ]
        );
        assert_eq!(
            plain(&actions_patch(&ctx, 1).unwrap())[0],
            (0x40_02D0, vec![0; 4], vec![0xCD, 0xCC, 0x4C, 0x3E])
        );
        let w = plain(&backstab_visual_base(&ctx, 1).unwrap());
        assert_eq!(w[0].1, w[0].2);
        let w = plain(&experience_for_intelligence(&ctx, 1).unwrap());
        assert_eq!(w[0].2, 1.0245f64.to_le_bytes());
    }

    /// The original's changed bytes, `??` for a call displacement into its arena.
    #[rustfmt::skip]
    const ORIGINAL: &[Original] = &[
        ("gameplay", "newperks", 1, &[], &[(eix_sites::UNIT_STATS_RECOMPUTE__8F, "19"), (eix_sites::UNIT_STATS_RECOMPUTE__B9, "1A"), (eix_sites::UNIT_STATS_RECOMPUTE__E0, "1B"), (eix_sites::UNIT_STATS_RECOMPUTE__2DF, "90 E8 ?? ?? ?? ??"), (eix_sites::UNIT_STATS_RECOMPUTE__2EE, "90 90 E8 ?? ?? ?? ??"), (eix_sites::UNIT_STATS_RESET_TO_BASE__84, "EC"), (eix_sites::UNIT_STATS_RESET_TO_BASE__98, "4A"), (eix_sites::UNIT_STATS_RESET_TO_BASE__BF, "ED"), (eix_sites::UNIT_STATS_RESET_TO_BASE__D3, "4D"), (eix_sites::UNIT_STATS_RESET_TO_BASE__FA, "EE"), (eix_sites::UNIT_STATS_RESET_TO_BASE__10E, "50"), (eix_sites::UNIT_STATS_CAN_LEARN_PERK__1A2, "1C"), (eix_sites::UNIT_STATS_CAN_LEARN_PERK__34A, "1C"), (eix_sites::UNIT_STATS_SPELL_SCHOOL_SKILL__7, "90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90"), (eix_sites::C_ITEM_INFO_PANEL_BUILD__EB, "33"), (eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_STATS__7EB, "1B"), (eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_STATS__116A, "90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90"), (eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_STATS__137E, "0A 0A")]),
        ("gameplay", "newperks", 2, &[], &[(eix_sites::UNIT_STATS_RECOMPUTE__8F, "19"), (eix_sites::UNIT_STATS_RECOMPUTE__B9, "1A"), (eix_sites::UNIT_STATS_RECOMPUTE__E0, "1B"), (eix_sites::UNIT_STATS_RECOMPUTE__2DF, "90 E8 ?? ?? ?? ??"), (eix_sites::UNIT_STATS_RECOMPUTE__2EE, "90 90 E8 ?? ?? ?? ??"), (eix_sites::UNIT_STATS_RESET_TO_BASE__84, "EC"), (eix_sites::UNIT_STATS_RESET_TO_BASE__98, "4A"), (eix_sites::UNIT_STATS_RESET_TO_BASE__BF, "ED"), (eix_sites::UNIT_STATS_RESET_TO_BASE__D3, "4D"), (eix_sites::UNIT_STATS_RESET_TO_BASE__FA, "EE"), (eix_sites::UNIT_STATS_RESET_TO_BASE__10E, "50"), (eix_sites::UNIT_STATS_CAN_LEARN_PERK__1A2, "1C"), (eix_sites::UNIT_STATS_CAN_LEARN_PERK__34A, "1C"), (eix_sites::UNIT_STATS_SPELL_SCHOOL_SKILL__7, "90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90"), (eix_sites::C_ITEM_INFO_PANEL_BUILD__EB, "33"), (eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_CARRIED_SPELL__25F, "14"), (eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_SPELL_RECORD__1E5, "14"), (eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_STATS__7EB, "1B"), (eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_STATS__116A, "90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90"), (eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_STATS__137E, "0A 0A"), (eix_sites::C_HUD_SPELL_COLUMN_SET_UNIT__15C, "14")]),
        // `??` also for the pushed key's address and the click table's last entry, both in the original's arena. The
        // box's height (0x6338F9) and the explanation's top (0x633967) are ours alone, a row lower; the test
        // `the_difficulty_box_makes_room_for_its_third_row` pins them.
        ("gameplay", "newdifficultylevels", 1, &[], &[(eix_sites::C_DIFFICULTY_BOX_BUILD__FD, "90 90 90 90 90 90 90 90 90 90 90 90 90 90"), (eix_sites::C_DIFFICULTY_BOX_BUILD__149, "?? ?? ?? ?? ?? ?? ?? ?? ??"), (eix_sites::C_DIFFICULTY_BOX_BUILD__1B7, "??"), (eix_sites::C_DIFFICULTY_BOX_BUILD__1E9, "90 E9 ?? ?? ?? ??"), (eix_sites::C_DIFFICULTY_BOX_BUILD__20F, "?? ?? ?? ??"), (eix_sites::C_DIFFICULTY_BOX_BUILD__2C4, "90 E9 ?? ?? ?? ??"), (eix_sites::C_DIFFICULTY_BOX_BUILD__314, "01"), (eix_sites::C_DIFFICULTY_BOX_BUILD__38C, "E9 ?? ?? ?? ??"), (eix_sites::C_DIFFICULTY_BOX_BUILD__3B5, "30"), (eix_sites::C_DIFFICULTY_BOX_BUILD__3D9, "90 90 90 90 90 90 90 90 90 90"), (eix_sites::C_DIFFICULTY_BOX_BUILD__5BF, "90 90 90 E9 ?? ?? ?? ??"), (eix_sites::TEXT_633DEA, "04"), (eix_sites::TEXT_633DED, "B7"), (eix_sites::TEXT_633DF4, "AC"), (eix_sites::TEXT_633E5F, "E9 ?? ?? ?? ?? 90 90 C7 82 CC"), (eix_sites::TEXT_633E6C, "00 00 00"), (eix_sites::TEXT_633E70, "C7 86 EC"), (eix_sites::TEXT_633E76, "02 00 00"), (eix_sites::TEXT_633E7A, "EB 2C 8B 46 74 8B 48 08 33 C0 C7 81 CC"), (eix_sites::TEXT_633E8A, "00 00 B8 BD 8B 56 74 8B 0A 89 81 CC"), (eix_sites::TEXT_633E97, "00 00 8B 4A 04 89 81 CC 00"), (eix_sites::TEXT_633EA1, "00 89 86 EC 00 00"), (eix_sites::TEXT_633EA8, "5E C2 0C"), (eix_sites::TEXT_633EAC, "F8 3D"), (eix_sites::TEXT_633EB0, "28"), (eix_sites::TEXT_633EB4, "4D 3E 63 00 7C 3E 63 00 ?? ?? ?? ??"), (eix_sites::C_OPTIONS_WINDOW_LOAD_SETTINGS__25C, "F7 D8 83 C0 02 89 82 F8 01 00"), (eix_sites::C_OPTIONS_WINDOW_LOAD_SETTINGS__267, "8B 83 78 03 00 00 03 C6 8B 48 08 85 C9 74 0B 8B 40 04 8B 11 50 FF 52 64 EB 10 8B 48 0C 85 C9 74 09 8B 50 04 52 E8 4F 9A 00 00 83 C6 14 81 FE 08 02 00 00 7C CB 5F 5E 5B C3"), (eix_sites::C_OPTIONS_WINDOW_APPLY__253, "23"), (eix_sites::C_OPTIONS_WINDOW_APPLY__260, "F7 D8 83 C0 02 90 90 90 90 90 90 90 90 A3 C4 BE 79 00 A3 C0 BE 79 00 8A 83 10 04"), (eix_sites::C_OPTIONS_WINDOW_APPLY__27C, "00 5F 5E 5B 84 C0 74 0A B9 B8 F5 7A 00 E8 72 B0 F1 FF 59 C3")]),
    ];

    #[test]
    fn new_features_write_the_originals_bytes() {
        assert_original(ORIGINAL);
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        assert_eq!(
            sites(&new_perks(&ctx, 3).unwrap()),
            sites(&new_perks(&ctx, 1).unwrap())
        );
        assert!(new_perks(&ctx, -1).unwrap().is_empty());
    }

    #[test]
    fn the_difficulty_box_adds_a_row_below_the_builds_two() {
        let (half, row, left) = (57, 205, 173);
        // The build's own boxes and labels, which ours continue a row apart.
        assert_eq!(
            check_box(half, row, EASY_BOX),
            Rect {
                left: 343,
                top: 205,
                right: 364,
                bottom: 226
            }
        );
        assert_eq!(label(left, row, EASY_BOX).top, 105);
        assert_eq!(label(left, row, NORMAL_BOX).top, 129);
        assert_eq!(
            check_box(half, row, NORMAL_BOX),
            Rect {
                left: 343,
                top: 229,
                right: 364,
                bottom: 250
            }
        );
        assert_eq!(
            check_box(half, row, HARD_BOX),
            Rect {
                left: 343,
                top: 253,
                right: 364,
                bottom: 274
            }
        );
        assert_eq!(
            label(left, row, HARD_BOX),
            Rect {
                left: 173,
                top: 153,
                right: 385,
                bottom: 450
            }
        );
        // Each click region keeps the build's left and right and takes its own box's rows.
        let second = Rect {
            left: 340,
            top: 1,
            right: 370,
            bottom: 2,
        };
        assert_eq!(
            region(second, row, NORMAL_BOX),
            Rect {
                left: 340,
                top: 229,
                right: 370,
                bottom: 250
            }
        );
        assert_eq!(
            region(second, row, HARD_BOX),
            Rect {
                left: 340,
                top: 253,
                right: 370,
                bottom: 274
            }
        );
    }

    #[test]
    fn the_difficulty_box_makes_room_for_its_third_row() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        let w = plain(&new_difficulty_levels(&ctx, 1).unwrap());
        // The explanation's top `lea edx, [ebx + edi - 7]` becomes `+ 17`, one ROW_STEP lower.
        assert!(w.contains(&(
            eix_sites::C_DIFFICULTY_BOX_BUILD__1B7,
            vec![0xF9],
            vec![0x11]
        )));
        assert_eq!(0x11 - ROW_STEP, -7);
        let (_, was, now) = w.iter().find(|w| w.0 == BOX_HEIGHT).unwrap();
        assert_eq!(was, &BOX_HEIGHT_WAS);
        assert_eq!(now[0], 0xE9);
        assert_eq!(now[5..], [0x90; 4]);
    }

    #[test]
    fn the_difficulty_choices_check_one_box_and_store_its_level() {
        assert_eq!(marks(NORMAL_BOX), [0, CHECKED, 0]);
        assert_eq!(marks(EASY_BOX), [CHECKED, 0, 0]);
        assert_eq!(f32::from_bits(CHECKED), -0.089_843_75);
        // The rewritten click cases: easy stores 2 (`mov dword [esi + 0xEC], 2`), hard checks the third sprite
        // (`mov ecx, [eax + 8]`) and stores EAX after `xor eax, eax`; normal stores NORMAL_LEVEL.
        assert_eq!(
            CLICK_CASES_NOW[0x11..0x1B],
            [0xC7, 0x86, 0xEC, 0, 0, 0, 2, 0, 0, 0]
        );
        assert_eq!(CLICK_CASES_NOW[0x21..0x25], [0x48, 0x08, 0x33, 0xC0]);
        assert_eq!(NORMAL_LEVEL, 1);
        // The options' slider shows `2 - stored`: easy 0, normal 1, hard 2.
        assert_eq!(OPTIONS_LOAD.2[..5], [0xF7, 0xD8, 0x83, 0xC0, 0x02]);
        assert_eq!(OPTIONS_APPLY.2[..5], [0xF7, 0xD8, 0x83, 0xC0, 0x02]);
    }

    #[test]
    fn new_difficulty_levels_points_its_table_at_its_own_case() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        let w = plain(&new_difficulty_levels(&ctx, 1).unwrap());
        // The option record's kind, outside the code the original's run is compared on.
        assert!(w.contains(&(eix_sites::DATA_787690, vec![0x01], vec![0x00])));
        let (_, _, click) = w.iter().find(|w| w.0 == CLICK_CASES).unwrap();
        let entry = u32::from_le_bytes(click[NORMAL_CASE..NORMAL_CASE + 4].try_into().unwrap());
        assert!(entry > 0x10000 && entry as usize != CLICK_END, "{entry:#x}");
        let (_, was, key) = w.iter().find(|w| w.0 == SECOND_KEY).unwrap();
        assert_eq!(was, &HARD_KEY.to_le_bytes());
        let key = u32::from_le_bytes(key[..].try_into().unwrap());
        // SAFETY: the address the build pushes, our NUL-terminated key.
        let key = unsafe { CStr::from_ptr(key as usize as *const core::ffi::c_char) };
        assert_eq!(key, c"difficulty_norm");
    }
}
