//! Combat features, and `enablekillsinmulti`, which the registry files under combat.
#![expect(
    unsafe_code,
    reason = "the hook handlers read and write the game objects of the hooked functions"
)]

use core::mem::offset_of;
use std::sync::atomic::{AtomicU32, Ordering};

use eix_engine::game::combat::AttackParams;
use eix_engine::game::player::NamedFloatTable;
use eix_engine::game::unit::{MapObject, Unit, UnitServer, UnitStatLayer, UnitStatSlot, UnitStats};
use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, FpuContext, Mode};
use eix_engine::x87::Ext;
use eix_sites::Site;

use super::common::{self, Bytes, NOWHERE, Stale, read, write};
use super::{BuildError, Ctx};

/// `CUnitServer::OnDamaged` 0x553B07: the virtual call that reports the damage, and where the original's added code
/// goes on with it (0x553B0C) or past the diplomacy reaction it would raise (0x553D62).
const ON_DAMAGED: Site = eix_sites::C_UNIT_SERVER_ON_DAMAGED__47;
/// `call [edx + 0x44]; test eax, eax`.
const ON_DAMAGED_WAS: [u8; 5] = [0xFF, 0x52, 0x44, 0x85, 0xC0];
const ON_DAMAGED_GO_ON: Site = eix_sites::C_UNIT_SERVER_ON_DAMAGED__4C;
const ON_DAMAGED_NO_REACTION: Site = eix_sites::C_UNIT_SERVER_ON_DAMAGED__2A2;
/// The variable the feature reads, with the damaged unit's id after it: player 0's `und<id>`.
const DUEL_VARIABLE: &[u8] = b"und";

/// `OnDamaged`'s virtual call, with the diplomacy reaction skipped when player 0 has `und<id>` set for the damaged
/// unit (`[esi + 0x1C]`): a duel, which the players' sides do not react to.
extern "C" fn duel_check(_: usize, ctx: &mut Context) {
    ctx.resume = ON_DAMAGED_GO_ON;
    let Some(player) = common::player_zero() else {
        return;
    };
    // SAFETY: ESI is the damaged unit's server controller; +0x1C its unit, whose +0xC is its id.
    let unit = unsafe { read::<u32>(ctx.esi + 0x1C) };
    // SAFETY: a live unit's engine-object id.
    let id = unsafe {
        read::<i32>(unit + offset_of!(eix_engine::game::object::EngineObject, id) as u32)
    };
    let mut name = [0u8; common::ID_NAME_LEN];
    let name = common::id_variable(DUEL_VARIABLE, id, &mut name);
    // SAFETY: `player` is a live `CPlayer`, whose +0xC4 is its variable table.
    let variables = unsafe { &raw const (*player).variables };
    // SAFETY: that table, with a NUL-terminated name.
    let value = unsafe { NamedFloatTable::get(variables, name) };
    if Ext::new(value.unwrap_or(0.0).into()).fistp() != 0 {
        ctx.resume = ON_DAMAGED_NO_REACTION;
    }
}

/// `duels` = 1: damage between players raises no diplomacy reaction while their duel variable is set.
pub(crate) fn duels(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let stub = common::tail_stub(ON_DAMAGED, duel_check, 0, &ON_DAMAGED_WAS)?;
    Ok(vec![SiteWrite::jmp(ON_DAMAGED, &ON_DAMAGED_WAS, stub)?])
}

/// `increasinghitchance`'s parameters, in the padding of two unrelated functions: `tohitmul`, `tohitsub`.
const TO_HIT_MUL: Site = eix_sites::TEXT_4500EC;
const TO_HIT_SUB: Site = eix_sites::OWNED_BONE_LINK_ARRAYS_SET_SIZE__184;
/// The miss counter the feature keeps per unit: the companion's byte +0x25.
const MISS_COUNTER: u32 = 0x25;

/// The miss counter's address for the defender `ResolveHit` has at `[ebp + 0xC]`, when it has a companion.
fn miss_counter(ctx: &Context) -> Option<u32> {
    // SAFETY: `ResolveHit`'s argument at EBP + 0xC is the defending unit, which the original reads the same way.
    let unit = unsafe { read::<u32>(ctx.ebp.wrapping_add(0xC)) };
    if unit == 0 {
        return None;
    }
    // SAFETY: a live unit's companion.
    let companion = unsafe { read::<u32>(unit + COMPANION) };
    (companion != 0).then(|| companion + MISS_COUNTER)
}

/// A map object's companion, where the counter lives.
const COMPANION: u32 = offset_of!(eix_engine::game::unit::MapObject, companion) as u32;

/// `ResolveHit` 0x553EF8, in place of `fild [ebp - 0xC]; fcomp [ebp - 4]`: past `tohitcheck` misses in a row the
/// roll is compared with `counter * tohitmul - tohitsub + [ebp - 4]` instead of `[ebp - 4]`, so a further miss
/// gets likelier to hit. The comparison's x87 status is what the following `fnstsw` reads.
extern "C" fn increased_hit_chance(check: usize, ctx: &mut Context) {
    // SAFETY: `ResolveHit`'s locals, as the original reads them.
    let roll = unsafe { read::<i32>(ctx.ebp.wrapping_sub(0xC)) };
    // SAFETY: as above.
    let against = unsafe { read::<f32>(ctx.ebp.wrapping_sub(4)) };
    let mut threshold = f64::from(against);
    if let Some(at) = miss_counter(ctx) {
        // SAFETY: the counter byte of a live companion.
        let counter = unsafe { read::<u8>(at) };
        if u32::from(counter) > check as u32 {
            threshold = Ext::new(counter.into())
                .fmul(game_float(TO_HIT_MUL).into())
                .fsub(game_float(TO_HIT_SUB).into())
                .fadd(against.into())
                .to_f64();
        }
    }
    let roll = f64::from(roll);
    // SAFETY: the compare the replaced instructions made, on two locals of ours; it uses and pops one x87
    // register, leaving the game's stack as it was and the status word the next `fnstsw` reads.
    unsafe {
        core::arch::asm!(
            "fld qword ptr [{a}]",
            "fcomp qword ptr [{b}]",
            a = in(reg) &roll,
            b = in(reg) &threshold,
            options(nostack),
        );
    }
}

/// `ResolveHit` 0x553F05 (the hit branch): the counter is cleared.
extern "C" fn clear_miss_counter(_: usize, ctx: &mut Context) {
    if let Some(at) = miss_counter(ctx) {
        // SAFETY: as in `increased_hit_chance`.
        unsafe { write::<u8>(at, 0) };
    }
}

/// `ResolveHit` 0x553F0D (the miss branch): the counter goes up, wrapping as the original's byte store does.
extern "C" fn raise_miss_counter(_: usize, ctx: &mut Context) {
    if let Some(at) = miss_counter(ctx) {
        // SAFETY: as above.
        let counter = unsafe { read::<u8>(at) };
        // SAFETY: as above.
        unsafe { write(at, counter.wrapping_add(1)) };
    }
}

const HIT_ROLL: Site = eix_sites::C_UNIT_SERVER_RESOLVE_HIT__108;
const HIT_ROLL_WAS: [u8; 6] = [0xDB, 0x45, 0xF4, 0xD8, 0x5D, 0xFC];
const HIT_ROLL_GO_ON: Site = eix_sites::C_UNIT_SERVER_RESOLVE_HIT__10E;
/// The two constant loads of the hit and miss marks, which the counter's update rides on.
const HIT_MARK_LOADS: [(Site, [u8; 6], Site); 2] = [
    (
        eix_sites::C_UNIT_SERVER_RESOLVE_HIT__115,
        [0xD9, 0x05, 0x3C, 0xF1, 0x73, 0x00],
        eix_sites::C_UNIT_SERVER_RESOLVE_HIT__11B,
    ),
    (
        RESOLVE_HIT_MARK,
        RESOLVE_HIT_MARK_WAS,
        eix_sites::C_UNIT_SERVER_RESOLVE_HIT__123,
    ),
];

/// `increasinghitchance` = 1: misses in a row raise the hit chance. Its stub at the second mark runs what
/// `newattackdefence` left there, so both apply (the original's stub runs the game's load, undoing that feature's).
pub(crate) fn increasing_hit_chance(
    ctx: &Ctx<'_>,
    value: i8,
) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let padding = f32::from_le_bytes([0x90; 4]);
    let check = ctx.values.byte("gameplay", "tohitcheck")?;
    let attack_defence = ctx.values.flag("gameplay", "newattackdefence")?;
    let stub = common::jump_stub(
        HIT_ROLL,
        &[],
        increased_hit_chance,
        check.into(),
        HIT_ROLL_GO_ON,
    )?;
    let mut writes = vec![
        common::float_at(
            TO_HIT_MUL,
            padding,
            ctx.values.float("gameplay", "tohitmul")?,
        )?,
        common::float_at(
            TO_HIT_SUB,
            padding,
            ctx.values.float("gameplay", "tohitsub")?,
        )?,
        common::branch(HIT_ROLL, &HIT_ROLL_WAS, &[0x90, 0xE9, 0, 0, 0, 0], 1, stub)?,
    ];
    for ((site, was, go_on), handler) in HIT_MARK_LOADS
        .into_iter()
        .zip([clear_miss_counter, raise_miss_counter])
    {
        let was = if site == RESOLVE_HIT_MARK {
            resolve_hit_mark(attack_defence)
        } else {
            was
        };
        let stub = common::jump_stub(site, &was, handler, 0, go_on)?;
        writes.push(common::branch(
            site,
            &was,
            &[0x90, 0xE9, 0, 0, 0, 0],
            1,
            stub,
        )?);
    }
    Ok(writes)
}

/// `meleesum_1`: value 1 is skipped by the registry (its hook faults); the shared function it writes is
/// [`melee_sum_server`], which `meleesum_low` still calls.
pub(crate) fn melee_sum_1(_: &Ctx<'_>, _: i8) -> Result<Vec<SiteWrite>, BuildError> {
    Ok(Vec::new())
}

/// Where a unit server keeps its first skill byte and its attack and defence values.
const STATS: usize = offset_of!(UnitServer, stats);
const FIRST_SKILL: u32 = (STATS + offset_of!(UnitStats, skills)) as u32;
const ATTACK_VALUE: u32 = (STATS + offset_of!(UnitStats, combat) + 4) as u32;
const DEFENCE_VALUE: u32 = (STATS + offset_of!(UnitStats, combat) + 12) as u32;

/// The function `meleesum_1` and `meleesum_2` both write at 0x799C68, which `meleesum_low` calls in
/// `CUnitServer::GetAttackParams` (EBP the unit server, ESI the parameters): attack and defence become the unit's
/// value plus its first skill byte plus `dexterity - 25`, each stored with `fistp`. The dexterity the call site
/// loads stays on the FPU stack for the site's `fstp st(0)`.
extern "C" fn melee_sum_server(_: usize, ctx: &mut Context) {
    let server = ctx.ebp;
    // SAFETY: EBP is the unit server whose parameters `GetAttackParams` builds; the original reads these fields.
    let dexterity = unsafe { read::<f32>(server + DEXTERITY) };
    // SAFETY: as above.
    let skill = unsafe { read::<u8>(server + FIRST_SKILL) };
    let above = Ext::new(dexterity.into())
        .fsub(game_float(TWENTY_FIVE).into())
        .to_f64();
    for (from, to) in [(ATTACK_VALUE, 0x18), (DEFENCE_VALUE, 0x1C)] {
        // SAFETY: as above.
        let value = unsafe { read::<f32>(server + from) };
        let sum = Ext::new(value.into())
            .fadd(skill.into())
            .fadd(above)
            .fistp();
        // SAFETY: ESI is the `AttackParams` being filled; +0x18 attack, +0x1C defence.
        unsafe { write(ctx.esi + to, sum) };
    }
}

/// `GetAttackParams` 0x555AF8 (`meleesum_low`): from the operand of `fld [ebp + 0x254]`, the two `_ftol` stores
/// become `fld [ebp + 0x1F8]; call 0x799C68; fstp st(0)` and NOPs.
const MELEE_LOW: Site = eix_sites::C_UNIT_SERVER_GET_ATTACK_PARAMS__F8;
const MELEE_LOW_WAS: [u8; 26] = [
    0x54, 0x02, 0x00, 0x00, 0xE8, 0x3B, 0x2A, 0x19, 0x00, 0x89, 0x46, 0x18, 0xD9, 0x85, 0x5C, 0x02,
    0x00, 0x00, 0xE8, 0x2D, 0x2A, 0x19, 0x00, 0x89, 0x46, 0x1C,
];
const MELEE_LOW_NOW: [u8; 26] = [
    0xF8, 0x01, 0x00, 0x00, 0xE8, 0, 0, 0, 0, 0xDD, 0xD8, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
];

/// `meleesum_low` = 1: units without a text key get their attack and defence from the sum. The registry skips it
/// unless `meleesum_1` or `meleesum_2` wrote the function it calls.
pub(crate) fn melee_sum_low(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let stub = common::call_stub(melee_sum_server, 0, MELEE_LOW.offset(9))?;
    Ok(vec![common::branch(
        MELEE_LOW,
        &MELEE_LOW_WAS,
        &MELEE_LOW_NOW,
        4,
        stub,
    )?])
}

/// The low byte of `a + b + c - 25`: the original adds whole words at the byte fields +0x40B and +0x33D, whose upper
/// bytes the mask drops.
fn melee_sum_byte(a: u32, unit: u32) -> u32 {
    // SAFETY: EDI's +0x1C is the client's unit; the original reads these words of it.
    let b = unsafe { read::<u32>(unit + 0x40B) };
    // SAFETY: as above.
    let c = unsafe { read::<u32>(unit + 0x33D) };
    a.wrapping_add(b).wrapping_add(c).wrapping_sub(0x19) & 0xFF
}

/// `CUnitClientSpecific::GetAttackParams` 0x538AB8 (`meleesum_2`): attack `[esi + 0x18]` is the byte ECX holds
/// plus the sum, defence (EAX, stored next) the unit's +0x34B byte plus the sum; on at 0x538ABD.
extern "C" fn melee_sum_client(_: usize, ctx: &mut Context) {
    // SAFETY: EDI is the client controller, whose +0x1C is its unit.
    let unit = unsafe { read::<u32>(ctx.edi + 0x1C) };
    ctx.ecx = melee_sum_byte(ctx.ecx, unit);
    // SAFETY: ESI is the `AttackParams` being filled.
    unsafe { write(ctx.esi + 0x18, ctx.ecx) };
    ctx.edx = unit;
    // SAFETY: as for `unit`.
    let defence = unsafe { read::<u8>(unit + 0x34B) };
    ctx.eax = melee_sum_byte(defence.into(), unit);
}

const MELEE_CLIENT: Site = eix_sites::C_UNIT_CLIENT_SPECIFIC_GET_ATTACK_PARAMS__151;
const MELEE_CLIENT_WAS: [u8; 12] = [
    0x89, 0x4E, 0x18, 0x8B, 0x57, 0x1C, 0x8A, 0x82, 0x4B, 0x03, 0x00, 0x00,
];
const MELEE_CLIENT_NOW: [u8; 12] = [0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0xE9, 0, 0, 0, 0];
const MELEE_CLIENT_GO_ON: Site = eix_sites::C_UNIT_CLIENT_SPECIFIC_GET_ATTACK_PARAMS__15D;

/// `meleesum_2` = 1: the client's attack and defence get the sum too.
pub(crate) fn melee_sum_2(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let stub = common::jump_stub(MELEE_CLIENT, &[], melee_sum_client, 0, MELEE_CLIENT_GO_ON)?;
    Ok(vec![common::branch(
        MELEE_CLIENT,
        &MELEE_CLIENT_WAS,
        &MELEE_CLIENT_NOW,
        7,
        stub,
    )?])
}

/// `newattackdefence`'s parameters in `.data`: `submod`, `divmod`.
const ATTACK_SUBMOD: Site = eix_sites::DATA_79A7F8;
const ATTACK_DIVMOD: Site = eix_sites::DATA_79A7FC;
/// `AttackRoll`'s weight constant (20.0, or `changeweightrangebonuses`' `weight`).
const ATTACK_WEIGHT: Site = WEIGHT_CONSTANT;
/// The damage factor G that `ResolveHit` computes and `ApplyHit` uses (values 1 and 2; the original's 0x79A7F4).
static ATTACK_FACTOR: AtomicU32 = AtomicU32::new(0);
/// What `AttackRoll` left below `ResolveHit`'s stack at 0x553E56: two integers (the attack and defence it
/// compared) and three floats (a speed term, a distance term and its divisor).
static ROLL_LEFTOVERS: Stale<5> = Stale::new();
const ROLL_LEFTOVER_OFFSETS: [i32; 5] = [-0x28, -0x44, -0x14, -0x38, -0x58];

/// `ResolveHit` 0x553E56, after `AttackRoll`: G from the roll's leftovers, then the replaced `mov al, [edi + 0x38];
/// test al, al`, going on past the `je` that follows it (0x553E5D).
extern "C" fn attack_factor(distance: usize, ctx: &mut Context) {
    let r = &ROLL_LEFTOVERS;
    let attack = f64::from(r.word(0) as i32);
    let defence = f64::from(r.word(1) as i32);
    let mut g = Ext::new(attack)
        .fsub(defence)
        .fdiv(game_float(ATTACK_DIVMOD).into())
        .fadd(game_float(ATTACK_SUBMOD).into())
        .fadd(r.float(2).into());
    if distance != 0 {
        let d = Ext::new(r.float(3).into())
            .fdiv(game_float(ATTACK_WEIGHT).into())
            .fdiv(r.float(4).into())
            .to_f64();
        g = Ext::new(g.to_f64()).fadd(d);
    }
    ATTACK_FACTOR.store(g.to_f32().to_bits(), Ordering::Relaxed);
    // SAFETY: EDI is the `DamageInfo` the original instruction reads.
    let mark = unsafe { read::<u8>(ctx.edi + 0x38) };
    ctx.eax = (ctx.eax & !0xFF) | u32::from(mark);
}

/// `ApplyHit` 0x5251DA (values 1 and 2): for damage types 0..2 (`[ebp - 0x10]`) the amount `[ebp - 8]` times G,
/// then the replaced `mov ebx, 1` and the moved `fcomp` (0x5251DF).
extern "C" fn apply_attack_factor(_: usize, ctx: &mut Context) {
    // SAFETY: `ApplyHit`'s frame: the damage type and the amount the original reads and writes.
    let kind = unsafe { read::<u32>(ctx.ebp.wrapping_sub(0x10)) };
    if kind <= 2 {
        let at = ctx.ebp.wrapping_sub(8);
        // SAFETY: as above.
        let amount = unsafe { read::<f32>(at) };
        let g = f32::from_bits(ATTACK_FACTOR.load(Ordering::Relaxed));
        // SAFETY: as above.
        unsafe { write(at, Ext::new(amount.into()).fmul(g.into()).to_f32()) };
    }
    ctx.ebx = 1;
}

/// The return address `ApplyHit`'s stack holds at +0x128 when `CUnitServer::DeliverAttack` 0x54F535 is the chain
/// that called it.
const DELIVER_ATTACK_RETURN: Site = eix_sites::C_UNIT_SERVER_DELIVER_ATTACK__E5;
/// The `.data` buffers the original fills with the attacker's and the defender's attack parameters.
const ATTACKER_PARAMS: Site = eix_sites::DATA_798188;
const DEFENDER_PARAMS: Site = eix_sites::DATA_798168;
/// 0.0 in `.rdata`: what a backstab subtracts instead of the defence.
const ZERO: Site = eix_sites::RDATA_73B858;
/// What the original passes in EDI to `CUnit::IsBehind`, which `backstabsforall`'s rewrite reads (`[edi + 0x264]`).
const IS_BEHIND_EDI: Site = eix_sites::DATA_7944F4;

/// `CUnit::IsBehind` 0x530A60 of `unit` and `target`, with EDI as the original sets it; the whole EAX, of
/// which the function sets only AL, as the original tests it.
///
/// # Safety
/// `unit` and `target` must be live units, on the game's thread.
unsafe fn is_behind(unit: u32, target: u32) -> u32 {
    let behind: u32;
    // SAFETY: 0x530A60 is `bool __thiscall (CUnit *target)`, `ret 4`; the caller guarantees the units.
    unsafe {
        core::arch::asm!(
            "push {target}",
            "call {f}",
            target = in(reg) target,
            f = in(reg) eix_sites::C_UNIT_IS_BEHIND.here(),
            in("ecx") unit,
            in("edi") IS_BEHIND_EDI.here(),
            lateout("eax") behind,
            clobber_abi("C"),
        );
    }
    behind
}

/// `ApplyHit` 0x5251E0 (values 3 and 4), in place of `mov ebx, 1`: for damage types 0..2 of a hit `DeliverAttack`
/// delivers, the amount `[ebp - 8]` times `max((attack - defence) * divmod + 1, submod)`, where a backstab counts no
/// defence. Then `mov eax, [ecx + 0xE0]`; EBX keeps its value (the original drops the `mov ebx, 1`).
extern "C" fn apply_attack_defence(_: usize, ctx: &mut Context) {
    scale_by_attack_defence(ctx);
    // SAFETY: the instruction that takes the place of `mov ebx, 1`; ECX is `ApplyHit`'s unit stats there.
    ctx.eax = unsafe { read::<u32>(ctx.ecx.wrapping_add(0xE0)) };
}

/// A word of the game's stack or of a game object, at an address the original reads.
fn word(address: u32) -> u32 {
    // SAFETY: only called for `ApplyHit`'s frame and stack and the unit servers its caller's frame names, as the
    // original reads them.
    unsafe { read::<u32>(address) }
}

fn scale_by_attack_defence(ctx: &Context) {
    let kind = word(ctx.ebp.wrapping_sub(0x10));
    let chain = word(ctx.stack() as u32 + 0x128);
    if kind > 2 || Site::of(chain as usize) != DELIVER_ATTACK_RETURN {
        return;
    }
    // The caller's frame (`DeliverAttack`) holds the attacker's and the defender's unit servers there.
    let attacker_at = ctx.ebp.wrapping_add(0xD8);
    let defender_at = ctx.ebp.wrapping_add(0x1C);
    let params = |server: u32, out: Site| {
        // SAFETY: a live unit server and the original's 0x20-byte `.data` buffer, on the game's thread.
        unsafe {
            AttackParams::of_server(
                core::ptr::with_exposed_provenance_mut(server as usize),
                out.ptr(),
            );
        }
    };
    let attacker = word(attacker_at);
    if attacker == 0 {
        return;
    }
    params(attacker, ATTACKER_PARAMS);
    let defender = word(defender_at);
    if defender == 0 {
        return;
    }
    params(defender, DEFENDER_PARAMS);
    // Re-read as the original does; a unit server's +0x1C is its unit.
    let unit = word(word(attacker_at) + 0x1C);
    if unit == 0 {
        return;
    }
    let target = word(word(defender_at) + 0x1C);
    // SAFETY: two live units.
    let behind = unsafe { is_behind(unit, target) };
    let attack = word(ATTACKER_PARAMS.offset(0x18).here32()) as i32;
    let minus = if behind != 0 {
        f64::from(game_float(ZERO))
    } else {
        f64::from(word(DEFENDER_PARAMS.offset(0x1C).here32()) as i32)
    };
    let factor = Ext::new(attack.into())
        .fsub(minus)
        .fmul(game_float(ATTACK_DIVMOD).into())
        .fadd(game_float(ONE).into())
        .to_f64();
    let floor = game_float(ATTACK_SUBMOD);
    let used = if factor > f64::from(floor) {
        factor
    } else {
        floor.into()
    };
    let at = ctx.ebp.wrapping_sub(8);
    // SAFETY: `ApplyHit`'s amount, as in `apply_attack_factor`.
    let amount = unsafe { read::<f32>(at) };
    // SAFETY: as above.
    unsafe { write(at, Ext::new(amount.into()).fmul(used).to_f32()) };
    // The original's `fcom` of the factor with `submod` is the comparison `ApplyHit`'s next `fnstsw` reads.
    // SAFETY: loads and pops one x87 register; the game's own stack stays as it was.
    unsafe {
        core::arch::asm!(
            "fld qword ptr [{f}]",
            "fcomp dword ptr [{s}]",
            f = in(reg) &factor,
            s = in(reg) ATTACK_SUBMOD.here(),
            options(nostack),
        );
    }
}

/// `ResolveHit` 0x553E56: `mov al, [edi + 0x38]; test al, al`.
const RESOLVE_ROLL: Site = eix_sites::C_UNIT_SERVER_RESOLVE_HIT__66;
const RESOLVE_ROLL_WAS: [u8; 5] = [0x8A, 0x47, 0x38, 0x84, 0xC0];
const RESOLVE_ROLL_GO_ON: Site = eix_sites::C_UNIT_SERVER_RESOLVE_HIT__6D;
/// `ResolveHit`'s load of the "hit" constant 0x73C1A8: NOPs (1, 2), or the "miss" constant 0x73F13C (4).
const RESOLVE_HIT_MARK: Site = eix_sites::C_UNIT_SERVER_RESOLVE_HIT__11D;
const RESOLVE_HIT_MARK_WAS: [u8; 6] = [0xD9, 0x05, 0xA8, 0xC1, 0x73, 0x00];

/// What `newattackdefence` = `value` leaves at [`RESOLVE_HIT_MARK`].
fn resolve_hit_mark(value: i8) -> [u8; 6] {
    match value {
        1 | 2 => [0x90; 6],
        4 => [0xD9, 0x05, 0x3C, 0xF1, 0x73, 0x00],
        _ => RESOLVE_HIT_MARK_WAS,
    }
}

/// `pop edi; pop esi; fnstsw ax; test ah, 0x41` at the end of `ResolveHit`: `nop; nop; jmp` to a copy.
const RESOLVE_END: Site = eix_sites::C_UNIT_SERVER_RESOLVE_HIT__12C;
const RESOLVE_END_WAS: [u8; 7] = [0x5F, 0x5E, 0xDF, 0xE0, 0xF6, 0xC4, 0x41];
/// `ApplyHit`'s `fcomp [0x73B84C]; mov ebx, 1`: `jmp` to the hook, then the `fcomp` moved down (1, 2).
const APPLY_HIT: Site = eix_sites::TEXT_5251DA;
const APPLY_HIT_WAS: [u8; 11] = [
    0xD8, 0x1D, 0x4C, 0xB8, 0x73, 0x00, 0xBB, 0x01, 0x00, 0x00, 0x00,
];
const APPLY_HIT_NOW: [u8; 11] = [0xE9, 0, 0, 0, 0, 0xD8, 0x1D, 0x4C, 0xB8, 0x73, 0x00];
const APPLY_HIT_GO_ON: Site = eix_sites::TEXT_5251DF;
/// `ApplyHit`'s `mov ebx, 1` (3, 4).
const APPLY_HIT_EBX: Site = eix_sites::TEXT_5251E0;
const APPLY_HIT_EBX_WAS: [u8; 5] = [0xBB, 0x01, 0x00, 0x00, 0x00];
const APPLY_HIT_EBX_GO_ON: Site = eix_sites::TEXT_5251E5;

/// `newattackdefence` 1 to 4: attack and defence scale damage instead of deciding misses.
///
/// The original's values 1 and 2 also write code at 0x79816A and 0x7981D8 that nothing reaches; it is left out.
pub(crate) fn new_attack_defence(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if !(1..=4).contains(&value) {
        return Ok(Vec::new());
    }
    let f = |key| ctx.values.float("gameplay", key);
    let mut writes = vec![
        common::float_at(ATTACK_DIVMOD, 0.0, f("divmod")?)?,
        common::float_at(ATTACK_SUBMOD, 0.0, f("submod")?)?,
    ];
    if value <= 2 {
        let stub = hook::stub(
            RESOLVE_ROLL,
            &ROLL_LEFTOVERS.copy(ROLL_LEFTOVER_OFFSETS),
            &[],
            false,
            attack_factor,
            usize::from(value == 1),
            RESOLVE_ROLL_GO_ON,
        )?;
        writes.push(common::branch(
            RESOLVE_ROLL,
            &RESOLVE_ROLL_WAS,
            &[0xE9, 0, 0, 0, 0],
            0,
            stub,
        )?);
        writes.push(SiteWrite::bytes(
            RESOLVE_HIT_MARK,
            &RESOLVE_HIT_MARK_WAS,
            &resolve_hit_mark(value),
        )?);
        let copy = hook::trampoline(RESOLVE_END, &RESOLVE_END_WAS)?;
        writes.push(common::branch(
            RESOLVE_END,
            &RESOLVE_END_WAS,
            &[0x90, 0x90, 0xE9, 0, 0, 0, 0],
            2,
            copy,
        )?);
        let stub = common::jump_stub(APPLY_HIT, &[], apply_attack_factor, 0, APPLY_HIT_GO_ON)?;
        writes.push(common::branch(
            APPLY_HIT,
            &APPLY_HIT_WAS,
            &APPLY_HIT_NOW,
            0,
            stub,
        )?);
    } else {
        let stub = common::jump_stub(
            APPLY_HIT_EBX,
            &[],
            apply_attack_defence,
            0,
            APPLY_HIT_EBX_GO_ON,
        )?;
        writes.push(common::branch(
            APPLY_HIT_EBX,
            &APPLY_HIT_EBX_WAS,
            &[0xE9, 0, 0, 0, 0],
            0,
            stub,
        )?);
        if value == 4 {
            // The original changes the constant's address alone; the whole `fld` gives another build all of it.
            writes.push(SiteWrite::bytes(
                RESOLVE_HIT_MARK,
                &RESOLVE_HIT_MARK_WAS,
                &resolve_hit_mark(value),
            )?);
        }
    }
    Ok(writes)
}

/// game.exe's constants 25.0 (the attribute every boost counts from) and 1.0, which the original loads from `.rdata`.
const TWENTY_FIVE: Site = eix_sites::RDATA_73E124;
const ONE: Site = eix_sites::RDATA_73B84C;

/// A `.rdata` or `.data` float of the game's.
fn game_float(site: Site) -> f32 {
    // SAFETY: the sites passed are game.exe's own constants and the parameters SpellAddon writes into `.data`.
    unsafe { read::<f32>(site.here32()) }
}

/// Where a unit server keeps strength, dexterity and intelligence.
const STRENGTH: u32 = (offset_of!(UnitServer, stats) + offset_of!(UnitStats, strength)) as u32;
const DEXTERITY: u32 = (offset_of!(UnitServer, stats) + offset_of!(UnitStats, dexterity)) as u32;
const INTELLIGENCE: u32 =
    (offset_of!(UnitServer, stats) + offset_of!(UnitStats, intelligence)) as u32;
/// A unit's controller.
const CONTROLLER: u32 = offset_of!(Unit, controller) as u32;

/// `damageboost`'s parameters in `.data`: `int`, `str`, `dex`.
const BOOST_INT: Site = eix_sites::DATA_799A64;
const BOOST_STR: Site = eix_sites::DATA_799A68;
const BOOST_DEX: Site = eix_sites::DATA_799A6C;
/// Value 1 divides by the parameters, the others multiply.
const BOOST_DIVIDE: usize = 1;
/// Value 5 boosts only spell effects whose cast set the mark.
const BOOST_MARKED: usize = 2;

/// `(stat - 25) / p` or `(stat - 25) * p`, as one x87 sequence.
fn boost_term(stat: f32, p: Site, flags: usize) -> f64 {
    let t = Ext::new(stat.into()).fsub(game_float(TWENTY_FIVE).into());
    let p = f64::from(game_float(p));
    if flags & BOOST_DIVIDE != 0 {
        t.fdiv(p)
    } else {
        t.fmul(p)
    }
    .to_f64()
}

/// The amount at `at` times `1 + term`, stored back: `fld [at]; ...; fadd [one]; fmulp; fstp [at]`.
///
/// # Safety
/// `at` must be a float of the game's that this point of its code lets change.
unsafe fn boost_by(at: u32, term: f64) {
    let factor = Ext::new(term).fadd(game_float(ONE).into()).to_f64();
    // SAFETY: the caller's guarantee.
    let amount = unsafe { read::<f32>(at) };
    // SAFETY: as above.
    unsafe { write(at, Ext::new(amount.into()).fmul(factor).to_f32()) };
}

/// The hit amount `[ebx + 0x24]` of the attacker `esi` (a `CUnitServer`) times `1 + str term + dex term`.
fn boost_hit(ctx: &Context, flags: usize) {
    // SAFETY: at the hooked points of `BuildAttackHit` ESI is the attacking unit server.
    let s = unsafe { read::<f32>(ctx.esi + STRENGTH) };
    // SAFETY: as above.
    let d = unsafe { read::<f32>(ctx.esi + DEXTERITY) };
    let s = boost_term(s, BOOST_STR, flags);
    let d = boost_term(d, BOOST_DEX, flags);
    // fadd [one] on the dexterity term, then faddp: the sum of the two terms and 1.
    let sum = Ext::new(d).fadd(game_float(ONE).into()).fadd(s).to_f64();
    let at = ctx.ebx + 0x24;
    // SAFETY: EBX is the `DamageInfo` being built; +0x24 its amount, which the next instructions store or read.
    let amount = unsafe { read::<f32>(at) };
    // SAFETY: as above.
    unsafe { write(at, Ext::new(amount.into()).fmul(sum).to_f32()) };
}

/// `BuildAttackHit` 0x54F6EB, after `RollDamage`: the amount, stored by the stub, is boosted; then the original's
/// jump to 0x54F7A6.
extern "C" fn boost_rolled_hit(flags: usize, ctx: &mut Context) {
    boost_hit(ctx, flags);
}

/// `BuildAttackHit`'s padding 0x54F84B (values 4 and 5): the amount after `ScaleDamageByEffects`, stored by the stub,
/// is boosted; then 0x54F7C9, past the moved store. Value 5 also sets the flags of `test eax, eax`.
extern "C" fn boost_scaled_hit(flags: usize, ctx: &mut Context) {
    boost_hit(ctx, flags);
    if flags & BOOST_MARKED != 0 {
        ctx.eflags = eix_core::x86_flags::cmp(ctx.eflags, ctx.eax, 0);
    }
}

/// The caster's intelligence term for the unit `unit` (a `CUnit`), when there is one.
fn intelligence_term(unit: u32, flags: usize) -> Option<f64> {
    if unit == 0 {
        return None;
    }
    // SAFETY: the hooks get a live unit, whose controller is its unit server on the server.
    let controller = unsafe { read::<u32>(unit + CONTROLLER) };
    // SAFETY: as above; the original reads the intelligence there the same way.
    let int = unsafe { read::<f32>(controller + INTELLIGENCE) };
    Some(boost_term(int, BOOST_INT, flags))
}

/// `CSpellEffect::Start` 0x67F902, before its jump through the effect-type table: the effect's value `[ebx + 0x58]`
/// times `1 + int term` of its caster `[ebx + 0x20]`; value 5 only when the cast marked it (`[ebx + 0x6F]` = 0xFF).
extern "C" fn boost_spell_effect(flags: usize, ctx: &mut Context) {
    // SAFETY: EBX is the starting `CSpellEffect`; +0x6F, +0x20 and +0x58 are what the original reads.
    let marked = unsafe { read::<u8>(ctx.ebx + 0x6F) } == 0xFF;
    if flags & BOOST_MARKED == 0 || marked {
        // SAFETY: as above.
        let caster = unsafe { read::<u32>(ctx.ebx + 0x20) };
        if let Some(term) = intelligence_term(caster, flags) {
            // SAFETY: the effect's value, which the effect's start reads after this point.
            unsafe { boost_by(ctx.ebx + 0x58, term) };
        }
    }
    let entry = EFFECT_START_TABLE
        .here32()
        .wrapping_add(ctx.ecx.wrapping_mul(4));
    // SAFETY: ECX is the effect type the original's `jmp [ecx*4 + 0x681AFC]` dispatches on, in the table's range.
    let target = unsafe { read::<u32>(entry) };
    // The table holds the running build's addresses.
    ctx.resume = Site::of(target as usize);
}

/// `CUnitServer::FinishCast` 0x54EE90 with value 4: the cast's amount `[ebx + 0x24]` times `1 + int term` of the
/// caster `[esi + 0x1C]`, then the replaced `mov ecx, [esi + 0x198]`.
extern "C" fn boost_cast(flags: usize, ctx: &mut Context) {
    // SAFETY: ESI is the casting unit server; +0x1C its unit.
    let caster = unsafe { read::<u32>(ctx.esi + 0x1C) };
    if let Some(term) = intelligence_term(caster, flags) {
        // SAFETY: EBX is the hit the cast built (tested non-null before the site); +0x24 its amount.
        unsafe { boost_by(ctx.ebx + 0x24, term) };
    }
    ctx.ecx = cast_target(ctx);
}

/// The replaced `mov ecx, [esi + 0x198]`.
fn cast_target(ctx: &Context) -> u32 {
    // SAFETY: ESI is the casting unit server, as at the original instruction.
    unsafe { read::<u32>(ctx.esi + 0x198) }
}

/// Value 5: `FinishCast` marks the hit (`[ebx + 0x3B]` = 0xFF) before the replaced `mov ecx, [esi + 0x198]`...
extern "C" fn mark_cast(_: usize, ctx: &mut Context) {
    // SAFETY: as in `boost_cast`; the original writes the byte there.
    unsafe { write::<u8>(ctx.ebx + 0x3B, 0xFF) };
    ctx.ecx = cast_target(ctx);
}

/// ...and clears the mark on its way out (0x54EFF5), then returns as the original does.
extern "C" fn unmark_cast(_: usize, ctx: &mut Context) {
    // SAFETY: EBX is the hit at both exits that reach 0x54EFF5 once the mark was set; the original writes it
    // unconditionally too.
    unsafe { write::<u8>(ctx.ebx + 0x3B, 0) };
}

/// `jmp [ecx*4 + 0x681AFC]`: `CSpellEffect::Start`'s effect-type table.
const EFFECT_START_TABLE: Site = eix_sites::C_SPELL_EFFECT_START__228C;
const EFFECT_START: Site = eix_sites::C_SPELL_EFFECT_START__92;
const EFFECT_START_WAS: [u8; 7] = [0xFF, 0x24, 0x8D, 0xFC, 0x1A, 0x68, 0x00];
const EFFECT_START_NOW: [u8; 7] = [0x90, 0x90, 0xE9, 0, 0, 0, 0];
/// `fstp [ebx + 0x24]; jmp 0x54F7A6` after `RollDamage`: the store goes into the stub.
const ROLLED_HIT: Site = eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__BB;
const ROLLED_HIT_WAS: [u8; 8] = [0xD9, 0x5B, 0x24, 0xE9, 0xB3, 0x00, 0x00, 0x00];
const ROLLED_HIT_NOW: [u8; 8] = [0x90, 0x90, 0x90, 0xE9, 0, 0, 0, 0];
const ROLLED_HIT_GO_ON: Site = eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__176;
/// `fstp dword [ebx + 0x24]`.
const STORE_AMOUNT: [u8; 3] = [0xD9, 0x5B, 0x24];
/// Value 3: `BuildAttackHit` jumps over `ScaleDamageByEffects` and drops its store; the client's
/// `GetAttackParams` jumps from 0x538B48 to 0x538B74.
const BOOST_SKIPS: &[Bytes] = &[
    (
        eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__176,
        &[0x8B, 0x53],
        &[0xEB, 0x13],
    ),
    (
        eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__191,
        &[0xD9, 0x5B, 0x24],
        &[0x90; 3],
    ),
    BOOST_CLIENT_SKIP,
];
const BOOST_CLIENT_SKIP: Bytes = (
    eix_sites::C_UNIT_CLIENT_SPECIFIC_GET_ATTACK_PARAMS__1E8,
    &[0x8B, 0x47],
    &[0xEB, 0x2A],
);
/// Where [`BOOST_CLIENT_SKIP`] jumps to.
const BOOST_CLIENT_GO_ON: Site = eix_sites::C_UNIT_CLIENT_SPECIFIC_GET_ATTACK_PARAMS__214;
/// Values 4 and 5: `fstp [ebx + 0x24]; test eax, eax; mov [ebx + 0x2C], ecx` becomes `mov [ebx + 0x2C], ecx; jmp
/// 0x54F842; fstp [ebx + 0x24]`, and the padding's last five bytes jump to the hook.
const SCALED_HIT_DETOUR: Bytes = (
    eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__191,
    &[0xD9, 0x5B, 0x24, 0x85, 0xC0, 0x89, 0x4B, 0x2C],
    &[0x89, 0x4B, 0x2C, 0xEB, 0x7C, 0xD9, 0x5B, 0x24],
);
const SCALED_HIT: Site = eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__212;
const SCALED_HIT_NOW: [u8; 14] = [
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0xE9, 0, 0, 0, 0,
];
const SCALED_HIT_GO_ON: Site = eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__199;
/// `FinishCast`'s `mov ecx, [esi + 0x198]`, after the hit is built.
const FINISH_CAST: Site = eix_sites::C_UNIT_SERVER_FINISH_CAST__80;
const FINISH_CAST_WAS: [u8; 6] = [0x8B, 0x8E, 0x98, 0x01, 0x00, 0x00];
const FINISH_CAST_GO_ON: Site = eix_sites::C_UNIT_SERVER_FINISH_CAST__86;
/// `FinishCast`'s exit `pop edi; pop esi; pop ebx; mov esp, ebp`, then `pop ebp; ret`.
const FINISH_CAST_EXIT: Site = eix_sites::C_UNIT_SERVER_FINISH_CAST__1E5;
const FINISH_CAST_EXIT_WAS: [u8; 5] = [0x5F, 0x5E, 0x5B, 0x8B, 0xE5];
const FINISH_CAST_EXIT_CODE: [u8; 7] = [0x5F, 0x5E, 0x5B, 0x8B, 0xE5, 0x5D, 0xC3];

/// `damageboost` 1 to 5: strength and dexterity scale hits, intelligence spell effects.
pub(crate) fn damage_boost(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if !(1..=5).contains(&value) {
        return Ok(Vec::new());
    }
    let f = |key| ctx.values.float("gameplay", key);
    let mut writes = vec![
        common::float_at(BOOST_INT, 0.0, f("int")?)?,
        common::float_at(BOOST_STR, 0.0, f("str")?)?,
        common::float_at(BOOST_DEX, 0.0, f("dex")?)?,
    ];
    let flags = match value {
        1 => BOOST_DIVIDE,
        5 => BOOST_MARKED,
        _ => 0,
    };
    let effect_start = |writes: &mut Vec<SiteWrite>| -> Result<(), BuildError> {
        let stub = common::jump_stub(EFFECT_START, &[], boost_spell_effect, flags, NOWHERE)?;
        writes.push(common::branch(
            EFFECT_START,
            &EFFECT_START_WAS,
            &EFFECT_START_NOW,
            2,
            stub,
        )?);
        Ok(())
    };
    match value {
        1 | 2 => {
            let stub = common::jump_stub(
                ROLLED_HIT,
                &STORE_AMOUNT,
                boost_rolled_hit,
                flags,
                ROLLED_HIT_GO_ON,
            )?;
            writes.push(common::branch(
                ROLLED_HIT,
                &ROLLED_HIT_WAS,
                &ROLLED_HIT_NOW,
                3,
                stub,
            )?);
            effect_start(&mut writes)?;
        }
        3 => writes.extend(common::fixed(BOOST_SKIPS)?),
        _ => {
            writes.extend(common::fixed(&[SCALED_HIT_DETOUR])?);
            let stub = common::jump_stub(
                SCALED_HIT,
                &STORE_AMOUNT,
                boost_scaled_hit,
                flags,
                SCALED_HIT_GO_ON,
            )?;
            writes.push(common::branch(
                SCALED_HIT,
                &[0x90; 14],
                &SCALED_HIT_NOW,
                9,
                stub,
            )?);
            if value == 4 {
                let stub =
                    common::jump_stub(FINISH_CAST, &[], boost_cast, flags, FINISH_CAST_GO_ON)?;
                writes.push(common::branch(
                    FINISH_CAST,
                    &FINISH_CAST_WAS,
                    &[0x90, 0xE9, 0, 0, 0, 0],
                    1,
                    stub,
                )?);
            } else {
                let stub = common::call_stub(mark_cast, 0, FINISH_CAST_GO_ON)?;
                writes.push(common::branch(
                    FINISH_CAST,
                    &FINISH_CAST_WAS,
                    &[0x90, 0xE8, 0, 0, 0, 0],
                    1,
                    stub,
                )?);
                let stub =
                    common::tail_stub(FINISH_CAST_EXIT, unmark_cast, 0, &FINISH_CAST_EXIT_CODE)?;
                writes.push(common::branch(
                    FINISH_CAST_EXIT,
                    &FINISH_CAST_EXIT_WAS,
                    &[0xE9, 0, 0, 0, 0],
                    0,
                    stub,
                )?);
                effect_start(&mut writes)?;
            }
        }
    }
    Ok(writes)
}

/// Both `GetAttackParams` copies: `je` over the `defence = 0` of bows and crossbows becomes a `jmp` past it.
const ARCHERS_DEFENCE: &[Bytes] = &[
    (
        eix_sites::C_UNIT_CLIENT_SPECIFIC_GET_ATTACK_PARAMS__1DA,
        &[0x74, 0x05],
        &[0xEB, 0x0C],
    ),
    (
        eix_sites::C_UNIT_SERVER_GET_ATTACK_PARAMS__16C,
        &[0x74, 0x05],
        &[0xEB, 0x0C],
    ),
];

/// `archersdefence` >= 1: archers keep their defence.
pub(crate) fn archers_defence(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    common::fixed(ARCHERS_DEFENCE)
}

/// `ApplyHit`'s `jle` to the part-breaking branch becomes a `jmp` past it (0x5252A8).
const BODY_PART_BREAK: Bytes = (eix_sites::TEXT_525247, &[0x7E, 0x45], &[0xEB, 0x5F]);
/// The `.rdata` constant -5.0 that the patch replaces with `bodypartbreakmodifier`.
const BODY_PART_FLOOR: Site = eix_sites::RDATA_73DDC0;

/// `unbreakablebodyparts` >= 1: parts no longer break.
pub(crate) fn unbreakable_body_parts(
    ctx: &Ctx<'_>,
    value: i8,
) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    let floor = ctx.values.float("gameplay", "bodypartbreakmodifier")?;
    Ok(vec![
        common::fixed(&[BODY_PART_BREAK])?.remove(0),
        common::float_at(BODY_PART_FLOOR, -5.0, floor)?,
    ])
}

/// `CUnitServer::TakeHit`'s `jne` that ignores a hit from another player's unit becomes NOPs. The original turns the
/// `je` before the god-mode test into a `jmp` instead, which drops god mode as well.
const KILLS_IN_MULTI: Bytes = (
    eix_sites::C_UNIT_SERVER_TAKE_HIT__227,
    &[0x0F, 0x85, 0x11, 0x02, 0x00, 0x00],
    NOP6,
);

/// `enablekillsinmulti` = 1: players can hurt players; god mode still holds.
pub(crate) fn kills_in_multi(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    common::fixed(&[KILLS_IN_MULTI])
}

const NOP2: &[u8] = &[0x90; 2];
const NOP6: &[u8] = &[0x90; 6];

/// `CUnit::IsBehind`'s two rejections of ranged weapons, and the ranged tests of `ExecuteOrder` and
/// `PrepareAttack` that keep a ranged attack off the backstab path.
const RANGED_BACKSTABS: &[Bytes] = &[
    (
        eix_sites::C_UNIT_IS_BEHIND__E1,
        &[0x0F, 0x84, 0x95, 0x00, 0x00, 0x00],
        NOP6,
    ),
    (
        eix_sites::C_UNIT_IS_BEHIND__F1,
        &[0x0F, 0x84, 0x85, 0x00, 0x00, 0x00],
        NOP6,
    ),
    (
        eix_sites::C_UNIT_SERVER_PREPARE_ATTACK__67,
        &[0x75, 0x20],
        NOP2,
    ),
    (
        eix_sites::C_UNIT_SERVER_PREPARE_ATTACK__5B,
        &[0x74, 0x0C],
        NOP2,
    ),
    (
        eix_sites::C_UNIT_SERVER_EXECUTE_ORDER__39C,
        &[0x74, 0x0C],
        NOP2,
    ),
    (
        eix_sites::C_UNIT_SERVER_EXECUTE_ORDER__3A8,
        &[0x75, 0x0F],
        NOP2,
    ),
    (
        eix_sites::C_UNIT_SERVER_EXECUTE_ORDER__390,
        &[0x74, 0x27],
        &[0xEB, 0x27],
    ),
];

/// `rangedweaponbackstabs` = 1: ranged weapons backstab.
pub(crate) fn ranged_weapon_backstabs(
    _: &Ctx<'_>,
    value: i8,
) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    common::fixed(RANGED_BACKSTABS)
}

/// The `jne` to the random spread in `CUnitServer::BuildAttackHit` and `CItemWeapon::RollDamage` become `jmp`s.
const ONLY_MIN_DAMAGE: &[Bytes] = &[
    (
        eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__10C,
        &[0x75],
        &[0xEB],
    ),
    (eix_sites::C_ITEM_WEAPON_ROLL_DAMAGE__11, &[0x75], &[0xEB]),
];

/// `useonlymindamage` = 1: every hit does the minimum damage.
pub(crate) fn only_min_damage(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    common::fixed(ONLY_MIN_DAMAGE)
}

/// `AttackRoll`'s weight (20.0) and range (10.0) constants in `.rdata`.
const WEIGHT_CONSTANT: Site = eix_sites::RDATA_73EED8;
const RANGE_CONSTANT: Site = eix_sites::RDATA_73EEDC;
/// `AttackRoll`+0x100: a `jne` that value 2 makes a `jmp`.
const WEIGHT_RANGE_TEST: Bytes = (eix_sites::C_UNIT_SERVER_ATTACK_ROLL__100, &[0x75], &[0xEB]);

/// `changeweightrangebonuses` >= 1: the weight and range constants come from the INI; 2 also drops a test.
pub(crate) fn weight_range_bonuses(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    let mut writes = vec![
        common::float_at(
            WEIGHT_CONSTANT,
            20.0,
            ctx.values.float("gameplay", "weight")?,
        )?,
        common::float_at(RANGE_CONSTANT, 10.0, ctx.values.float("gameplay", "range")?)?,
    ];
    if value == 2 {
        writes.extend(common::fixed(&[WEIGHT_RANGE_TEST])?);
    }
    Ok(writes)
}

/// The strength factors in `.rdata`: HP (0.005), damage (0.003), carried weight (12.0).
const STRENGTH_HP: Site = eix_sites::RDATA_73E130;
const STRENGTH_DAMAGE: Site = eix_sites::RDATA_73E134;
const STRENGTH_WEIGHT: Site = eix_sites::RDATA_73E138;
/// `CFlyingHitPoints::Draw`'s two `fadd [0x73E138]`, which the original points at 0x73E0FC through their operands'
/// low bytes: whole, so another build gets the whole address.
const FLYING_NUMBERS: &[Bytes] = &[
    (
        eix_sites::C_FLYING_HIT_POINTS_DRAW__6BB.back(2),
        &[0xD8, 0x05, 0x38, 0xE1, 0x73, 0x00],
        &[0xD8, 0x05, 0xFC, 0xE0, 0x73, 0x00],
    ),
    (
        eix_sites::C_FLYING_HIT_POINTS_DRAW__7FD.back(2),
        &[0xD8, 0x05, 0x38, 0xE1, 0x73, 0x00],
        &[0xD8, 0x05, 0xFC, 0xE0, 0x73, 0x00],
    ),
];

/// `strengthstats` = 1: the strength factors come from the INI.
pub(crate) fn strength_stats(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let f = |key| ctx.values.float("gameplay", key);
    let mut writes = vec![
        common::float_at(STRENGTH_HP, 0.005, f("strhp")?)?,
        common::float_at(STRENGTH_DAMAGE, 0.003, f("strdmg")?)?,
        common::float_at(STRENGTH_WEIGHT, 12.0, f("strweight")?)?,
    ];
    writes.extend(common::fixed(FLYING_NUMBERS)?);
    Ok(writes)
}

/// `CWorldServer::CreateUnits`' call of `CMapObject::ClearProtectionBases` for heroes.
const CLEAR_HERO_PROTECTION: Bytes = (
    eix_sites::C_WORLD_SERVER_CREATE_UNITS__6CB,
    &[0xE8, 0x90, 0xFA, 0xF7, 0xFF],
    &[0x90; 5],
);

/// `allowheroesarmors` = 1: heroes keep their natural armour.
pub(crate) fn heroes_armors(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    common::fixed(&[CLEAR_HERO_PROTECTION])
}

/// `CUnit::IsBehind`'s "no player: not behind" (`mov eax, [esi + 0x24C]; test eax, eax; jne +9; xor al, al; pop
/// esi; add esp, 0x18; ret 4`) becomes `mov eax, [edi + 0x264]; cmp eax, 3; jae 0x530BDE` and four NOPs.
const BACKSTABS_FOR_ALL: Bytes = (
    eix_sites::C_UNIT_IS_BEHIND__6,
    &[
        0x8B, 0x86, 0x4C, 0x02, 0x00, 0x00, 0x85, 0xC0, 0x75, 0x09, 0x32, 0xC0, 0x5E, 0x83, 0xC4,
        0x18, 0xC2, 0x04, 0x00,
    ],
    &[
        0x8B, 0x87, 0x64, 0x02, 0x00, 0x00, 0x83, 0xF8, 0x03, 0x0F, 0x83, 0x69, 0x01, 0x00, 0x00,
        0x90, 0x90, 0x90, 0x90,
    ],
);

/// `backstabsforall` = 1: units without a player can backstab.
pub(crate) fn backstabs_for_all(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    common::fixed(&[BACKSTABS_FOR_ALL])
}

/// `CUnitServer::ActionInReach`'s reach test from 0x549E80: the comparison with the attack range `[esi + 0xE8]`
/// gets `fmul [0x54A094]; fadd [0x54A098]` in front, and the rest moves down into the function's padding.
const REACH_TEST: Bytes = (
    eix_sites::C_UNIT_SERVER_ACTION_IN_REACH__140,
    &[
        0xC2, 0xD8, 0xCB, 0xDE, 0xC1, 0xD8, 0x9E, 0xE8, 0x00, 0x00, 0x00, 0xDF, 0xE0, 0xDD, 0xD8,
        0xF6, 0xC4, 0x01, 0xDD, 0xD8, 0x74, 0x0B, 0x5F, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x5E, 0x83,
        0xC4, 0x18, 0xC3, 0x5F, 0x33, 0xC0, 0x5E, 0x83, 0xC4, 0x18, 0xC3, 0x5F, 0xB0, 0x01, 0x5E,
        0x83, 0xC4, 0x18, 0xC3, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
        0x90, 0x90, 0x90, 0x90,
    ],
    &[
        // the bytes before the test, unchanged
        0xC2, 0xD8, 0xCB, 0xDE, 0xC1, //
        0xD8, 0x0D, 0x94, 0xA0, 0x54, 0x00, // fmul [0x54A094]
        0xD8, 0x05, 0x98, 0xA0, 0x54, 0x00, // fadd [0x54A098]
        0x90, 0x90, //
        0xD8, 0x9E, 0xE8, 0x00, 0x00, 0x00, // fcomp [esi + 0xE8]
        0xDF, 0xE0, 0xDD, 0xD8, 0xF6, 0xC4, 0x01, 0x90, 0xDD,
        0xD8, // fnstsw ax; fstp st0; test ah, 1; nop; fstp st0
        0x74, 0x0B, 0x5F, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x5E, 0x83, 0xC4, 0x18,
        0xC3, // the three returns
        0x5F, 0x33, 0xC0, 0x5E, 0x83, 0xC4, 0x18, 0xC3, //
        0x5F, 0xB0, 0x01, 0x5E, 0x83, 0xC4, 0x18, 0xC3,
    ],
);
/// The two early exits' `je 0x549EA9`, which go to the moved returns (0x549EB8) instead: whole, so another build
/// gets the whole displacement.
const REACH_EXITS: &[Bytes] = &[
    (
        eix_sites::C_UNIT_SERVER_ACTION_IN_REACH__31.back(2),
        &[0x0F, 0x84, 0x34, 0x01, 0x00, 0x00],
        &[0x0F, 0x84, 0x43, 0x01, 0x00, 0x00],
    ),
    (
        eix_sites::C_UNIT_SERVER_ACTION_IN_REACH__3A.back(2),
        &[0x0F, 0x84, 0x2B, 0x01, 0x00, 0x00],
        &[0x0F, 0x84, 0x3A, 0x01, 0x00, 0x00],
    ),
];
/// The function padding that holds `hitdistmul` and `hitdistadd`.
const HIT_DISTANCE_MUL: Site = eix_sites::C_UNIT_SERVER_FINISH_ACTION__1D4;
const HIT_DISTANCE_ADD: Site = eix_sites::C_UNIT_SERVER_FINISH_ACTION__1D8;

/// `runawaydistcontrol` = 1: an attack reaches `range * hitdistmul + hitdistadd`.
pub(crate) fn run_away_distance(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let padding = f32::from_le_bytes([0x90; 4]);
    let mut writes = common::fixed(&[REACH_TEST])?;
    writes.extend(common::fixed(REACH_EXITS)?);
    writes.push(common::float_at(
        HIT_DISTANCE_MUL,
        padding,
        ctx.values.float("gameplay", "hitdistmul")?,
    )?);
    writes.push(common::float_at(
        HIT_DISTANCE_ADD,
        padding,
        ctx.values.float("gameplay", "hitdistadd")?,
    )?);
    Ok(writes)
}

/// The weapon types `newrangedweapontypes` counts as ranged: bow, crossbow, and 11 and 15.
const RANGED_TYPES: [u32; 4] = [5, 6, 11, 15];

/// `CUnitServer::HasRangedWeapon` 0x54B6A0, in place of its two type tests: EAX, the weapon's type, becomes 1 for
/// a ranged type and 0 for any other.
extern "C" fn ranged_type(_: usize, ctx: &mut Context) {
    ctx.eax = is_ranged(ctx.eax);
}

fn is_ranged(weapon_type: u32) -> u32 {
    u32::from(RANGED_TYPES.contains(&weapon_type))
}

const HAS_RANGED: Site = eix_sites::C_UNIT_SERVER_HAS_RANGED_WEAPON__1D;
/// `cmp eax, 5; je +0x10; mov ecx, esi; call CItemWeapon::GetType; cmp eax, 6; je`.
const HAS_RANGED_WAS: [u8; 16] = [
    0x83, 0xF8, 0x05, 0x74, 0x10, 0x8B, 0xCE, 0xE8, 0x47, 0x51, 0x15, 0x00, 0x83, 0xF8, 0x06, 0x74,
];
/// `nop x3; call ..; nop x5; test eax, eax; jne`.
const HAS_RANGED_NOW: [u8; 16] = [
    0x90, 0x90, 0x90, 0xE8, 0, 0, 0, 0, 0x90, 0x90, 0x90, 0x90, 0x90, 0x85, 0xC0, 0x75,
];
/// `CUnitServer::HasRangedWeapon`, which the other tests now call.
const HAS_RANGED_WEAPON: Site = eix_sites::C_UNIT_SERVER_HAS_RANGED_WEAPON;

/// A weapon test of the unit server's callers: the unit's weapon and its two type compares (43 bytes, `was`)
/// become `mov ecx, <server>` (`server`), a call of `HasRangedWeapon` and NOPs, and the last compare's `cmp eax, 6`
/// becomes `cmp eax, <test>`.
struct RangedTest {
    site: Site,
    was: [u8; 43],
    server: [u8; 2],
    test: u8,
}

const RANGED_TESTS: [RangedTest; 5] = [
    // CUnitServer::AttacksFromRange
    RangedTest {
        site: eix_sites::C_UNIT_SERVER_ATTACKS_FROM_RANGE__16,
        was: [
            0x8B, 0x4E, 0x1C, 0x8B, 0x91, 0x6C, 0x02, 0x00, 0x00, 0x52, 0xE8, 0xCB, 0x61, 0xFE,
            0xFF, 0x8B, 0xF0, 0x85, 0xF6, 0x74, 0x1C, 0x8B, 0xCE, 0xE8, 0xFE, 0x50, 0x15, 0x00,
            0x83, 0xF8, 0x05, 0x74, 0x0C, 0x8B, 0xCE, 0xE8, 0xF2, 0x50, 0x15, 0x00, 0x83, 0xF8,
            0x06,
        ],
        server: [0x8B, 0xCE],
        test: 0,
    },
    // CUnitServer::CanReachTarget
    RangedTest {
        site: eix_sites::C_UNIT_SERVER_CAN_REACH_TARGET__4D,
        was: [
            0x8B, 0x4B, 0x1C, 0x8B, 0x81, 0x6C, 0x02, 0x00, 0x00, 0x50, 0xE8, 0x54, 0x50, 0xFE,
            0xFF, 0x8B, 0xF0, 0x85, 0xF6, 0x74, 0x18, 0x8B, 0xCE, 0xE8, 0x87, 0x3F, 0x15, 0x00,
            0x83, 0xF8, 0x05, 0x74, 0x39, 0x8B, 0xCE, 0xE8, 0x7B, 0x3F, 0x15, 0x00, 0x83, 0xF8,
            0x06,
        ],
        server: [0x89, 0xD9],
        test: 1,
    },
    // CUnitServer::CanReachAndPlanFrom
    RangedTest {
        site: eix_sites::C_UNIT_SERVER_CAN_REACH_AND_PLAN_FROM__45,
        was: [
            0x8B, 0x4E, 0x1C, 0x8B, 0x81, 0x6C, 0x02, 0x00, 0x00, 0x50, 0xE8, 0xFC, 0x4E, 0xFE,
            0xFF, 0x8B, 0xF8, 0x85, 0xFF, 0x74, 0x18, 0x8B, 0xCF, 0xE8, 0x2F, 0x3E, 0x15, 0x00,
            0x83, 0xF8, 0x05, 0x74, 0x2E, 0x8B, 0xCF, 0xE8, 0x23, 0x3E, 0x15, 0x00, 0x83, 0xF8,
            0x06,
        ],
        server: [0x8B, 0xCE],
        test: 1,
    },
    // CUnitServer::ExecuteOrder
    RangedTest {
        site: eix_sites::C_UNIT_SERVER_EXECUTE_ORDER__37D,
        was: [
            0x8B, 0x4E, 0x1C, 0x8B, 0x91, 0x6C, 0x02, 0x00, 0x00, 0x52, 0xE8, 0xC4, 0x3B, 0xFE,
            0xFF, 0x8B, 0xF8, 0x85, 0xFF, 0x74, 0x27, 0x8B, 0xCF, 0xE8, 0xF7, 0x2A, 0x15, 0x00,
            0x83, 0xF8, 0x05, 0x74, 0x0C, 0x8B, 0xCF, 0xE8, 0xEB, 0x2A, 0x15, 0x00, 0x83, 0xF8,
            0x06,
        ],
        server: [0x8B, 0xCE],
        test: 0,
    },
    // CUnitServer::OrderAttack
    RangedTest {
        site: eix_sites::C_UNIT_SERVER_ORDER_ATTACK__101,
        was: [
            0x8B, 0x4F, 0x1C, 0x8B, 0x91, 0x6C, 0x02, 0x00, 0x00, 0x52, 0xE8, 0x70, 0xCC, 0xFD,
            0xFF, 0x8B, 0xE8, 0x85, 0xED, 0x74, 0x18, 0x8B, 0xCD, 0xE8, 0xA3, 0xBB, 0x14, 0x00,
            0x83, 0xF8, 0x05, 0x74, 0x67, 0x8B, 0xCD, 0xE8, 0x97, 0xBB, 0x14, 0x00, 0x83, 0xF8,
            0x06,
        ],
        server: [0x89, 0xF9],
        test: 1,
    },
];
/// `AttacksFromRange`'s `jne` after the compare becomes `je`: it now leaves when the unit has no ranged weapon.
const ATTACKS_FROM_RANGE_JUMP: Bytes = (
    eix_sites::C_UNIT_SERVER_ATTACKS_FROM_RANGE__41,
    &[0x75],
    &[0x74],
);

impl RangedTest {
    fn write(&self) -> Result<SiteWrite, BuildError> {
        let mut now = [0x90u8; 43];
        now[2..4].copy_from_slice(&self.server);
        now[4] = 0xE8;
        now[5..9].copy_from_slice(&common::rel32(self.site.offset(9), HAS_RANGED_WEAPON.ru()));
        now[40..].copy_from_slice(&[0x83, 0xF8, self.test]);
        Ok(SiteWrite::bytes(self.site, &self.was, &now)?)
    }
}

/// `newrangedweapontypes` = 1: weapon types 11 and 15 count as ranged beside bows and crossbows, and every caller
/// that tested for a bow or crossbow asks `HasRangedWeapon` instead. With `rangedweaponbackstabs`, `ExecuteOrder`'s
/// test stays as that feature left it, which always takes the melee way (the original rewrites it, dropping that).
pub(crate) fn new_ranged_weapon_types(
    ctx: &Ctx<'_>,
    value: i8,
) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let backstabs = ctx.values.flag("gameplay", "rangedweaponbackstabs")? == 1;
    let stub = common::call_stub(ranged_type, 0, HAS_RANGED.offset(8))?;
    let mut writes = vec![common::branch(
        HAS_RANGED,
        &HAS_RANGED_WAS,
        &HAS_RANGED_NOW,
        3,
        stub,
    )?];
    for t in &RANGED_TESTS {
        if backstabs && t.site == eix_sites::C_UNIT_SERVER_EXECUTE_ORDER__37D {
            continue;
        }
        writes.push(t.write()?);
    }
    writes.extend(common::fixed(&[ATTACKS_FROM_RANGE_JUMP])?);
    Ok(writes)
}

/// `CSpellEffect::UpdateLasting`'s cases for spell kind 8 (value 1, 0x68290E) and kind 6 (value 2, 0x682AB0): both
/// begin `mov eax, [ebx + 0xA0]`.
const ACID_CASES: [Site; 2] = [
    eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__2BE,
    eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__460,
];
const ACID_CASE_WAS: [u8; 6] = [0x8B, 0x83, 0xA0, 0x00, 0x00, 0x00];

/// Nothing to add: the case goes on as the game's own code.
extern "C" fn acid_case(_: usize, _: &mut Context) {}

/// `acidattack` 1 or 2: the case's first instruction becomes a jump to code that does that instruction and goes on
/// with the case. The original's arena code runs a copy of the game's case either way (value 2 sends kind 8 to the
/// acid fog case, but only kind 6 reaches it), so nothing the game does changes; the author marked it unfinished.
pub(crate) fn acid_attack(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    let site = match value {
        1 => ACID_CASES[0],
        2 => ACID_CASES[1],
        _ => return Ok(Vec::new()),
    };
    let stub = common::jump_stub(
        site,
        &ACID_CASE_WAS,
        acid_case,
        0,
        site.offset(ACID_CASE_WAS.len()),
    )?;
    Ok(vec![common::branch(
        site,
        &ACID_CASE_WAS,
        &[0x90, 0xE9, 0, 0, 0, 0],
        1,
        stub,
    )?])
}

// --- Night power ---------------------------------------------------------------------------------------------------

/// `nightpower`'s parameters, which the original also keeps in padding at 0x500534.
#[derive(Debug, Clone, Copy)]
struct NightPower {
    /// `nightmeleeadd` (0x500534).
    add: f32,
    /// `nightmeleemul` (0x500538).
    mul: f32,
}

/// `nightpower` 2's factor of attack and defence for a unit of night vision `vision`, in the original's x87 order:
/// `add - P * mul + vision * P * mul / 100`, P the client world's night level (`CWorld::NightLevel` 0x55FC50).
fn night_factor(vision: Ext, night: &NightPower) -> Ext {
    type NightLevel = unsafe extern "thiscall" fn(world: u32) -> f32;
    // SAFETY: `CWorld::NightLevel` 0x55FC50 (thiscall, no arguments, a float).
    let night_level = unsafe { hook::as_fn::<NightLevel>(eix_sites::C_WORLD_NIGHT_LEVEL) };
    // SAFETY: the client world, a static object, on the game's thread.
    let level = Ext::new(unsafe { night_level(eix_sites::DATA_7AF5B8.here32()) }.into());
    let scaled = level.fmul(night.mul.into());
    let raised = vision.fmul_ext(scaled).fdiv(100.0);
    Ext::new(night.add.into()).fsub_ext(scaled).fadd_ext(raised)
}

/// Attack (+0x18) and defence (+0x1C) of the `AttackParams` at `params` times `factor`, rounded as `fistp` does.
///
/// # Safety
/// `params` must be a live `AttackParams`.
unsafe fn night_scale(params: u32, factor: Ext) {
    for at in [0x18, 0x1C] {
        // SAFETY: the caller vouches for the parameters.
        let value = unsafe { read::<i32>(params + at) };
        let scaled = Ext::new(value.into()).fmul_ext(factor).fistp();
        // SAFETY: as above.
        unsafe { write(params + at, scaled) };
    }
}

/// `CUnitServer::GetAttackParams` 0x555B7A (`mov eax, [esi + 0x10]; mov ecx, [ebp + 0x1C]`; EBP the controller, ESI
/// the parameters): with a unit, attack and defence times [`night_factor`] of its controller's night vision (+0x29C).
extern "C" fn night_server(arg: usize, ctx: &mut Context) {
    // SAFETY: the build keeps the parameters for the process.
    let night = unsafe { common::kept::<NightPower>(arg) };
    // SAFETY: EBP is the controller; +0x1C its unit.
    let unit = unsafe { read::<u32>(ctx.ebp + 0x1C) };
    if unit != 0 {
        // SAFETY: the unit's controller (+0x240).
        let controller = unsafe { read::<u32>(unit + 0x240) };
        // SAFETY: the controller's night vision.
        let vision = unsafe { read::<f32>(controller + 0x29C) };
        // SAFETY: ESI is the parameters being filled.
        unsafe { night_scale(ctx.esi, night_factor(Ext::new(vision.into()), night)) };
    }
    // SAFETY: as above.
    ctx.eax = unsafe { read::<u32>(ctx.esi + 0x10) };
    ctx.ecx = unit;
}

/// `CUnitClientSpecific::GetAttackParams` 0x538B48 (`mov eax, [edi + 0x1C]; mov ecx, [esi + 0x10]`): called from the
/// unit panel's `DrawUnitInfo` (its return address 0x6472DB, EBP that function's frame, whose `[ebp - 0x1C]` is the
/// shown unit's +0x330), the shown attack and defence times [`night_factor`] of the unit's byte +0x353.
extern "C" fn night_client(arg: usize, ctx: &mut Context) {
    night_client_scale(arg, ctx);
    // SAFETY: EDI is the client controller (+0x1C its unit), ESI the parameters.
    ctx.eax = unsafe { read::<u32>(ctx.edi + 0x1C) };
    // SAFETY: as above.
    ctx.ecx = unsafe { read::<u32>(ctx.esi + 0x10) };
}

/// [`night_client`]'s scaling alone.
extern "C" fn night_client_scale(arg: usize, ctx: &mut Context) {
    // SAFETY: the build keeps the parameters for the process.
    let night = unsafe { common::kept::<NightPower>(arg) };
    if ctx.stack_word(4).map(|w| Site::of(w as usize))
        == Some(eix_sites::C_HUD_UNIT_PANEL_DRAW_UNIT_INFO__29B)
    {
        // SAFETY: `DrawUnitInfo`'s frame and its pointer into the shown unit.
        let shown = unsafe { read::<u32>(ctx.ebp - 0x1C) };
        // SAFETY: the shown unit's byte.
        let vision = unsafe { read::<u8>(shown + 0x23) };
        // SAFETY: ESI is the parameters being filled.
        unsafe { night_scale(ctx.esi, night_factor(Ext::new(vision.into()), night)) };
    }
}

/// `nightpower` 2: at night attack and defence scale by [`night_factor`], on the server for every attack and on the
/// client for the unit panel. Value 1, whose server routine loads one value too few and so stores the integer
/// indefinite as attack and defence, is skipped by the registry. With `damageboost` 3, the client's hook goes on where
/// that feature's jump goes, so the panel keeps both (the original's hook drops the jump).
pub(crate) fn night_power(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 2 {
        return Ok(Vec::new());
    }
    let night = NightPower {
        add: ctx.values.float("gameplay", "nightmeleeadd")?,
        mul: ctx.values.float("gameplay", "nightmeleemul")?,
    };
    let mut params = [0u8; 8];
    params[..4].copy_from_slice(&night.add.to_le_bytes());
    params[4..].copy_from_slice(&night.mul.to_le_bytes());
    let arg = common::keep(night);
    let mut writes = vec![SiteWrite::bytes(
        eix_sites::C_SCENE_RENDERER_DESTRUCT_BASE__B4,
        &[0x90; 8],
        &params,
    )?];
    writes.extend(common::jump_at(
        eix_sites::C_UNIT_SERVER_GET_ATTACK_PARAMS__17A,
        &[0x8B, 0x46, 0x10, 0x8B, 0x4D, 0x1C],
        1,
        night_server,
        arg,
    )?);
    let (client, boost_was, boost_now) = BOOST_CLIENT_SKIP;
    let client_was = [0x8B, 0x47, 0x1C, 0x8B, 0x4E, 0x10];
    if ctx.values.flag("gameplay", "damageboost")? == 3 {
        let mut was = client_was;
        was[..boost_was.len()].copy_from_slice(boost_now);
        let stub = common::jump_stub(client, &[], night_client_scale, arg, BOOST_CLIENT_GO_ON)?;
        writes.push(common::branch(
            client,
            &was[..5],
            &[0xE9, 0, 0, 0, 0],
            0,
            stub,
        )?);
    } else {
        writes.extend(common::jump_at(client, &client_was, 1, night_client, arg)?);
    }
    Ok(writes)
}

// --- Aimed hits ----------------------------------------------------------------------------------------------------

/// `CUnitServer::BuildAttackSteps` 0x54F970 after `ActionDistance`'s call (`fstp [ebp + 0x14]; fld [ebp - 0x2C]`):
/// st(0) the reach, then the distance.
const AIM_REACH: Site = eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__EF;
const AIM_REACH_WAS: [u8; 6] = [0xD9, 0x5D, 0x14, 0xD9, 0x45, 0xD4];
/// `mov esi, 0x2000000`, ESI the target unit until then.
const AIM_TARGET: Site = eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__15D;
const AIM_TARGET_WAS: [u8; 5] = [0xBE, 0x00, 0x00, 0x00, 0x02];
/// The displacement of the `jl` that takes an aim mode of 0 to 5 past `PickLocation`; the original points it at the
/// second half of [`AIM_PICKED`], past the chooser.
const AIM_FIXED: Site = eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__23F;
/// After `PickLocation` (EAX its part): `fld [ebp - 0x40]; fadd [ebp + 0x14]`, then `mov [ebp - 0x2C], eax;
/// fst [ebp - 0x70]`. The original turns each half into `nop; jmp`: to the chooser, and to a copy of the whole.
const AIM_PICKED: Site = eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__24A;
const AIM_PICKED_WAS: [u8; 12] = [
    0xD9, 0x45, 0xC0, 0xD8, 0x45, 0x14, 0x89, 0x45, 0xD4, 0xD9, 0x55, 0x90,
];

/// The reach and the target of the attack `BuildAttackSteps` is building, kept by its first two sites for the chooser
/// at the third in the same call, on the game's thread (the original keeps them at 0x79A8DC and 0x79A924).
static AIMED_REACH: AtomicU32 = AtomicU32::new(0);
static AIMED_TARGET: AtomicU32 = AtomicU32::new(0);

/// Where the chooser takes the attacker's damage from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AimDamage {
    /// Values 1, 3, 4: the attacker's attack block as its last hit left it, one amount (so no expected damage).
    LastHit,
    /// Values 5, 8: the smallest and the largest amount of its weapon or its own attack.
    Range,
}

/// The chooser's settings.
#[derive(Debug, Clone, Copy)]
struct Aim {
    damage: AimDamage,
    /// `bodypart`: the part when every score is 0, and an attacker's with a player under `checkhero`.
    fallback: u32,
    /// `checkhero` with [`AimDamage::Range`]: an attacker that has a player aims at `bodypart`.
    check_hero: bool,
    /// `headmod`, `handmod`, `legmod`: the parts' prices.
    head: f32,
    hand: f32,
    leg: f32,
    /// `criticalmod`: the head's factor when a hit there takes all its hit points, and the legs' for a ranged attacker.
    critical: f32,
    /// `healthdowntoignore`: a part whose hit points times this are below its maximum is not aimed at.
    health_down_to_ignore: f32,
    /// `rangeddistance`: the reach from which an attacker counts as ranged.
    ranged_distance: f32,
}

/// The attacker's damage the chooser weighs.
#[derive(Debug, Clone, Copy, Default)]
struct AimAttack {
    /// The weight of each damage type.
    weights: [f32; 7],
    /// The smallest and the largest amount.
    min: f32,
    max: f32,
    /// The block's multiplier of what a layer takes (its +0x28).
    absorb_factor: f32,
    /// The attack's reach.
    reach: f32,
}

/// What the chooser reads of a target's part (a `UnitStatSlot`).
#[derive(Debug, Clone, Copy, Default)]
struct AimSlot {
    kind: i32,
    current: f32,
    maximum: f32,
    share: f32,
    /// The slot's +0xF0.
    scale: f32,
    /// What each armour layer takes from each damage type.
    absorb: [[f32; 7]; 3],
}

/// What the chooser reads of the target.
#[derive(Debug, Clone, Copy, Default)]
struct AimTarget {
    slots: [AimSlot; 6],
    /// What the unit's map-object companion takes from each damage type after the middle layer (its +0x44 + 8 * type).
    resist: [f32; 7],
}

/// The damage left of the largest hit on `slot`, as the original's copy of the game's walk over a part's layers
/// computes it: for each damage type with damage, its amount less each layer from the outermost (and with `resist`,
/// value 5's, the companion's resistance after the middle one; value 1's copy has that step NOPed) until nothing is
/// left. What the last such type leaves stands; nothing for a part whose kind is below 2 or a hit without damage.
fn aim_residual(slot: &AimSlot, resist: Option<&[f32; 7]>, attack: &AimAttack) -> f32 {
    if slot.kind < 2 {
        return 0.0;
    }
    let mut left = 0.0;
    for (t, &weight) in attack.weights.iter().enumerate() {
        let mut x = weight * attack.max;
        if !positive(x) {
            continue;
        }
        for layer in (0..3).rev() {
            x -= attack.absorb_factor * slot.absorb[layer][t];
            if let (1, Some(resist)) = (layer, resist) {
                x -= resist[t];
            }
            if !positive(x) {
                break;
            }
        }
        left = x;
    }
    left
}

/// Above 0: false for NaN, as the original's `fcomp 0; test ah, 0x41` takes an unordered result.
fn positive(x: f32) -> bool {
    x > 0.0
}

/// `a` below `b` or unordered: the x87 compare's C0.
fn below_or_unordered(a: f32, b: f32) -> bool {
    matches!(a.partial_cmp(&b), None | Some(core::cmp::Ordering::Less))
}

/// The part (0 head, 1 body, 2 and 3 the hands, 4 and 5 the legs) `randomlessaimedhits` aims at: each part's damage
/// left of the largest hit becomes the expected damage over the amount's range (none for a single amount), scaled by
/// the part's share of the hit points, priced, and zeroed for a part too damaged to aim at; the highest score wins
/// (the later part on a tie), `bodypart` when the scores sum to 0. The comparisons take an unordered result as the
/// original's x87 tests do.
fn aim_choice(target: &AimTarget, attack: &AimAttack, aim: &Aim) -> u32 {
    let mut s = [0.0f32; 6];
    let resist = (aim.damage == AimDamage::Range).then_some(&target.resist);
    for (score, slot) in s.iter_mut().zip(&target.slots) {
        *score = aim_residual(slot, resist, attack);
    }
    let (min, max) = (attack.min, attack.max);
    let body_share = target.slots[1].share;
    for (score, slot) in s.iter_mut().zip(&target.slots) {
        // Equal or unordered skips, as the original's `fcomp; test ah, 0x40`.
        if matches!(
            min.partial_cmp(&max),
            Some(core::cmp::Ordering::Less | core::cmp::Ordering::Greater)
        ) {
            let v = *score;
            let t = max - v;
            // A residual below 0 also takes the quadratic way, as the original's.
            *score = if !below_or_unordered(v, 0.0) && below_or_unordered(t, min) {
                (min + max) * 0.5 - t
            } else {
                v * v / ((max - min) * 2.0)
            };
        }
        *score *= body_share / slot.share * slot.scale;
    }
    if s[0] >= target.slots[0].maximum {
        s[0] *= aim.critical;
    }
    s[0] *= aim.head;
    s[2] *= aim.hand;
    s[3] *= aim.hand;
    s[4] *= aim.leg;
    s[5] *= aim.leg;
    for (p, slot) in target.slots.iter().enumerate() {
        if p != 1 && below_or_unordered(slot.current * aim.health_down_to_ignore, slot.maximum) {
            s[p] = 0.0;
        }
    }
    if attack.reach >= aim.ranged_distance {
        s[4] *= aim.critical;
        s[5] *= aim.critical;
    }
    // The original's argmax runs six times over; a pass after the first can only move to an equal score.
    let mut best = 0;
    for _ in 0..6 {
        for i in 0..6 {
            if s[i] >= s[best] {
                best = i;
            }
        }
    }
    let sum: f32 = s.iter().sum();
    if sum == 0.0 || sum.is_nan() {
        aim.fallback
    } else {
        best as u32
    }
}

/// The attacker's damage as the original's copy of the game's damage builder (0x54F630) computes it, without the
/// builder's random roll: the weights of the damage types and the smallest or largest amount, after
/// `ScaleDamageByEffects`. A hero's weapon in hand gives them; else the unit's own attack.
///
/// # Safety
/// `server` must be a live `CUnitServer`, on the game's thread.
unsafe fn aim_damage(server: u32, largest: bool) -> ([f32; 7], f32) {
    type GetDamageTypes = unsafe extern "thiscall" fn(weapon: u32, out: *mut [f32; 7]);
    type ScaleDamageByEffects =
        unsafe extern "thiscall" fn(stats: u32, unit: u32, amount: f32) -> f32;
    let stats_at = offset_of!(UnitServer, stats) as u32;
    // SAFETY: the caller vouches for the controller.
    let ctl = unsafe { common::Obj::new(server, size_of::<UnitServer>() as u32) };
    let unit: u32 = ctl.get(offset_of!(UnitServer, unit) as u32);
    // SAFETY: a controller's live unit.
    let u = unsafe { common::Obj::new(unit, size_of::<Unit>() as u32) };
    let slot: u32 = u.get(0x26C);
    let weapon: u32 = if common::is_hero_id(u.get(0xC)) && slot < 4 {
        u.get(0x440 + 4 * slot)
    } else {
        0
    };
    let mut weights = [0.0f32; 7];
    let (low, spread): (f32, f32) = if weapon == 0 {
        for (t, w) in weights.iter_mut().enumerate() {
            *w = ctl.get(stats_at + offset_of!(UnitStats, damage_weights) as u32 + 4 * t as u32);
        }
        (
            ctl.get(stats_at + offset_of!(UnitStats, damage) as u32),
            ctl.get(stats_at + offset_of!(UnitStats, damage_spread) as u32),
        )
    } else {
        // SAFETY: the game's function of this signature.
        let get_types =
            unsafe { hook::as_fn::<GetDamageTypes>(eix_sites::C_ITEM_WEAPON_GET_DAMAGE_TYPES) };
        // SAFETY: the unit's weapon in hand, which writes seven floats.
        unsafe { get_types(weapon, &raw mut weights) };
        // SAFETY: the weapon item, whose +0x50 and +0x54 are its damage and spread.
        let w = unsafe { common::Obj::new(weapon, 0x58) };
        (w.get(0x50), w.get(0x54))
    };
    let amount = if largest { low + spread } else { low };
    // SAFETY: the game's function of this signature.
    let scale = unsafe {
        hook::as_fn::<ScaleDamageByEffects>(eix_sites::SCALE_DAMAGE_BY_EFFECTS_AT_523D30)
    };
    // SAFETY: the controller's stats and its unit, on the game's thread.
    (weights, unsafe { scale(server + stats_at, unit, amount) })
}

/// The chooser's view of `target`.
///
/// # Safety
/// `target` must be a live `CUnit` with a controller and a companion, on the game's thread.
unsafe fn aim_target(target: u32) -> AimTarget {
    // SAFETY: the caller vouches for the unit.
    let u = unsafe { common::Obj::new(target, size_of::<Unit>() as u32) };
    let server: u32 = u.get(offset_of!(Unit, controller) as u32);
    let companion: u32 = u.get(offset_of!(MapObject, companion) as u32);
    // SAFETY: the unit's live stats, in its controller.
    let stats = unsafe {
        common::Obj::new(
            server + offset_of!(UnitServer, stats) as u32,
            size_of::<UnitStats>() as u32,
        )
    };
    // SAFETY: the unit's companion, whose resistances end at +0x44 + 8 * 6.
    let comp = unsafe { common::Obj::new(companion, 0x44 + 8 * 7) };
    let mut out = AimTarget::default();
    for (t, r) in out.resist.iter_mut().enumerate() {
        *r = comp.get(0x44 + 8 * t as u32);
    }
    for (p, slot) in out.slots.iter_mut().enumerate() {
        let at = (offset_of!(UnitStats, slots) + p * size_of::<UnitStatSlot>()) as u32;
        let field = |o: usize| at + o as u32;
        slot.kind = stats.get(field(offset_of!(UnitStatSlot, kind)));
        slot.current = stats.get(field(offset_of!(UnitStatSlot, current)));
        slot.maximum = stats.get(field(offset_of!(UnitStatSlot, maximum)));
        slot.share = stats.get(field(offset_of!(UnitStatSlot, share)));
        slot.scale = stats.get(field(0xF0));
        for (layer, absorb) in slot.absorb.iter_mut().enumerate() {
            let base = offset_of!(UnitStatSlot, layers)
                + layer * size_of::<UnitStatLayer>()
                + offset_of!(UnitStatLayer, absorb);
            for (t, a) in absorb.iter_mut().enumerate() {
                *a = stats.get(field(base + 4 * t));
            }
        }
    }
    out
}

/// `BuildAttackSteps` after its class test (0x54F9B5, `xor eax, eax; push ebx; mov [ebp - 0x54], eax`; EDI the
/// attacker's controller, ESI the target unit), with `randomlessaimedhits` 2, 6, 7 and 9.
const AIM_START: Site = eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__45;
const AIM_START_WAS: [u8; 6] = [0x33, 0xC0, 0x53, 0x89, 0x45, 0xAC];

/// The target's reach against the attacker that [`aim_start`] measured, for [`aim_reach`] in the same call.
static AIMED_OTHER_REACH: AtomicU32 = AtomicU32::new(0);

/// [`AIM_START`]: `ActionDistance` with the attacker's unit swapped for the target's and the target being the
/// attacker, that is the reach the target would have against it (its weapon in hand, the radii, the floor).
/// The original reruns the function's start that far with the unit swapped, and leaks the attacker's reference
/// that its second run's start overwrites; ours calls `ActionDistance` on an order block of its own.
extern "C" fn aim_start(_: usize, ctx: &mut Context) {
    type ActionDistance = unsafe extern "thiscall" fn(server: u32, order: *const u32) -> f32;
    let server = ctx.edi;
    let unit_at = server + offset_of!(UnitServer, unit) as u32;
    // SAFETY: EDI is the attacker's live controller; +0x1C its unit.
    let attacker = unsafe { read::<u32>(unit_at) };
    // An attack order as the function builds it: kind 3, 6, the target (here the attacker), no flag at +0x18.
    let order: [u32; 8] = [3, 6, attacker, 0, 0, 0, 0, 0];
    // SAFETY: as above; the swap is undone before the game goes on.
    unsafe { common::write::<u32>(unit_at, ctx.esi) };
    // SAFETY: `CUnitServer::ActionDistance` 0x54C070 (thiscall, the order; `ret 4`).
    let distance =
        unsafe { hook::as_fn::<ActionDistance>(eix_sites::C_UNIT_SERVER_ACTION_DISTANCE) };
    // SAFETY: the controller and an order of the layout the function reads.
    let reach = unsafe { distance(server, order.as_ptr()) };
    // SAFETY: as above.
    unsafe { common::write::<u32>(unit_at, attacker) };
    AIMED_OTHER_REACH.store(reach.to_bits(), Ordering::Relaxed);
}

/// [`AIM_REACH`]: keeps the reach, stores it in `[ebp + 0x14]` as the game does and loads the distance. With
/// [`aim_start`] (`arg` 1) what the chooser compares with `rangeddistance` is the target's reach less the attacker's.
/// The original pops the stack twice there, which fits the start hook's extra value; without it the game's slot gets
/// the invalid operation's NaN and its later minimum of distance and reach always takes the distance; ours keeps the
/// game's minimum.
extern "C" fn aim_reach(arg: usize, ctx: &mut FpuContext) {
    let reach = ctx.fpu.pop().map_or(0.0, Ext::to_f32);
    let compared = if arg == 1 {
        f32::from_bits(AIMED_OTHER_REACH.load(Ordering::Relaxed)) - reach
    } else {
        reach
    };
    AIMED_REACH.store(compared.to_bits(), Ordering::Relaxed);
    // SAFETY: `BuildAttackSteps`' frame: the distance at EBP-0x2C, its argument slot EBP+0x14.
    let frame = unsafe { common::Obj::new(ctx.regs.ebp - 0x2C, 0x2C + 0x18) };
    frame.set(0x2C + 0x14, reach);
    let distance: f32 = frame.get(0);
    // The pop above left room.
    let _ = ctx.fpu.push(Ext::new(distance.into()));
}

/// [`AIM_TARGET`]: keeps the target, then the replaced `mov esi, 0x2000000`.
extern "C" fn aim_target_site(_: usize, ctx: &mut Context) {
    AIMED_TARGET.store(ctx.esi, Ordering::Relaxed);
    ctx.esi = 0x200_0000;
}

/// The attacker's attack block (`CUnitServer`+0x108, a `DamageInfo`) as its last hit left it: the weights at +0x0,
/// the amount at +0x24 (as smallest and largest), the layers' multiplier at +0x28.
///
/// # Safety
/// `server` must be a live `CUnitServer`.
unsafe fn aim_last_hit(server: u32) -> AimAttack {
    // SAFETY: the caller vouches for the controller, whose block ends at +0x108 + 0x3C.
    let block = unsafe { common::Obj::new(server + 0x108, 0x3C) };
    let mut attack = AimAttack::default();
    for (t, w) in attack.weights.iter_mut().enumerate() {
        *w = block.get(4 * t as u32);
    }
    attack.max = block.get(0x24);
    attack.min = attack.max;
    attack.absorb_factor = block.get(0x28);
    attack
}

/// `UnitStats::Reduce` 0x5259B0 (thiscall, `ret 4`): the hit points left of `full` by the parts' damage.
type Reduce = unsafe extern "thiscall" fn(stats: u32, full: f32) -> f32;

/// [`AIM_PICKED`] after `PickLocation`: EAX becomes the chosen part ([`aim_choice`]), and the replaced instructions'
/// copy runs. The original's copy of the damage builder writes the attacker's attack block (its +0x108), which the
/// game refills before the hit (0x54DD2B), and leaves an x87 value behind for an attacker without a weapon; ours
/// computes aside. Its player test reads through the attacker's pointer with the low byte replaced; ours tests the
/// attacker. Values 1, 3, 4 also set the target companion's hit points (+0x28) from `Reduce` of its +0x2C when a part
/// scores, as the original's copy of the game's walk does at its end.
extern "C" fn aim_part(arg: usize, ctx: &mut Context) {
    // SAFETY: the build keeps the settings for the process.
    let aim = unsafe { common::kept::<Aim>(arg) };
    ctx.resume = AIM_PICKED.offset(6);
    let server = ctx.edi;
    let target_unit = AIMED_TARGET.load(Ordering::Relaxed);
    // SAFETY: the target the same call kept at [`AIM_TARGET`], a unit (the function's class test).
    let target = unsafe { aim_target(target_unit) };
    let mut attack = match aim.damage {
        AimDamage::Range => {
            // SAFETY: EDI is the attacker's `CUnitServer` `BuildAttackSteps` runs on.
            let unit: u32 = unsafe { read(server + offset_of!(UnitServer, unit) as u32) };
            // SAFETY: the controller's live unit.
            if aim.check_hero && unsafe { common::has_player(unit) } {
                ctx.eax = aim.fallback;
                return;
            }
            // SAFETY: as above.
            let (_, min) = unsafe { aim_damage(server, false) };
            // SAFETY: as above.
            let (weights, max) = unsafe { aim_damage(server, true) };
            AimAttack {
                weights,
                min,
                max,
                absorb_factor: 1.0,
                reach: 0.0,
            }
        }
        AimDamage::LastHit => {
            if target.slots.iter().any(|s| s.kind >= 2) {
                // SAFETY: the target unit's companion and controller.
                unsafe { aim_refresh_hit_points(target_unit) };
            }
            // SAFETY: as above.
            unsafe { aim_last_hit(server) }
        }
    };
    attack.reach = f32::from_bits(AIMED_REACH.load(Ordering::Relaxed));
    ctx.eax = aim_choice(&target, &attack, aim);
}

/// The target companion's hit points (+0x28) become `Reduce` of its +0x2C.
///
/// # Safety
/// `target` must be a live `CUnit` with a controller and a companion, on the game's thread.
unsafe fn aim_refresh_hit_points(target: u32) {
    // SAFETY: the caller vouches for the unit.
    let u = unsafe { common::Obj::new(target, size_of::<Unit>() as u32) };
    let server: u32 = u.get(offset_of!(Unit, controller) as u32);
    // SAFETY: the unit's companion, whose +0x28 and +0x2C the game's walk updates.
    let comp = unsafe { common::Obj::new(u.get(offset_of!(MapObject, companion) as u32), 0x30) };
    // SAFETY: the game's function of this signature.
    let reduce = unsafe { hook::as_fn::<Reduce>(eix_sites::UNIT_STATS_REDUCE) };
    // SAFETY: the unit's stats, on the game's thread.
    let left = unsafe {
        reduce(
            server + offset_of!(UnitServer, stats) as u32,
            comp.get(0x2C),
        )
    };
    comp.set(0x28, left);
}

/// `randomlessaimedhits`: 1 to 4 and 5 to 9 (the same chooser within each group in the original) aim at the part
/// [`aim_choice`] gives, where the game would pick one at random ([`AimDamage`]); 2, 6, 7 and 9 compare the reach
/// difference ([`aim_start`]). 10, whose damage call goes to arena code the original never writes, is skipped by the
/// registry.
pub(crate) fn randomless_aimed_hits(
    ctx: &Ctx<'_>,
    value: i8,
) -> Result<Vec<SiteWrite>, BuildError> {
    let damage = match value {
        1..=4 => AimDamage::LastHit,
        5..=9 => AimDamage::Range,
        _ => return Ok(Vec::new()),
    };
    let start = matches!(value, 2 | 6 | 7 | 9);
    let v = ctx.values;
    let aim = Aim {
        damage,
        fallback: v.int("gameplay", "bodypart")? as u32,
        check_hero: v.int("gameplay", "checkhero")? != 0,
        head: v.float("gameplay", "headmod")?,
        hand: v.float("gameplay", "handmod")?,
        leg: v.float("gameplay", "legmod")?,
        critical: v.float("gameplay", "criticalmod")?,
        health_down_to_ignore: v.float("gameplay", "healthdowntoignore")?,
        ranged_distance: v.float("gameplay", "rangeddistance")?,
    };
    let chooser = common::entry(aim_part, common::keep(aim))?;
    let copy = hook::trampoline(AIM_PICKED, &AIM_PICKED_WAS)?;
    let mut writes = Vec::new();
    if start {
        // `nop; jmp`, the stub running the replaced instructions after the handler, as the original's tail does.
        let stub = hook::stub(
            AIM_START,
            &[],
            &AIM_START_WAS,
            false,
            aim_start,
            0,
            AIM_START.offset(AIM_START_WAS.len()),
        )?;
        writes.push(common::branch(
            AIM_START,
            &AIM_START_WAS,
            &[0x90, 0xE9, 0, 0, 0, 0],
            1,
            stub,
        )?);
    }
    writes.extend([
        SiteWrite::nop(AIM_REACH, &AIM_REACH_WAS[..1]),
        hook::fpu_hook(
            AIM_REACH.offset(1),
            &AIM_REACH_WAS[1..],
            &AIM_REACH_WAS[1..],
            Mode::Drop,
            aim_reach,
            usize::from(start),
        )?,
        hook::context_hook(AIM_TARGET, &AIM_TARGET_WAS, Mode::Drop, aim_target_site, 0)?,
        SiteWrite::bytes(AIM_FIXED, &[0x0A], &[0x10])?,
        common::branch(
            AIM_PICKED,
            &AIM_PICKED_WAS[..6],
            &[0x90, 0xE9, 0, 0, 0, 0],
            1,
            chooser,
        )?,
        common::branch(
            AIM_PICKED.offset(6),
            &AIM_PICKED_WAS[6..],
            &[0x90, 0xE9, 0, 0, 0, 0],
            1,
            copy,
        )?,
    ]);
    Ok(writes)
}

#[cfg(test)]
mod tests {
    use super::super::testing::{Original, assert_original, no_log, plain, values};
    use super::*;

    /// The sites a build writes, without their bytes.
    fn sites(writes: &[SiteWrite]) -> Vec<(Site, usize)> {
        writes.iter().map(|w| (w.site(), w.size())).collect()
    }

    /// The features whose block acts on a value of 3 as well.
    const ON_AT_THREE: [&str; 6] = [
        "archersdefence",
        "unbreakablebodyparts",
        "changeweightrangebonuses",
        "damageboost",
        "newattackdefence",
        "randomlessaimedhits",
    ];

    #[test]
    fn switches_write_only_at_their_values() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        let keys: std::collections::BTreeSet<_> = ORIGINAL
            .iter()
            .map(|(section, key, ..)| (*section, *key))
            .collect();
        for (section, key) in keys {
            let build = crate::registry::FEATURES
                .iter()
                .find(|f| f.section == section && f.key == key)
                .unwrap()
                .build
                .unwrap();
            for value in [0, 3, -1] {
                if ON_AT_THREE.contains(&key) && value > 0 {
                    continue;
                }
                assert!(build(&ctx, value).unwrap().is_empty(), "{key} = {value}");
            }
        }
    }

    #[test]
    fn the_hook_features_write_the_originals_sites() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        // damageboost: the three parameters, then the hooks per value.
        for (value, want) in [
            (
                1,
                vec![
                    (eix_sites::DATA_799A64, 4),
                    (eix_sites::DATA_799A68, 4),
                    (eix_sites::DATA_799A6C, 4),
                    (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__BB, 8),
                    (eix_sites::C_SPELL_EFFECT_START__92, 7),
                ],
            ),
            (
                3,
                vec![
                    (eix_sites::DATA_799A64, 4),
                    (eix_sites::DATA_799A68, 4),
                    (eix_sites::DATA_799A6C, 4),
                    (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__176, 2),
                    (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__191, 3),
                    (eix_sites::C_UNIT_CLIENT_SPECIFIC_GET_ATTACK_PARAMS__1E8, 2),
                ],
            ),
            (
                4,
                vec![
                    (eix_sites::DATA_799A64, 4),
                    (eix_sites::DATA_799A68, 4),
                    (eix_sites::DATA_799A6C, 4),
                    (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__191, 8),
                    (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__212, 14),
                    (eix_sites::C_UNIT_SERVER_FINISH_CAST__80, 6),
                ],
            ),
            (
                5,
                vec![
                    (eix_sites::DATA_799A64, 4),
                    (eix_sites::DATA_799A68, 4),
                    (eix_sites::DATA_799A6C, 4),
                    (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__191, 8),
                    (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__212, 14),
                    (eix_sites::C_UNIT_SERVER_FINISH_CAST__80, 6),
                    (eix_sites::C_UNIT_SERVER_FINISH_CAST__1E5, 5),
                    (eix_sites::C_SPELL_EFFECT_START__92, 7),
                ],
            ),
        ] {
            assert_eq!(
                sites(&damage_boost(&ctx, value).unwrap()),
                want,
                "damageboost {value}"
            );
        }
        // newattackdefence: values 1 and 2 hook ResolveHit and ApplyHit, 3 and 4 only ApplyHit's `mov ebx, 1`.
        assert_eq!(
            sites(&new_attack_defence(&ctx, 2).unwrap()),
            [
                (eix_sites::DATA_79A7FC, 4),
                (eix_sites::DATA_79A7F8, 4),
                (eix_sites::C_UNIT_SERVER_RESOLVE_HIT__66, 5),
                (eix_sites::C_UNIT_SERVER_RESOLVE_HIT__11D, 6),
                (eix_sites::C_UNIT_SERVER_RESOLVE_HIT__12C, 7),
                (eix_sites::TEXT_5251DA, 11)
            ]
        );
        assert_eq!(
            sites(&new_attack_defence(&ctx, 4).unwrap()),
            [
                (eix_sites::DATA_79A7FC, 4),
                (eix_sites::DATA_79A7F8, 4),
                (eix_sites::TEXT_5251E0, 5),
                (eix_sites::C_UNIT_SERVER_RESOLVE_HIT__11D, 6)
            ]
        );
        assert_eq!(
            sites(&melee_sum_low(&ctx, 1).unwrap()),
            [(eix_sites::C_UNIT_SERVER_GET_ATTACK_PARAMS__F8, 26)]
        );
        assert_eq!(
            sites(&melee_sum_2(&ctx, 1).unwrap()),
            [(eix_sites::C_UNIT_CLIENT_SPECIFIC_GET_ATTACK_PARAMS__151, 12)]
        );
        assert!(melee_sum_1(&ctx, 1).unwrap().is_empty());
        assert_eq!(
            sites(&increasing_hit_chance(&ctx, 1).unwrap()),
            [
                (eix_sites::TEXT_4500EC, 4),
                (eix_sites::OWNED_BONE_LINK_ARRAYS_SET_SIZE__184, 4),
                (eix_sites::C_UNIT_SERVER_RESOLVE_HIT__108, 6),
                (eix_sites::C_UNIT_SERVER_RESOLVE_HIT__115, 6),
                (eix_sites::C_UNIT_SERVER_RESOLVE_HIT__11D, 6)
            ]
        );
    }

    #[test]
    fn the_duel_variable_is_the_units_id_after_und() {
        let mut buf = [0u8; common::ID_NAME_LEN];
        assert_eq!(common::id_variable(DUEL_VARIABLE, 0, &mut buf), c"und0");
        assert_eq!(
            common::id_variable(DUEL_VARIABLE, 1234, &mut buf),
            c"und1234"
        );
        assert_eq!(
            common::id_variable(DUEL_VARIABLE, i32::MIN, &mut buf),
            c"und-2147483648"
        );
    }

    #[test]
    fn a_patched_site_keeps_the_originals_instruction_kinds() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        // damageboost 1: three NOPs then a jump, in place of the store and the jump that followed it.
        let w = plain(&damage_boost(&ctx, 1).unwrap());
        assert_eq!(w[3].2[..4], [0x90, 0x90, 0x90, 0xE9]);
        // Value 5 keeps the call at FinishCast a call.
        let w = plain(&damage_boost(&ctx, 5).unwrap());
        assert_eq!(w[5].2[..2], [0x90, 0xE8]);
        // duels keeps the site a 5-byte jump to its stub.
        let w = plain(&duels(&ctx, 1).unwrap());
        assert_eq!(
            (w.len(), w[0].0, w[0].2[0]),
            (1, eix_sites::C_UNIT_SERVER_ON_DAMAGED__47, 0xE9)
        );
        // increasinghitchance writes the parameters over the padding of two functions.
        let w = plain(&increasing_hit_chance(&ctx, 1).unwrap());
        assert_eq!(
            (w[0].1.as_slice(), w[1].1.as_slice()),
            (&[0x90; 4][..], &[0x90; 4][..])
        );
    }

    #[test]
    fn plain_features_write_the_originals_sites() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        assert_eq!(
            sites(&ranged_weapon_backstabs(&ctx, 1).unwrap()),
            [
                (eix_sites::C_UNIT_IS_BEHIND__E1, 6),
                (eix_sites::C_UNIT_IS_BEHIND__F1, 6),
                (eix_sites::C_UNIT_SERVER_PREPARE_ATTACK__67, 2),
                (eix_sites::C_UNIT_SERVER_PREPARE_ATTACK__5B, 2),
                (eix_sites::C_UNIT_SERVER_EXECUTE_ORDER__39C, 2),
                (eix_sites::C_UNIT_SERVER_EXECUTE_ORDER__3A8, 2),
                (eix_sites::C_UNIT_SERVER_EXECUTE_ORDER__390, 2)
            ]
        );
        assert_eq!(
            plain(&archers_defence(&ctx, 2).unwrap())[1],
            (
                eix_sites::C_UNIT_SERVER_GET_ATTACK_PARAMS__16C,
                vec![0x74, 0x05],
                vec![0xEB, 0x0C]
            )
        );
        assert_eq!(
            sites(&kills_in_multi(&ctx, 1).unwrap()),
            [(eix_sites::C_UNIT_SERVER_TAKE_HIT__227, 6)]
        );
        assert_eq!(
            sites(&only_min_damage(&ctx, 1).unwrap()),
            [
                (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_HIT__10C, 1),
                (eix_sites::C_ITEM_WEAPON_ROLL_DAMAGE__11, 1)
            ]
        );
        assert_eq!(
            sites(&heroes_armors(&ctx, 1).unwrap()),
            [(eix_sites::C_WORLD_SERVER_CREATE_UNITS__6CB, 5)]
        );
        assert_eq!(
            sites(&backstabs_for_all(&ctx, 1).unwrap()),
            [(eix_sites::C_UNIT_IS_BEHIND__6, 19)]
        );
    }

    #[test]
    fn parameters_land_where_the_game_reads_them() {
        let v = values(&[
            ("gameplay", "bodypartbreakmodifier", "-3"),
            ("gameplay", "weight", "25"),
            ("gameplay", "strweight", "10.50"),
            ("gameplay", "hitdistmul", "1.9"),
            ("gameplay", "hitdistadd", "4.9"),
        ]);
        let ctx = Ctx::new(&v, no_log);
        assert_eq!(
            plain(&unbreakable_body_parts(&ctx, 1).unwrap())[1],
            (
                eix_sites::RDATA_73DDC0,
                vec![0x00, 0x00, 0xA0, 0xC0],
                vec![0x00, 0x00, 0x40, 0xC0]
            )
        );
        let w = plain(&weight_range_bonuses(&ctx, 1).unwrap());
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].2, 25.0f32.to_le_bytes());
        // The built-in default is the game's own range.
        assert_eq!(w[1].1, w[1].2);
        assert_eq!(
            sites(&weight_range_bonuses(&ctx, 2).unwrap())[2],
            (eix_sites::C_UNIT_SERVER_ATTACK_ROLL__100, 1)
        );
        let w = plain(&strength_stats(&ctx, 1).unwrap());
        assert_eq!(w[2].2, 10.5f32.to_le_bytes());
        assert_eq!(
            (w[0].1.clone(), w[1].1.clone()),
            (w[0].2.clone(), w[1].2.clone())
        );
        assert_eq!(w.len(), 5);
        // Fractional distance modifiers reach the game constants.
        let w = plain(&run_away_distance(&ctx, 1).unwrap());
        assert_eq!(
            sites(&run_away_distance(&ctx, 1).unwrap()),
            [
                (eix_sites::C_UNIT_SERVER_ACTION_IN_REACH__140, 64),
                (eix_sites::C_UNIT_SERVER_ACTION_IN_REACH__31.back(2), 6),
                (eix_sites::C_UNIT_SERVER_ACTION_IN_REACH__3A.back(2), 6),
                (eix_sites::C_UNIT_SERVER_FINISH_ACTION__1D4, 4),
                (eix_sites::C_UNIT_SERVER_FINISH_ACTION__1D8, 4)
            ]
        );
        assert_eq!(w[3].2, 1.9f32.to_le_bytes());
        assert_eq!(w[4].2, 4.9f32.to_le_bytes());
        assert_eq!(w[3].1, [0x90; 4]);
    }

    /// The original's changed bytes, `??` for a branch displacement into its arena.
    #[rustfmt::skip]
    const ORIGINAL: &[Original] = &[
        ("gameplay", "newrangedweapontypes", 1, &[], &[(eix_sites::C_UNIT_SERVER_HAS_RANGED_WEAPON__1D, "90 90 90 E8 ?? ?? ?? ?? 90 90 90 90 90 85 C0 75"), (eix_sites::C_UNIT_SERVER_ATTACKS_FROM_RANGE__16, "90 90 8B CE E8 A1 FF FF FF 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90"), (eix_sites::C_UNIT_SERVER_ATTACKS_FROM_RANGE__40, "00 74"), (eix_sites::C_UNIT_SERVER_CAN_REACH_TARGET__4D, "90 90 89 D9 E8 2A EE FF FF 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90"), (eix_sites::C_UNIT_SERVER_CAN_REACH_TARGET__77, "01"), (eix_sites::C_UNIT_SERVER_CAN_REACH_AND_PLAN_FROM__45, "90 90 8B CE E8 D2 EC FF FF 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90"), (eix_sites::C_UNIT_SERVER_CAN_REACH_AND_PLAN_FROM__6F, "01"), (eix_sites::C_UNIT_SERVER_EXECUTE_ORDER__37D, "90 90 8B CE E8 9A D9 FF FF 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90"), (eix_sites::C_UNIT_SERVER_EXECUTE_ORDER__3A7, "00"), (eix_sites::C_UNIT_SERVER_ORDER_ATTACK__101, "90 90 89 F9 E8 46 6A FF FF 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90 90"), (eix_sites::C_UNIT_SERVER_ORDER_ATTACK__12B, "01")]),
        ("gameplay", "acidattack", 1, &[], &[(eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__2BE, "90 E9 ?? ?? ?? ??")]),
        ("gameplay", "acidattack", 2, &[], &[(eix_sites::C_SPELL_EFFECT_UPDATE_LASTING__460, "90 E9 ?? ?? ?? ??")]),
        ("gameplay", "nightpower", 2, &[], &[(eix_sites::C_SCENE_RENDERER_DESTRUCT_BASE__B4, "00 00 80 3F 00 00 80 3F"), (eix_sites::C_UNIT_CLIENT_SPECIFIC_GET_ATTACK_PARAMS__1E8, "90 E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_GET_ATTACK_PARAMS__17A, "90 E9 ?? ?? ?? ??")]),
        ("gameplay", "randomlessaimedhits", 5, &[], &[(eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__EF, "90 E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__15D, "E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__23F, "10"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__24A, "90 E9 ?? ?? ?? ?? 90 E9 ?? ?? ?? ??")]),
        ("gameplay", "randomlessaimedhits", 8, &[], &[(eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__EF, "90 E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__15D, "E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__23F, "10"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__24A, "90 E9 ?? ?? ?? ?? 90 E9 ?? ?? ?? ??")]),
        ("gameplay", "randomlessaimedhits", 1, &[], &[(eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__EF, "90 E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__15D, "E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__23F, "10"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__24A, "90 E9 ?? ?? ?? ?? 90 E9 ?? ?? ?? ??")]),
        ("gameplay", "randomlessaimedhits", 3, &[], &[(eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__EF, "90 E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__15D, "E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__23F, "10"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__24A, "90 E9 ?? ?? ?? ?? 90 E9 ?? ?? ?? ??")]),
        ("gameplay", "randomlessaimedhits", 4, &[], &[(eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__EF, "90 E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__15D, "E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__23F, "10"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__24A, "90 E9 ?? ?? ?? ?? 90 E9 ?? ?? ?? ??")]),
        ("gameplay", "randomlessaimedhits", 2, &[], &[(eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__45, "90 E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__EF, "90 E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__15D, "E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__23F, "10"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__24A, "90 E9 ?? ?? ?? ?? 90 E9 ?? ?? ?? ??")]),
        ("gameplay", "randomlessaimedhits", 6, &[], &[(eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__45, "90 E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__EF, "90 E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__15D, "E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__23F, "10"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__24A, "90 E9 ?? ?? ?? ?? 90 E9 ?? ?? ?? ??")]),
        ("gameplay", "randomlessaimedhits", 7, &[], &[(eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__45, "90 E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__EF, "90 E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__15D, "E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__23F, "10"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__24A, "90 E9 ?? ?? ?? ?? 90 E9 ?? ?? ?? ??")]),
        ("gameplay", "randomlessaimedhits", 9, &[], &[(eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__45, "90 E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__EF, "90 E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__15D, "E9 ?? ?? ?? ??"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__23F, "10"), (eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__24A, "90 E9 ?? ?? ?? ?? 90 E9 ?? ?? ?? ??")]),
    ];

    #[test]
    fn arena_features_write_the_originals_bytes() {
        assert_original(ORIGINAL);
    }

    /// Six whole parts of 100 hit points, no armour; a hit of 10 to 20 of the first damage type.
    fn bare() -> (AimTarget, AimAttack, Aim) {
        let slot = AimSlot {
            kind: 2,
            current: 100.0,
            maximum: 100.0,
            share: 1.0,
            scale: 1.0,
            absorb: [[0.0; 7]; 3],
        };
        let mut weights = [0.0; 7];
        weights[0] = 1.0;
        let attack = AimAttack {
            weights,
            min: 10.0,
            max: 20.0,
            absorb_factor: 1.0,
            reach: 1.0,
        };
        let aim = Aim {
            damage: AimDamage::Range,
            fallback: 1,
            check_hero: true,
            head: 2.0,
            hand: 1.0,
            leg: 1.0,
            critical: 3.0,
            health_down_to_ignore: 1.0,
            ranged_distance: 5.0,
        };
        let target = AimTarget {
            slots: [slot; 6],
            resist: [0.0; 7],
        };
        (target, attack, aim)
    }

    #[test]
    fn the_head_wins_at_its_price() {
        // Every part: (10 + 20) / 2 expected; the head's doubled.
        let (target, attack, aim) = bare();
        assert_eq!(aim_choice(&target, &attack, &aim), 0);
    }

    #[test]
    fn armour_moves_the_aim_to_the_last_best_part() {
        // A helmet of 25 leaves the head 25 / 20 * 2; the other parts tie at 15, and the last wins.
        let (mut target, attack, aim) = bare();
        target.slots[0].absorb[2][0] = 25.0;
        assert_eq!(aim_choice(&target, &attack, &aim), 5);
        // The middle layer also takes the companion's resistance, but not in value 1's walk.
        let (mut target, attack, _) = bare();
        target.resist[0] = 25.0;
        assert_eq!(
            aim_residual(&target.slots[0], Some(&target.resist), &attack),
            -5.0
        );
        assert_eq!(aim_residual(&target.slots[0], None, &attack), 20.0);
    }

    #[test]
    fn a_ranged_attacker_prices_the_legs_up() {
        let (target, mut attack, aim) = bare();
        attack.reach = 5.0;
        assert_eq!(aim_choice(&target, &attack, &aim), 5);
    }

    #[test]
    fn a_broken_part_is_not_aimed_at_but_the_body_is() {
        let (mut target, attack, mut aim) = bare();
        aim.health_down_to_ignore = 2.0;
        for (p, slot) in target.slots.iter_mut().enumerate() {
            if p != 3 {
                slot.current = 40.0;
            }
        }
        // Only the body and the second hand keep a score; the later of the tie wins.
        assert_eq!(aim_choice(&target, &attack, &aim), 3);
        target.slots[3].current = 40.0;
        assert_eq!(aim_choice(&target, &attack, &aim), 1);
    }

    /// xorshift32: the generator of the differential cases.
    struct Rng(u32);

    impl Rng {
        fn next(&mut self) -> u32 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.0 = x;
            x
        }

        /// A multiple of 1/8, exact in every float width.
        fn eighths(&mut self, n: u32, plus: u32) -> f32 {
            (self.next() % n + plus) as f32 * 0.125
        }
    }

    /// The differential case of `seed`: a natural attacker without a player and a target, in the generator's order.
    fn case(seed: u32) -> (AimTarget, AimAttack, Aim) {
        let mut r = Rng(seed);
        let mut attack = AimAttack::default();
        for w in &mut attack.weights {
            *w = if r.next().is_multiple_of(3) {
                r.eighths(9, 0)
            } else {
                0.0
            };
        }
        attack.min = r.eighths(200, 0);
        let spread = if r.next().is_multiple_of(4) {
            0.0
        } else {
            r.eighths(160, 0)
        };
        attack.max = attack.min + spread;
        attack.absorb_factor = 1.0;
        attack.reach = r.eighths(80, 0);
        let mut target = AimTarget::default();
        for s in &mut target.slots {
            s.kind = (r.next() % 4) as i32;
            s.maximum = r.eighths(800, 8);
            s.current = r.eighths(800, 0);
            s.share = r.eighths(16, 1);
            s.scale = r.eighths(16, 1);
            for row in &mut s.absorb {
                for a in row {
                    *a = if r.next().is_multiple_of(2) {
                        0.0
                    } else {
                        r.eighths(40, 0)
                    };
                }
            }
        }
        for x in &mut target.resist {
            *x = r.eighths(20, 0);
        }
        let aim = Aim {
            damage: AimDamage::Range,
            fallback: r.next() % 6,
            check_hero: true,
            head: r.eighths(24, 1),
            hand: r.eighths(24, 1),
            leg: r.eighths(24, 1),
            critical: r.eighths(24, 1),
            health_down_to_ignore: r.eighths(32, 1),
            ranged_distance: r.eighths(64, 0),
        };
        (target, attack, aim)
    }

    /// The part the original's value-5 code gives for the cases of seeds 1, 2, ...
    #[rustfmt::skip]
    const ORIGINAL_PICKS: [u8; 300] = [
        1,1,2,3,4,3,3,3,2,5,3,5,2,4,1,4,0,1,2,3,3,4,1,1,2,5,0,1,4,5,0,5,1,0,4,5,5,2,5,0,3,4,3,4,2,1,3,2,1,4,4,0,3,2,2,1,
        2,1,1,4,3,5,5,1,4,0,2,1,4,1,0,5,5,3,5,5,3,5,3,2,3,2,3,1,2,2,4,5,2,1,5,2,3,4,5,1,2,3,0,0,0,5,5,1,0,4,3,2,2,5,4,5,
        5,5,1,5,3,1,1,4,5,2,0,1,2,2,5,1,4,4,0,4,0,5,1,5,1,4,1,2,2,0,4,2,4,5,5,2,2,2,1,2,4,1,0,1,0,4,0,0,3,4,2,2,1,2,1,3,
        0,0,0,5,3,5,2,1,1,0,4,2,0,3,2,3,1,3,2,2,4,1,5,1,0,4,5,5,4,5,4,1,0,5,1,1,4,3,0,4,3,5,4,0,4,0,1,4,1,0,0,4,3,1,1,4,
        4,1,3,1,4,4,1,4,0,3,1,0,1,2,5,0,4,3,0,4,5,4,0,3,2,5,5,5,0,4,2,4,3,0,4,5,5,5,4,5,5,0,5,1,3,4,1,1,0,3,3,1,0,5,0,4,
        4,0,5,3,0,5,4,0,2,1,3,4,2,1,3,1,3,3,0,0,
    ];

    #[test]
    fn the_choice_matches_the_originals_runs() {
        let mut scored = 0;
        for (i, &part) in ORIGINAL_PICKS.iter().enumerate() {
            let seed = i as u32 + 1;
            let (target, attack, mut aim) = case(seed);
            assert_eq!(
                aim_choice(&target, &attack, &aim),
                u32::from(part),
                "seed {seed}"
            );
            // A case the scores decide, not `bodypart`.
            aim.fallback = 9;
            if aim_choice(&target, &attack, &aim) != 9 {
                scored += 1;
            }
        }
        assert!(scored > 200, "only {scored} cases decided by the scores");
    }

    /// The value-1 case of `seed`: [`case`]'s, with its largest amount as the last hit's and the layers' factor from a
    /// second generator.
    fn last_hit_case(seed: u32) -> (AimTarget, AimAttack, Aim) {
        let (target, mut attack, mut aim) = case(seed);
        attack.min = attack.max;
        attack.absorb_factor = Rng(seed ^ 0x9E37_79B9).eighths(16, 1);
        aim.damage = AimDamage::LastHit;
        (target, attack, aim)
    }

    /// The part the original's value-1 code gives for [`last_hit_case`] of seeds 1, 2, ...
    #[rustfmt::skip]
    const ORIGINAL_LAST_HIT_PICKS: [u8; 300] = [
        1,5,2,3,1,3,3,3,2,5,3,5,0,0,1,4,0,1,5,3,3,5,5,1,0,3,0,1,4,5,0,5,4,1,4,5,5,2,5,0,3,4,0,4,2,1,0,2,1,4,0,0,3,5,2,1,
        2,1,1,4,3,5,5,1,4,0,2,1,2,0,0,5,5,3,5,5,0,1,5,5,3,2,3,1,2,2,4,1,2,1,5,2,3,4,3,1,2,3,0,5,0,5,5,1,0,2,3,2,1,5,4,3,
        5,4,1,5,3,1,1,4,5,2,4,0,2,2,5,1,4,4,0,4,5,5,1,3,1,4,1,2,2,5,4,2,4,5,5,2,2,2,2,2,4,1,0,1,5,4,4,0,1,4,2,2,1,2,5,3,
        0,0,0,5,3,5,2,1,1,0,4,2,0,3,2,3,5,4,2,2,5,1,5,5,0,4,5,1,0,5,4,3,0,5,1,4,4,3,0,4,1,2,4,0,4,0,1,4,1,0,0,4,3,1,5,4,
        4,1,3,1,4,4,1,3,0,3,1,0,1,2,5,0,4,3,1,4,5,4,0,3,2,5,5,5,0,4,2,0,3,0,2,0,5,5,4,5,1,0,5,1,3,4,5,1,0,3,5,1,4,5,5,4,
        3,0,5,3,0,5,4,0,2,1,3,4,2,1,3,3,0,2,0,0,
    ];

    #[test]
    fn the_last_hit_choice_matches_the_originals_runs() {
        let mut scored = 0;
        for (i, &part) in ORIGINAL_LAST_HIT_PICKS.iter().enumerate() {
            let seed = i as u32 + 1;
            let (target, attack, mut aim) = last_hit_case(seed);
            assert_eq!(
                aim_choice(&target, &attack, &aim),
                u32::from(part),
                "seed {seed}"
            );
            aim.fallback = 9;
            if aim_choice(&target, &attack, &aim) != 9 {
                scored += 1;
            }
        }
        assert!(scored > 200, "only {scored} cases decided by the scores");
    }

    #[test]
    fn no_damage_aims_at_bodypart() {
        let (target, mut attack, aim) = bare();
        attack.weights = [0.0; 7];
        assert_eq!(aim_choice(&target, &attack, &aim), aim.fallback);
    }

    #[test]
    fn the_start_hook_goes_with_two_six_seven_and_nine() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        for value in 1..=9 {
            let has_start = sites(&randomless_aimed_hits(&ctx, value).unwrap())
                .contains(&(eix_sites::C_UNIT_SERVER_BUILD_ATTACK_STEPS__45, 6));
            assert_eq!(has_start, matches!(value, 2 | 6 | 7 | 9), "{value}");
        }
        for value in [0, 10, 11] {
            assert!(randomless_aimed_hits(&ctx, value).unwrap().is_empty());
        }
    }

    #[test]
    fn ranged_types_are_bows_crossbows_and_two_more() {
        for (ty, ranged) in [
            (5, 1),
            (6, 1),
            (11, 1),
            (15, 1),
            (0, 0),
            (7, 0),
            (8, 0),
            (16, 0),
        ] {
            assert_eq!(is_ranged(ty), ranged, "type {ty}");
        }
    }
}
