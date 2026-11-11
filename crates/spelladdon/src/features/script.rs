//! Script features.
//!
//! A command's or function's new argument comes from its keyword's signature (`ScriptKeyword`): the original points
//! the entry at another of the game's signature strings, or at one it writes into padding, and the parser then
//! accepts the argument. Where the original jumps to added code, the site becomes a hook whose handler does that
//! code's work on the command (`ScriptCommand`: the float arguments' block at +0x8, the object arguments' at +0x30).
//! A feature of the file without a build here is logged as not implemented when it is on.
#![expect(
    unsafe_code,
    reason = "the handlers read script commands, units and strings of the hooked game code and call its functions"
)]

use core::ffi::CStr;
use core::mem::offset_of;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use eix_engine::game::flow::{DIFFICULTY, UPDATE_PERIOD};
use eix_engine::game::mfc::{self, GameString};
use eix_engine::game::object::{CLASS_UNIT, EngineObject};
use eix_engine::game::script::{ScriptCommand, ScriptObjectRef};
use eix_engine::game::unit::{Unit, UnitServer, UnitStats};
use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, FpuContext, Handler, Mode};
use eix_engine::patch;
use eix_engine::x87::Ext;
use eix_sites::Site;

use super::common::{self, Bytes, Obj, read};
use super::{Build, BuildError, Ctx};

/// The implemented features: `(section, key, build)`.
pub(crate) const BUILDS: &[(&str, &str, Build)] = &[
    ("speed", "gsgetvar", gs_get_var),
    ("speed", "gssetvar", gs_set_var),
    ("saves", "savecontrol", save_control),
    ("gameplay", "autorunmobfile", auto_run_mob_file),
    ("gameplay", "infiniteaddmobspawn", infinite_add_mob_spawn),
    ("gameplay", "waslootedtrans", was_looted_trans),
    ("gameplay", "castscripttrans", cast_script_trans),
    ("gameplay", "hptrans", hp_trans),
    ("gameplay", "mptrans", mp_trans),
    ("gameplay", "giveunitspelltrans", give_unit_spell_trans),
    ("gameplay", "notifydeaths", notify_deaths),
    ("gameplay", "castspellunittrans", cast_spell_unit_trans),
    ("gameplay", "castspellpointtrans", cast_spell_point_trans),
    ("gameplay", "removefrozenvar", remove_frozen_var),
    ("gameplay", "freemultiplayervars", free_multiplayer_vars),
    ("gameplay", "genocideincvars", genocide_inc_vars),
    ("gameplay", "genocidedipvars", genocide_dip_vars),
    ("gameplay", "movetoobjecttrans", move_to_object_trans),
    ("gameplay", "groupseetrans", group_see_trans),
    ("gameplay", "unitseetrans", unit_see_trans),
    ("gameplay", "notifylootings", notify_lootings),
    ("gameplay", "runtrans", run_trans),
    ("gameplay", "crawltrans", crawl_trans),
    ("gameplay", "getaiclasstrans", get_ai_class_trans),
    ("gameplay", "getplayertrans", get_player_trans),
    ("gameplay", "setplayertrans", set_player_trans),
    ("gameplay", "standtrans", stand_trans),
    ("tech", "drawunitsids", draw_units_ids),
    ("tech", "scriptchecking", script_checking),
    ("gameplay", "playmusictrans", play_music_trans),
    ("gameplay", "legendaryspellsystem", legendary_spell_system),
    ("gameplay", "killunittrans", kill_unit_trans),
    ("gameplay", "fixworldtimetrans", fix_world_time_trans),
    (
        "gameplay",
        "setspellaggressiontrans",
        set_spell_aggression_trans,
    ),
];

fn bytes(site: Site, expected: &[u8], new: &[u8]) -> Result<SiteWrite, BuildError> {
    Ok(SiteWrite::bytes(site, expected, new)?)
}

// --- Script arguments ------------------------------------------------------------------------------------------------

/// The keyword entries whose signatures the features change, and the field each changes (`ScriptKeyword`: +0x8 a
/// function's, +0xC a command's).
mod keyword {
    use eix_sites::Site;

    pub(super) const FUNCTION: usize = 0x8;
    pub(super) const COMMAND: usize = 0xC;
    pub(super) const CAST: Site = eix_sites::DATA_78BE20;
    pub(super) const CRAWL: Site = eix_sites::DATA_78BEA0;
    pub(super) const FIX_WORLD_TIME: Site = eix_sites::DATA_78BFB0;
    pub(super) const KILL_UNIT: Site = eix_sites::DATA_78C0F0;
    pub(super) const MOVE_TO_OBJECT: Site = eix_sites::DATA_78C150;
    pub(super) const RUN: Site = eix_sites::DATA_78C320;
    pub(super) const HP: Site = eix_sites::DATA_78C7B0;
    pub(super) const MANA: Site = eix_sites::DATA_78C7D0;
    pub(super) const UNIT_SEE: Site = eix_sites::DATA_78C8F0;
    pub(super) const GET_PLAYER: Site = eix_sites::DATA_78C9B0;
    pub(super) const SET_PLAYER: Site = eix_sites::DATA_78C410;
    pub(super) const GIVE_QUEST_ITEM: Site = eix_sites::DATA_78C030;
    pub(super) const MAX_HP: Site = eix_sites::DATA_78C7C0;
    pub(super) const MAX_MANA: Site = eix_sites::DATA_78C7E0;
    pub(super) const GROUP_SEE: Site = eix_sites::DATA_78C900;
    pub(super) const WAS_LOOTED: Site = eix_sites::DATA_78CA70;
}

/// The game's signature strings the features point keywords at: `"f"`, `"o"`, `"of"`, `"ff"`, `"Fo"`, `"Fof"`,
/// `"off"`, `"offf"`, `"oo"`, `"Gg"`.
mod signature {
    use eix_sites::Site;

    pub(super) const F: Site = eix_sites::DATA_78BBD8;
    pub(super) const FS: Site = eix_sites::DATA_78D4F4;
    pub(super) const O: Site = eix_sites::DATA_78BBD0;
    pub(super) const OF: Site = eix_sites::DATA_78D6C4;
    pub(super) const FF: Site = eix_sites::DATA_78D614;
    pub(super) const FN_O: Site = eix_sites::DATA_78CD44;
    pub(super) const FN_OF: Site = eix_sites::DATA_78CCB8;
    pub(super) const OFF: Site = eix_sites::DATA_78D5E0;
    pub(super) const OFFF: Site = eix_sites::DATA_78D34C;
    pub(super) const OO: Site = eix_sites::DATA_78D5F8;
    pub(super) const FN_GG: Site = eix_sites::DATA_78CC28;
    pub(super) const FN_GO: Site = eix_sites::DATA_78CC38;
}

/// The keyword at `entry` takes the signature at `now` instead of `was` in its `field`.
fn resign(entry: Site, field: usize, was: Site, now: Site) -> SiteWrite {
    SiteWrite::pointer(entry.offset(field), was.ru(), now.ru())
}

/// The `ScriptCommand` at `at`, which a hooked command or function runs on.
///
/// # Safety
/// `at` must be the game's live command, on the game's thread.
unsafe fn command(at: u32) -> Obj {
    // SAFETY: the caller vouches for a live command of `size_of::<ScriptCommand>()` bytes.
    unsafe { Obj::new(at, size_of::<ScriptCommand>() as u32) }
}

/// Float argument `i` of `cmd`, when the command has it.
fn float_arg(cmd: Obj, i: usize) -> Option<f32> {
    let floats = offset_of!(ScriptCommand, float_args) as u32;
    let data: u32 = cmd.get(floats + 4);
    let len: i32 = cmd.get(floats + 8);
    if data == 0 || i >= usize::try_from(len).unwrap_or(0) {
        return None;
    }
    // SAFETY: a live command's float block holds `len` floats.
    Some(unsafe { read::<f32>(data + 4 * i as u32) })
}

/// The object of object argument `i` of `cmd`, of any class, or 0.
fn object_arg(cmd: Obj, i: usize) -> u32 {
    let objects = offset_of!(ScriptCommand, object_args) as u32;
    let data: u32 = cmd.get(objects + 4);
    let len: i32 = cmd.get(objects + 8);
    if data == 0 || i >= usize::try_from(len).unwrap_or(0) {
        return 0;
    }
    // SAFETY: a live command's object block holds `len` pointers to its argument references.
    let reference = unsafe { read::<u32>(data + 4 * i as u32) };
    if reference == 0 {
        return 0;
    }
    // SAFETY: an argument reference's one word is its object or null.
    unsafe { read::<u32>(reference + offset_of!(ScriptObjectRef, object) as u32) }
}

/// The object of object argument `i` of `cmd` when it is a unit (`AsUnit` 0x4B2530), else 0.
fn unit_arg(cmd: Obj, i: usize) -> u32 {
    let object = object_arg(cmd, i);
    if object == 0 {
        return 0;
    }
    let ptr = core::ptr::with_exposed_provenance_mut::<EngineObject>(object as usize);
    // SAFETY: a live engine object the reference holds, on the game's thread.
    if unsafe { EngineObject::class_id(ptr) } == CLASS_UNIT {
        object
    } else {
        0
    }
}

/// `x` rounded to an integer as the original's `fistp` rounds it, under the game's control word.
fn whole(x: f32) -> i32 {
    Ext::new(x.into()).fistp()
}

/// The id of the engine object at `object`.
///
/// # Safety
/// `object` must be a live engine object.
unsafe fn object_id(object: u32) -> i32 {
    // SAFETY: the caller vouches for the object; +0xC is its id.
    unsafe { read::<i32>(object + offset_of!(EngineObject, id) as u32) }
}

/// The id of the unit of the server controller at `server` (its +0x1C).
///
/// # Safety
/// `server` must be a live `CUnitServer`.
unsafe fn controlled_unit_id(server: u32) -> i32 {
    // SAFETY: the caller vouches for the controller.
    let unit = unsafe { read::<u32>(server + offset_of!(UnitServer, unit) as u32) };
    // SAFETY: a live controller's unit.
    unsafe { object_id(unit) }
}

/// The first `N` bytes of the C string at `at`, or `None` when it is shorter or `at` is null.
///
/// # Safety
/// `at` must be null or a NUL-terminated string.
unsafe fn prefix<const N: usize>(at: u32, skip: usize) -> Option<[u8; N]> {
    if at == 0 {
        return None;
    }
    // SAFETY: the caller vouches for the string.
    let text = unsafe { CStr::from_ptr(core::ptr::with_exposed_provenance(at as usize)) };
    text.to_bytes().get(skip..skip + N)?.try_into().ok()
}

// --- Variables ---------------------------------------------------------------------------------------------------------

/// The names `GSGetVar` and `GSSetVar` answer with the game's own values: `"Diff"` as characters 0-3 (the difficulty)
/// and `"peed"` as characters 4-7 (the game speed).
fn is_difficulty(name: u32) -> bool {
    // SAFETY: the callers pass a variable name's characters.
    let head = unsafe { prefix::<4>(name, 0) };
    head == Some(*b"Diff")
}

fn is_speed(name: u32) -> bool {
    // SAFETY: as in `is_difficulty`.
    let tail = unsafe { prefix::<4>(name, 4) };
    tail == Some(*b"peed")
}

/// The node `NamedFloatTable::Get` returns from for a game value: its value at +8, as a variable's.
static GAME_VALUE: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];

/// `NamedFloatTable::Get` 0x460B68, before its `~CString` of the name (`[esp + 0x18]`): for the difficulty's or the
/// speed's name the lookup returns that value, whether the table has the variable or not. The name is read before the
/// copied call frees it.
extern "C" fn get_game_value(_: usize, ctx: &mut Context) {
    let Some(name) = ctx.stack_word(6) else {
        return;
    };
    let global = if is_difficulty(name) {
        DIFFICULTY
    } else if is_speed(name) {
        UPDATE_PERIOD
    } else {
        return;
    };
    // SAFETY: a game.exe global, on the game's thread.
    let value = unsafe { global.read() };
    GAME_VALUE[2].store((value as f32).to_bits(), Ordering::Relaxed);
    ctx.esi = GAME_VALUE.as_ptr() as u32;
    ctx.resume = eix_sites::NAMED_FLOAT_TABLE_GET__81;
}

/// `gsgetvar` = 1: `GSGetVar` answers the difficulty and the game speed.
fn gs_get_var(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![hook::context_hook(
        eix_sites::NAMED_FLOAT_TABLE_GET__78,
        &[0xE8, 0x03, 0xFE, 0x29, 0x00],
        Mode::After,
        get_game_value,
        0,
    )?])
}

/// The `GSSetVar` case at 0x4B8312, after the copied `mov ecx, [edx]; push ecx; mov ecx, [eax + 0x20]`: the name is
/// stack word 0 and the value word 1. A speed's name also sets the game speed; the variable is set as for any name.
extern "C" fn set_game_speed(_: usize, ctx: &mut Context) {
    let (Some(name), Some(value)) = (ctx.stack_word(0), ctx.stack_word(1)) else {
        return;
    };
    if is_speed(name) {
        // SAFETY: the game's global, on its thread.
        unsafe { UPDATE_PERIOD.write(whole(f32::from_bits(value))) };
    }
}

/// `gssetvar` = 1: `GSSetVar` sets the game speed.
fn gs_set_var(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![common::branch_before(
        eix_sites::SCRIPT_RUN_EXECUTE__1BB2,
        &[0x8B, 0x0A, 0x51, 0x8B, 0x48, 0x20],
        &[0x90, 0xE9, 0, 0, 0, 0],
        1,
        set_game_speed,
        0,
    )?])
}

/// Whether saving is forbidden: player 0's variable "Save" is not 0.
fn save_forbidden() -> bool {
    common::get_variable(c"Save") != 0.0
}

/// The escape menu's save entry (`mov ecx, 0x7AFE20` before its multiplayer test): while saving is forbidden the
/// hover or the click goes to `arg`, past the entry's action.
extern "C" fn menu_save(arg: usize, ctx: &mut Context) {
    if save_forbidden() {
        ctx.resume = Site::new(arg);
    } else {
        ctx.ecx = eix_sites::DATA_7AFE20.here32();
    }
}

/// The quick-save key of `BattleScreenKeyDown` (0x63B157, `xor dl, dl; mov ecx, 0x7933AC` before the save call):
/// while saving is forbidden the key goes to the handler's exit. Player 0's variable is read, where the original's
/// added code reads a player numbered by EDX and leaves its stack unbalanced.
extern "C" fn key_save(_: usize, ctx: &mut Context) {
    if save_forbidden() {
        ctx.resume = eix_sites::BATTLE_SCREEN_KEY_DOWN__F64;
        return;
    }
    ctx.edx &= !0xFF;
    ctx.ecx = eix_sites::DATA_7933AC.here32();
    ctx.resume = eix_sites::BATTLE_SCREEN_KEY_DOWN__BFE;
}

/// `savecontrol` 1 or 2: player 0's variable "Save" forbids saving from the escape menu and by key.
fn save_control(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if !(1..=2).contains(&value) {
        return Ok(Vec::new());
    }
    const MENU_ECX: [u8; 5] = [0xB9, 0x20, 0xFE, 0x7A, 0x00];
    let mut writes = common::call_at(
        eix_sites::TEXT_62AD51,
        &MENU_ECX,
        0,
        menu_save,
        eix_sites::TEXT_62AD77.ru(),
    )?;
    writes.extend(common::call_at(
        eix_sites::TEXT_62AEED,
        &MENU_ECX,
        0,
        menu_save,
        eix_sites::TEXT_62B11F.ru(),
    )?);
    writes.extend(common::jump_at(
        eix_sites::BATTLE_SCREEN_KEY_DOWN__BF7,
        &[0x32, 0xD2, 0xB9, 0xAC, 0x33, 0x79, 0x00],
        2,
        key_save,
        0,
    )?);
    Ok(writes)
}

/// An id at or above it is a player's hero, below it any other unit, for every feature here (the original's death
/// and genocide tests take ids strictly above it, so id 1000000000 was a hero only for loot).
const HERO_IDS: u32 = 1_000_000_000;

/// `CUnitServer::OnDamaged` 0x553B85 (`lea ecx, [esi + 0xA74]`) as the victim dies: player 0's variable named after
/// the victim's id becomes 1, or with `notifydeaths` = 2 (`arg`) 2 when the killer (EDI) is a hero.
extern "C" fn notify_death(arg: usize, ctx: &mut Context) {
    ctx.ecx = ctx.esi.wrapping_add(0xA74);
    // SAFETY: ESI is the victim's live server controller.
    let victim = unsafe { controlled_unit_id(ctx.esi) };
    // SAFETY: EDI is the killing unit, live.
    let killer = unsafe { object_id(ctx.edi) };
    let value = if arg == 2 && killer as u32 >= HERO_IDS {
        2.0
    } else {
        1.0
    };
    let mut name = [0u8; common::ID_NAME_LEN];
    common::set_variable(common::id_variable(b"", victim, &mut name), value);
}

/// `notifydeaths` 1 or 2: a unit's death sets the variable named after its id.
fn notify_deaths(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if !(1..=2).contains(&value) {
        return Ok(Vec::new());
    }
    Ok(vec![hook::context_call(
        eix_sites::C_UNIT_SERVER_ON_DAMAGED__C5,
        &[0x8D, 0x8E, 0x74, 0x0A, 0x00, 0x00],
        Mode::Drop,
        notify_death,
        value as usize,
    )?])
}

/// `CUnitServer::UseSkill` 0x54F07C (`mov ecx, 0x7AFF60`) when a unit loots the body EDI. With `arg`:
/// 1, the variable named after the body's id grows by 20 for a hero looter, 10 for another;
/// 2, a hero looter's id goes into the body's variable;
/// 3, the body's id goes into a hero looter's variable.
extern "C" fn notify_looting(arg: usize, ctx: &mut Context) {
    ctx.ecx = eix_sites::DATA_7AFF60.here32();
    // SAFETY: EDI is the looted unit, live.
    let body = unsafe { object_id(ctx.edi) };
    // SAFETY: ESI is the looter's live server controller.
    let looter = unsafe { controlled_unit_id(ctx.esi) };
    let mut name = [0u8; common::ID_NAME_LEN];
    match arg {
        1 => {
            let name = common::id_variable(b"", body, &mut name);
            let step = if looter as u32 >= HERO_IDS {
                20.0
            } else {
                10.0
            };
            common::set_variable(name, common::get_variable(name) + step);
        }
        2 if looter >= HERO_IDS as i32 => {
            common::set_variable(common::id_variable(b"", body, &mut name), looter as f32);
        }
        3 if looter >= HERO_IDS as i32 => {
            common::set_variable(common::id_variable(b"", looter, &mut name), body as f32);
        }
        _ => {}
    }
}

/// `notifylootings` 1-3: looting a body sets a variable named after an id ([`notify_looting`]).
fn notify_lootings(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if !(1..=3).contains(&value) {
        return Ok(Vec::new());
    }
    Ok(vec![hook::context_call(
        eix_sites::C_UNIT_SERVER_USE_SKILL__7C,
        &[0xB9, 0x60, 0xFF, 0x7A, 0x00],
        Mode::Drop,
        notify_looting,
        value as usize,
    )?])
}

/// `removefrozenvar` = 1: `EnterZone` treats zone 6 as not unique, whatever its frozen state (`jne` -> `jmp`).
fn remove_frozen_var(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![bytes(eix_sites::ENTER_ZONE__18E, &[0x75], &[0xEB])?])
}

/// `freemultiplayervars` = 1: `ClassTemplatesBuild` and `Journal::Refresh` skip the multiplayer variables.
fn free_multiplayer_vars(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    // `je rel32` -> `nop; jmp rel32`, the same target.
    common::fixed(&[
        (
            eix_sites::CLASS_TEMPLATES_BUILD__37,
            &[0x0F, 0x84],
            &[0x90, 0xE9],
        ),
        (
            eix_sites::GAME_FLOW_UPDATE_FROM_SERVER__113,
            &[0x77],
            &[0xEB],
        ),
    ])
}

/// What a genocide hook records, by feature and value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Genocide {
    /// `genocideincvars` 1, 3: "Hits" grows by 1 when a hero hits a non-hero.
    Count,
    /// `genocidedipvars` 1-3: the variable becomes the victim's side when a hero hits or kills a non-hero.
    HeroSide,
    /// `genocidedipvars` 4 at `TakeHit`: "Hits" becomes the victim's side when it has no owner.
    OwnerlessSide,
    /// `genocidedipvars` 4 at `OnDamaged`, 5 at both: the side when an owned unit hits or kills an ownerless one.
    OwnedOnOwnerless,
}

/// Whether `rule` records `victim` hit or killed by `attacker` (both live map objects; the victim a unit).
///
/// # Safety
/// Both must be live objects of at least 0x250 bytes, as the units the hooks pass are.
unsafe fn genocide_records(rule: Genocide, attacker: u32, victim: u32) -> bool {
    // SAFETY: the caller vouches for the attacker.
    let attacker_id = unsafe { object_id(attacker) } as u32;
    // SAFETY: the caller vouches for the victim.
    let victim_id = unsafe { object_id(victim) } as u32;
    // SAFETY: as above; +0x24C is a map object's owner.
    let owner = |o: u32| unsafe { read::<u32>(o + 0x24C) };
    match rule {
        Genocide::Count | Genocide::HeroSide => attacker_id >= HERO_IDS && victim_id < HERO_IDS,
        Genocide::OwnerlessSide => owner(victim) == 0,
        Genocide::OwnedOnOwnerless => owner(attacker) != 0 && owner(victim) == 0,
    }
}

/// Records a hit or a kill in player 0's variable `name` as `rule` says.
///
/// # Safety
/// As [`genocide_records`].
unsafe fn genocide_record(rule: Genocide, name: &CStr, attacker: u32, victim: u32) -> bool {
    // SAFETY: the caller vouches for both objects.
    if !unsafe { genocide_records(rule, attacker, victim) } {
        return false;
    }
    let value = if rule == Genocide::Count {
        common::get_variable(name) + 1.0
    } else {
        // SAFETY: the victim is a unit, whose +0x250 is its side.
        unsafe { read::<i32>(victim + 0x250) as f32 }
    };
    common::set_variable(name, value);
    true
}

/// `CUnitServer::TakeHit` (0x553FBB, `mov ecx, [esi + 0x1c]; mov eax, [edi + 0xc]`, ESI the victim's controller,
/// EDI the attacker): records the hit in "Hits" by the rule `arg`. ECX becomes the victim when it records and the
/// game flow otherwise (a debug log line's only input), as in the original. `genocideincvars` reads the two words
/// TakeHit saved for its caller instead of the attacker and victim; ours tests the attacker and victim.
extern "C" fn genocide_hit(arg: usize, ctx: &mut Context) {
    // SAFETY: `arg` is the rule the build kept.
    let rule = *unsafe { common::kept::<Genocide>(arg) };
    // SAFETY: ESI is the victim's `CUnitServer`, whose +0x1C is its unit.
    let victim = unsafe { read::<u32>(ctx.esi + 0x1C) };
    // SAFETY: EDI is the attacker, a live map object (TakeHit checked it for null).
    ctx.eax = unsafe { object_id(ctx.edi) } as u32;
    // SAFETY: both are live map objects.
    let recorded = unsafe { genocide_record(rule, c"Hits", ctx.edi, victim) };
    ctx.ecx = if recorded { victim } else { GAME_FLOW.here32() };
}

/// `CUnitServer::OnDamaged`'s death path (0x553B7E, `mov ebp, [edi + 0x240]`, EDI the killer, EAX the victim):
/// records the kill in "Kils" by the rule `arg`, then does the replaced load.
extern "C" fn genocide_kill(arg: usize, ctx: &mut Context) {
    // SAFETY: `arg` is the rule the build kept.
    let rule = *unsafe { common::kept::<Genocide>(arg) };
    // SAFETY: EDI and EAX are the killer and victim units the death path holds.
    unsafe { genocide_record(rule, c"Kils", ctx.edi, ctx.eax) };
    // SAFETY: EDI is a unit, whose +0x240 is its controller.
    ctx.ebp = unsafe { read::<u32>(ctx.edi + 0x240) };
}

/// The game flow object, which TakeHit's debug line reads when the original's genocide code skips.
const GAME_FLOW: Site = eix_sites::DATA_7AFE20;

/// `TakeHit`'s and `OnDamaged`'s sites, which the genocide hooks are called from.
const TAKE_HIT: (Site, &[u8]) = (
    eix_sites::C_UNIT_SERVER_TAKE_HIT__7B,
    &[0x8B, 0x4E, 0x1C, 0x8B, 0x47, 0x0C],
);
const ON_DAMAGED: (Site, &[u8]) = (
    eix_sites::C_UNIT_SERVER_ON_DAMAGED__BE,
    &[0x8B, 0xAF, 0x40, 0x02, 0x00, 0x00],
);

fn genocide_hook(
    at: (Site, &[u8]),
    handler: Handler,
    rule: Genocide,
) -> Result<Vec<SiteWrite>, BuildError> {
    Ok(common::call_at(at.0, at.1, 0, handler, common::keep(rule))?)
}

/// `genocideincvars` 1 or 3: "Hits" counts a hero's hits on non-heroes (2 is skipped: see the registry).
fn genocide_inc_vars(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    match value {
        1 | 3 => genocide_hook(TAKE_HIT, genocide_hit, Genocide::Count),
        _ => Ok(Vec::new()),
    }
}

/// `genocidedipvars` 1-5: "Hits" and "Kils" hold the side of the last unit hit or killed.
fn genocide_dip_vars(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    let (hit, kill) = match value {
        1 => (Some(Genocide::HeroSide), None),
        2 => (None, Some(Genocide::HeroSide)),
        3 => (Some(Genocide::HeroSide), Some(Genocide::HeroSide)),
        4 => (
            Some(Genocide::OwnerlessSide),
            Some(Genocide::OwnedOnOwnerless),
        ),
        5 => (
            Some(Genocide::OwnedOnOwnerless),
            Some(Genocide::OwnedOnOwnerless),
        ),
        _ => (None, None),
    };
    let mut writes = Vec::new();
    if let Some(rule) = kill {
        writes.extend(genocide_hook(ON_DAMAGED, genocide_kill, rule)?);
    }
    if let Some(rule) = hit {
        writes.extend(genocide_hook(TAKE_HIT, genocide_hit, rule)?);
    }
    Ok(writes)
}

// --- Mob files ---------------------------------------------------------------------------------------------------------

/// The line the zone's start script gets.
const AUTO_RUN_MOB: &CStr = c"AddMob(\"AutoRunSpellAddon123.mob\")";

/// `CWorldServer::CreateUnits` 0x58FA2C, before its `push 0x78D71C` (")"): the start script being built (the
/// string at the site's `[esp + 0x20]`) gets [`AUTO_RUN_MOB`].
extern "C" fn add_auto_run_mob(_: usize, ctx: &mut Context) {
    let script = core::ptr::with_exposed_provenance_mut::<GameString>(ctx.stack() + 0x20);
    // SAFETY: the function's constructed start-script string, on the game's thread.
    unsafe { mfc::append(script, AUTO_RUN_MOB) };
}

/// The zone's start script loads `AutoRunSpellAddon123.mob` from Maps: `autorunmobfile`'s change, which `newspells`
/// 1 and 2 write as well (the later block's write wins).
pub(crate) fn auto_run_mob() -> Result<SiteWrite, BuildError> {
    Ok(hook::context_hook(
        eix_sites::C_WORLD_SERVER_CREATE_UNITS__B0C,
        &[0x68, 0x1C, 0xD7, 0x78, 0x00],
        Mode::After,
        add_auto_run_mob,
        0,
    )?)
}

/// `autorunmobfile` >= 1: [`auto_run_mob`].
fn auto_run_mob_file(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    Ok(vec![auto_run_mob()?])
}

/// `CWorldServer::LoadMobFile` 0x595ABB (`mov eax, [esi + 0x12C]`, the loaded files' count): a file whose name (EBP)
/// does not start with "Auto" skips the "already loaded" check and loads again; an "Auto" file loads once.
extern "C" fn add_mob_again(_: usize, ctx: &mut Context) {
    // SAFETY: EBP is the mob file's name, the function's argument.
    if unsafe { prefix::<4>(ctx.ebp, 0) } != Some(*b"Auto") {
        ctx.resume = eix_sites::C_WORLD_SERVER_LOAD_MOB_FILE__37;
        return;
    }
    // SAFETY: ESI is the server world, whose loaded-file count is at +0x12C.
    ctx.eax = unsafe { Obj::new(ctx.esi, 0x130) }.get(0x12C);
    ctx.resume = eix_sites::C_WORLD_SERVER_LOAD_MOB_FILE__11;
}

/// `infiniteaddmobspawn` >= 1: `AddMob` loads a file any number of times ([`add_mob_again`]).
fn infinite_add_mob_spawn(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    Ok(common::jump_at(
        eix_sites::C_WORLD_SERVER_LOAD_MOB_FILE__B,
        &[0x8B, 0x86, 0x2C, 0x01, 0x00, 0x00],
        1,
        add_mob_again,
        0,
    )?)
}

// --- Functions -----------------------------------------------------------------------------------------------------------

/// `WasLooted`'s `shr edx, 0x14; and edx, 1` (0x4B467F): the unit's flag word shifted by its second argument.
extern "C" fn was_looted_bit(_: usize, ctx: &mut Context) {
    // SAFETY: ESI is the function call `SSCCmd::CalcFloat` evaluates.
    let bit = float_arg(unsafe { command(ctx.esi) }, 0).map_or(0, whole);
    ctx.edx = (ctx.edx >> (bit as u32 & 31)) & 1;
}

/// `waslootedtrans`: 1 `WasLooted` returns the unit's whole flag word; 2 the bit its second argument names.
fn was_looted_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    const SITE: Site = eix_sites::SSC_CMD_CALC_FLOAT__49F;
    const SHIFT: [u8; 6] = [0xC1, 0xEA, 0x14, 0x83, 0xE2, 0x01];
    match value {
        1 => Ok(vec![SiteWrite::nop(SITE, &SHIFT)]),
        2 => {
            // The original pushes ECX before its jump and pops it in the added code; the stub pops it first.
            let stub =
                common::jump_stub(SITE, &[0x59], was_looted_bit, 0, SITE.offset(SHIFT.len()))?;
            Ok(vec![
                common::branch(SITE, &SHIFT, &[0x51, 0xE9, 0, 0, 0, 0], 1, stub)?,
                resign(
                    keyword::WAS_LOOTED,
                    keyword::FUNCTION,
                    signature::FN_O,
                    signature::FN_OF,
                ),
            ])
        }
        _ => Ok(Vec::new()),
    }
}

/// `HP` (0x4B4434, `fld [eax + 0x28]; mov edx, [ebp + 0x10]`, EAX the unit's companion): the companion's float at
/// four times the second argument (10 the hit points). An index outside the companion's vitals returns 0.
extern "C" fn hp_by_index(_: usize, ctx: &mut FpuContext) {
    /// The companion's head and its `UnitVitals`, which the author's list of indices covers.
    const COMPANION_VITALS: i32 = 0x20 + 0x58;
    let regs = &mut ctx.regs;
    // SAFETY: ESI is the function call `SSCCmd::CalcFloat` evaluates.
    let index = float_arg(unsafe { command(regs.esi) }, 0).map_or(-1, whole);
    let offset = index.wrapping_mul(4);
    if !(0..COMPANION_VITALS).contains(&offset) {
        regs.resume = eix_sites::SSC_CMD_CALC_FLOAT__B50;
        return;
    }
    // SAFETY: EAX is the unit's companion, a `CMapObjectServer` with its vitals.
    let value: f32 = unsafe { Obj::new(regs.eax, COMPANION_VITALS as u32) }.get(offset as u32);
    // SAFETY: EBP is `CalcFloat`'s frame, whose +0x10 is the result's address.
    regs.edx = unsafe { read::<u32>(regs.ebp.wrapping_add(0x10)) };
    if !ctx.fpu.push(Ext::new(value.into())) {
        regs.resume = eix_sites::SSC_CMD_CALC_FLOAT__B50;
    }
}

/// `hptrans` = 1: `HP(unit, index)` ([`hp_by_index`]).
fn hp_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    const HP: [u8; 6] = [0xD9, 0x40, 0x28, 0x8B, 0x55, 0x10];
    Ok(vec![
        hook::fpu_hook(
            eix_sites::SSC_CMD_CALC_FLOAT__254,
            &HP,
            &HP,
            Mode::Drop,
            hp_by_index,
            0,
        )?,
        resign(
            keyword::HP,
            keyword::FUNCTION,
            signature::FN_O,
            signature::FN_OF,
        ),
    ])
}

/// `Mana` (0x4B44A4, `fld [eax + 0x34]; mov edx, [ebp + 0x10]`, EAX the unit and EDX its companion after the rewritten
/// load): the float at four times the second argument in the unit's `UnitStats` (1 its experience, 433 the right leg's
/// vitality), which its server controller holds. Another object, a unit without a controller or an index outside the
/// stats returns 0.
extern "C" fn stat_by_index(_: usize, ctx: &mut FpuContext) {
    const STATS: u32 = offset_of!(UnitServer, stats) as u32;
    let regs = &mut ctx.regs;
    let object = core::ptr::with_exposed_provenance_mut::<EngineObject>(regs.eax as usize);
    // SAFETY: EAX is the function's object argument, a live engine object.
    let controller = if unsafe { EngineObject::class_id(object) } == CLASS_UNIT {
        // SAFETY: a unit's +0x240 is its controller or null.
        unsafe { read::<u32>(regs.eax + offset_of!(Unit, controller) as u32) }
    } else {
        0
    };
    // SAFETY: ESI is the function call `SSCCmd::CalcFloat` evaluates.
    let index = float_arg(unsafe { command(regs.esi) }, 0).map_or(-1, whole);
    let offset = index.wrapping_mul(4);
    if controller == 0 || !(0..size_of::<UnitStats>() as i32).contains(&offset) {
        regs.resume = eix_sites::SSC_CMD_CALC_FLOAT__B50;
        return;
    }
    // SAFETY: on the server a unit's controller is its `CUnitServer`, whose `UnitStats` are at +0x1E0.
    let stats = unsafe {
        Obj::new(
            controller.wrapping_add(STATS),
            size_of::<UnitStats>() as u32,
        )
    };
    let value: f32 = stats.get(offset as u32);
    // SAFETY: EBP is `CalcFloat`'s frame, whose +0x10 is the result's address.
    regs.edx = unsafe { read::<u32>(regs.ebp.wrapping_add(0x10)) };
    if !ctx.fpu.push(Ext::new(value.into())) {
        regs.resume = eix_sites::SSC_CMD_CALC_FLOAT__B50;
    }
}

/// `mptrans` = 1: `Mana(unit, index)` ([`stat_by_index`]); `HP` takes the argument as well.
fn mp_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    const MANA: [u8; 6] = [0xD9, 0x40, 0x34, 0x8B, 0x55, 0x10];
    Ok(vec![
        // `mov eax, [eax + 0x170]; test eax, eax` -> `mov edx, ...; test edx, edx`: EAX keeps the unit.
        bytes(eix_sites::SSC_CMD_CALC_FLOAT__2B7, &[0x80], &[0x90])?,
        bytes(eix_sites::SSC_CMD_CALC_FLOAT__2BD, &[0xC0], &[0xD2])?,
        hook::fpu_hook(
            eix_sites::SSC_CMD_CALC_FLOAT__2C4,
            &MANA,
            &MANA,
            Mode::Drop,
            stat_by_index,
            0,
        )?,
        resign(
            keyword::HP,
            keyword::FUNCTION,
            signature::FN_O,
            signature::FN_OF,
        ),
        resign(
            keyword::MANA,
            keyword::FUNCTION,
            signature::FN_O,
            signature::FN_OF,
        ),
    ])
}

/// `groupseetrans` = 1: `GroupSee` takes a third, float argument (the signature `"Ggf"`, written into
/// `SSCCmd::CalcGroup`'s padding); its case stays the game's.
fn group_see_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    const GGF: Site = eix_sites::SSC_CMD_CALC_GROUP__3C9;
    Ok(vec![
        SiteWrite::plain(GGF, &[0x90; 4], b"Ggf\0")?,
        resign(keyword::GROUP_SEE, keyword::FUNCTION, signature::FN_GG, GGF),
    ])
}

/// `GetPlayer`'s load (0x4B49E9, `fild [esi + 0x250]`; the case's rewritten loads leave ESI the call, or 0 for an
/// object that is not a unit): the 32-bit integer at the unit plus four times the index, rounded. The original reads
/// through a null ESI and any offset; ours returns 0 there, as the game does for a non-unit.
extern "C" fn player_word(_: usize, ctx: &mut FpuContext) {
    /// `GetPlayer`'s exit with the result 0, which the case already stored.
    const ZERO: Site = eix_sites::SSC_CMD_CALC_FLOAT__12C3;
    let regs = &mut ctx.regs;
    if regs.esi == 0 {
        regs.resume = ZERO;
        return;
    }
    // SAFETY: ESI is the function call `SSCCmd::CalcFloat` evaluates.
    let cmd = unsafe { command(regs.esi) };
    let unit = unit_arg(cmd, 0);
    let offset = float_arg(cmd, 0).map_or(-1, whole).wrapping_mul(4);
    if unit == 0 || !(0..=(size_of::<Unit>() as i32 - 4)).contains(&offset) {
        regs.resume = ZERO;
        return;
    }
    // SAFETY: `unit` is a live `CUnit` of `size_of::<Unit>()` bytes, the argument the case checked.
    let value: i32 = unsafe { Obj::new(unit, size_of::<Unit>() as u32) }.get(offset as u32);
    // An i32 is exact in an f64, as `fild` loads it.
    if !ctx.fpu.push(Ext::new(f64::from(value))) {
        regs.resume = ZERO;
    }
}

/// `getplayertrans` = 1: `GetPlayer(object, index)` ([`player_word`]), the signature `"of"`.
fn get_player_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    const LOAD: [u8; 6] = [0xDB, 0x86, 0x50, 0x02, 0x00, 0x00];
    let mut writes = common::fixed(&[
        (
            eix_sites::SSC_CMD_CALC_FLOAT__7E5,
            &[0x31, 0x85, 0xF6, 0x74, 0x0C, 0x8B, 0x16, 0x8B, 0xCE],
            &[0x11, 0x85, 0xD2, 0x74, 0x0C, 0x89, 0xD1, 0x8B, 0x12],
        ),
        (eix_sites::SSC_CMD_CALC_FLOAT__7FC, &[0xF6], &[0xD2]),
    ])?;
    writes.push(hook::fpu_hook(
        eix_sites::SSC_CMD_CALC_FLOAT__809,
        &LOAD,
        &LOAD,
        Mode::Drop,
        player_word,
        0,
    )?);
    writes.push(resign(
        keyword::GET_PLAYER,
        keyword::FUNCTION,
        signature::FN_O,
        signature::FN_OF,
    ));
    Ok(writes)
}

/// `SetPlayer`'s case after its class check (0x4B7106, `mov ecx, [esi + 8]; fld [ecx]`, EDI the unit, ESI the
/// command): a field in [132, 149) (the table the original keeps in `ScriptItemRepair`'s padding) goes the game's
/// way; any other stores the value, rounded, as a 32-bit integer at the unit plus four times the field, and ends the
/// command. The original leaves the field on the x87 stack on the game's way, and stores at any offset; ours does
/// neither.
extern "C" fn set_player_word(_: usize, ctx: &mut FpuContext) {
    /// The end of the command, past the game's `SetPlayer`.
    const DONE: Site = eix_sites::TEXT_4B9BC5;
    let regs = &mut ctx.regs;
    // SAFETY: ESI is the command `ScriptRun::Execute` runs.
    let cmd = unsafe { command(regs.esi) };
    let value = float_arg(cmd, 0).unwrap_or(0.0);
    let field = float_arg(cmd, 1).unwrap_or(0.0);
    if (SET_PLAYER_GAME[0]..SET_PLAYER_GAME[1]).contains(&field) {
        let floats = offset_of!(ScriptCommand, float_args) as u32;
        regs.ecx = cmd.get(floats + 4);
        if !ctx.fpu.push(Ext::new(value.into())) {
            regs.resume = DONE;
        }
        return;
    }
    regs.resume = DONE;
    let offset = whole(field).wrapping_mul(4);
    if (0..=(size_of::<Unit>() as i32 - 4)).contains(&offset) {
        // SAFETY: EDI is the `CUnit` the case's class check passed.
        unsafe { Obj::new(regs.edi, size_of::<Unit>() as u32) }.set(offset as u32, whole(value));
    }
}

/// The fields `SetPlayer` leaves to the game: from the first, up to the second.
const SET_PLAYER_GAME: [f32; 2] = [132.0, 149.0];

/// `setplayertrans` = 1: `SetPlayer(unit, value, field)` ([`set_player_word`]), the signature `"off"`.
fn set_player_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    const CASE: [u8; 5] = [0x8B, 0x4E, 0x08, 0xD9, 0x01];
    let mut table = [0; 8];
    table[..4].copy_from_slice(&SET_PLAYER_GAME[0].to_le_bytes());
    table[4..].copy_from_slice(&SET_PLAYER_GAME[1].to_le_bytes());
    Ok(vec![
        hook::fpu_hook(
            eix_sites::SCRIPT_RUN_EXECUTE__9A6,
            &CASE,
            &CASE,
            Mode::Drop,
            set_player_word,
            0,
        )?,
        bytes(eix_sites::SCRIPT_ITEM_REPAIR__8, &[0x90; 8], &table)?,
        resign(
            keyword::SET_PLAYER,
            keyword::COMMAND,
            signature::OF,
            signature::OFF,
        ),
    ])
}

/// `Stand`'s case (0x4B73C1, `mov ecx, [esi + 0x30]; mov ecx, [ecx]`, ESI the command): before the game's own
/// `Stand`, a unit argument is revived: its packed hit points' low byte becomes 1, its dead flag's low byte and the
/// client copy's byte +0x434 become 0. ECX is left as the replaced loads leave it. The original swaps saved registers
/// when the argument is no unit; ours writes nothing then.
extern "C" fn stand_revives(_: usize, ctx: &mut Context) {
    // SAFETY: ESI is the command `ScriptRun::Execute` runs.
    let cmd = unsafe { command(ctx.esi) };
    let objects = offset_of!(ScriptCommand, object_args) as u32;
    let data: u32 = cmd.get(objects + 4);
    // SAFETY: `Stand`'s signature gives the command one object argument, whose reference the case loads here.
    ctx.ecx = unsafe { read::<u32>(data) };
    let unit = unit_arg(cmd, 0);
    if unit == 0 {
        return;
    }
    // SAFETY: `unit` is a live `CUnit` of `size_of::<Unit>()` bytes.
    let unit = unsafe { Obj::new(unit, size_of::<Unit>() as u32) };
    unit.set(0x17E, 1u8);
    unit.set(0x274, 0u8);
    unit.set(0x434, 0u8);
}

/// `standtrans` = 1: `Stand(unit)` also revives the unit ([`stand_revives`]).
fn stand_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(common::call_at(
        eix_sites::SCRIPT_RUN_EXECUTE__C61,
        &[0x8B, 0x4E, 0x30, 0x8B, 0x09],
        0,
        stand_revives,
        0,
    )?)
}

/// The info panel's unit name as [`unit_id_name`] writes it: the panel draws it after the hook returns.
static ID_NAME: [AtomicU8; 256] = [const { AtomicU8::new(0) }; 256];

/// The info panel's name (0x646361, `cmp eax, esi; mov [esp + 0x14], ecx`, ECX the name, EDI the unit): it becomes
/// `"<unit id> <name>"`, cut to fit [`ID_NAME`]; the flags stay those of `cmp eax, esi` for the next `je`. Without a
/// unit the name stays (the original reads through null).
extern "C" fn unit_id_name(_: usize, ctx: &mut Context) {
    ctx.eflags = eix_core::x86_flags::cmp(ctx.eflags, ctx.eax, ctx.esi);
    let mut name = ctx.ecx;
    if ctx.edi != 0 && ctx.ecx != 0 {
        // SAFETY: EDI is the unit the panel shows, an engine object.
        let id = unsafe { object_id(ctx.edi) };
        // SAFETY: ECX is the unit's name, a NUL-terminated game string.
        let text = unsafe { CStr::from_ptr(core::ptr::with_exposed_provenance(ctx.ecx as usize)) };
        let mut line = format!("{id} ").into_bytes();
        line.extend_from_slice(text.to_bytes());
        line.truncate(ID_NAME.len() - 1);
        line.push(0);
        for (cell, &b) in ID_NAME.iter().zip(&line) {
            cell.store(b, Ordering::Relaxed);
        }
        name = ID_NAME.as_ptr().expose_provenance() as u32;
    }
    ctx.set_stack_word(5, name);
}

/// `drawunitsids` = 1: the info panel shows the unit's id before its name ([`unit_id_name`]).
fn draw_units_ids(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(common::jump_at(
        eix_sites::C_HUD_UNIT_PANEL_DRAW_VANILLA__C1,
        &[0x3B, 0xC6, 0x89, 0x4C, 0x24, 0x14],
        1,
        unit_id_name,
        0,
    )?)
}

/// `MaxHP` after `legendaryspellsystem`'s load of the unit's controller (0x4B446D, `[fld [eax + 0x2c];] mov edx,
/// [ebp + 0x10]`, EAX the controller, ESI the call): the unsigned byte at controller + 0x8A8 + the index, rounded.
/// An index outside the controller returns 0 (the original reads any address).
extern "C" fn skill_byte(_: usize, ctx: &mut FpuContext) {
    /// The skill bits' place in `CUnitServer`.
    const SKILLS: i32 = 0x8A8;
    let regs = &mut ctx.regs;
    // SAFETY: ESI is the function call `SSCCmd::CalcFloat` evaluates.
    let index = float_arg(unsafe { command(regs.esi) }, 0).map_or(-1, whole);
    let offset = SKILLS.wrapping_add(index);
    if !(SKILLS..size_of::<UnitServer>() as i32).contains(&offset) {
        regs.resume = eix_sites::SSC_CMD_CALC_FLOAT__B50;
        return;
    }
    // SAFETY: EAX is the unit's controller, a live `CUnitServer`.
    let value: u8 =
        unsafe { Obj::new(regs.eax, size_of::<UnitServer>() as u32) }.get(offset as u32);
    // SAFETY: EBP is `CalcFloat`'s frame, whose +0x10 is the result's address.
    regs.edx = unsafe { read::<u32>(regs.ebp.wrapping_add(0x10)) };
    if !ctx.fpu.push(Ext::new(value.into())) {
        regs.resume = eix_sites::SSC_CMD_CALC_FLOAT__B50;
    }
}

/// `SetPlayer`'s case's class check (0x4B70F6, `mov eax, [edi]; mov ecx, edi; call [eax + 0x30]`, EDI the object,
/// EAX the case's index, ESI the command), which `GiveQuestItem` reaches through the case table: for
/// `GiveQuestItem` (index 0x90) on a unit, the controller's byte +0x8B2 becomes the float, rounded, and the command
/// ends. Otherwise the check runs as the game's.
extern "C" fn quest_item_bit(_: usize, ctx: &mut Context) {
    /// `GiveQuestItem`'s case index.
    const GIVE_QUEST_ITEM: u32 = 0x90;
    let ptr = core::ptr::with_exposed_provenance_mut::<EngineObject>(ctx.edi as usize);
    let index = ctx.eax;
    // SAFETY: EDI is the command's object, a live engine object (the case checked it for null).
    ctx.eax = unsafe { EngineObject::class_id(ptr) };
    ctx.ecx = ctx.edi;
    if index != GIVE_QUEST_ITEM || ctx.eax != CLASS_UNIT {
        return;
    }
    // SAFETY: a unit, whose +0x240 is its controller or null.
    let controller = unsafe { read::<u32>(ctx.edi + 0x240) };
    if controller != 0 {
        // SAFETY: ESI is the command `ScriptRun::Execute` runs.
        let value = float_arg(unsafe { command(ctx.esi) }, 0).map_or(0, whole);
        // SAFETY: the unit's live `CUnitServer`.
        unsafe { Obj::new(controller, size_of::<UnitServer>() as u32) }.set(0x8B2, value as u8);
    }
    ctx.resume = eix_sites::TEXT_4B9BC5;
}

/// `legendaryspellsystem` = 1: `MaxHP(unit, index)` returns the unit's skill bits ([`skill_byte`]),
/// `GiveQuestItem(unit, value)` sets one ([`quest_item_bit`]), and `MaxMana` takes seven floats. The original's 96
/// bytes over `newspells`' code at 0x79AA80 are not written: the built-in `newspells` keeps its code elsewhere, so
/// nothing reaches them.
fn legendary_spell_system(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    const MAX_HP: [u8; 6] = [0xD9, 0x40, 0x2C, 0x8B, 0x55, 0x10];
    const CHECK: [u8; 7] = [0x8B, 0x07, 0x8B, 0xCF, 0xFF, 0x50, 0x30];
    const FN_FFFFFFF: Site = eix_sites::DATA_78CD8C;
    let mut writes = common::fixed(&[
        (
            eix_sites::SSC_CMD_CALC_FLOAT__280,
            &[0x70, 0x01],
            &[0x40, 0x02],
        ),
        // The case table byte of `GiveQuestItem` (id 0x92): SetPlayer's case.
        (eix_sites::TEXT_4B9EA8, &[0x4A], &[0x2A]),
    ])?;
    writes.push(SiteWrite::nop(
        eix_sites::SSC_CMD_CALC_FLOAT__28C,
        &MAX_HP[..1],
    ));
    writes.push(hook::fpu_hook(
        eix_sites::SSC_CMD_CALC_FLOAT__28D,
        &MAX_HP[1..],
        &MAX_HP[1..],
        Mode::Drop,
        skill_byte,
        0,
    )?);
    writes.extend(common::jump_at(
        eix_sites::SCRIPT_RUN_EXECUTE__996,
        &CHECK,
        2,
        quest_item_bit,
        0,
    )?);
    writes.push(resign(
        keyword::GIVE_QUEST_ITEM,
        keyword::COMMAND,
        signature::FS,
        signature::OF,
    ));
    writes.push(resign(
        keyword::MAX_HP,
        keyword::FUNCTION,
        signature::FN_O,
        signature::FN_OF,
    ));
    writes.push(resign(
        keyword::MAX_MANA,
        keyword::FUNCTION,
        signature::FN_O,
        FN_FFFFFFF,
    ));
    Ok(writes)
}

/// `UnitSee`'s walk (0x4B5C39, `mov esi, [eax + 0xa94]`, EAX the unit's controller, ESI the command): the list it
/// walks by the mode float's bits: 0 the visible living units (+0xA94, the game's), negative as an integer (-0.0
/// included) +0xA78, any other value (NaN included) +0xAB0.
extern "C" fn unit_see_list(_: usize, ctx: &mut Context) {
    // SAFETY: ESI is the command `SSCCmd::CalcGroup` evaluates.
    let bits = float_arg(unsafe { command(ctx.esi) }, 0).map_or(0, f32::to_bits);
    let list = match bits as i32 {
        0 => 0xA94,
        ..0 => 0xA78,
        _ => 0xAB0,
    };
    // SAFETY: EAX is the unit's controller (`CUnitServer`), which holds the three list heads.
    ctx.esi = unsafe { read::<u32>(ctx.eax + list) };
}

/// `unitseetrans` = 1: `UnitSee(unit, mode)` (the signature `"Gof"`, written into `SSCCmd::CalcGroup`'s padding).
fn unit_see_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    const GOF: Site = eix_sites::SSC_CMD_CALC_GROUP__3C5;
    let mut writes = common::call_at(
        eix_sites::SSC_CMD_CALC_GROUP__139,
        &[0x8B, 0xB0, 0x94, 0x0A, 0x00, 0x00],
        1,
        unit_see_list,
        0,
    )?;
    writes.push(SiteWrite::plain(GOF, &[0x90; 4], b"Gof\0")?);
    writes.push(resign(
        keyword::UNIT_SEE,
        keyword::FUNCTION,
        signature::FN_GO,
        GOF,
    ));
    Ok(writes)
}

/// `getaiclasstrans` = 1: `GetAIClass` reads the unit's controller and its field +0x984 instead of storing -1 first
/// and reading +0x324.
fn get_ai_class_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    common::fixed(&[
        (
            eix_sites::SSC_CMD_CALC_FLOAT__842,
            &[0xC7, 0x00, 0x00, 0x00, 0x80, 0xBF, 0x0F, 0x84, 0x75, 0x0A],
            &[0x0F, 0x84, 0x7B, 0x0A, 0x00, 0x00, 0x8B, 0xB6, 0x40, 0x02],
        ),
        (
            eix_sites::SSC_CMD_CALC_FLOAT__850,
            &[0x24, 0x03],
            &[0x84, 0x09],
        ),
    ])
}

// --- Commands ------------------------------------------------------------------------------------------------------------

/// `Cast` (0x4B7554, `push 0; lea ecx, [edi + 0x980]` before the brain's cast order): the order's first argument, the
/// spell, becomes the command's third float (ESI points at the floats here).
extern "C" fn cast_spell_number(_: usize, ctx: &mut Context) {
    // SAFETY: ESI is the command's float block, which the new signature gives three floats.
    let spell = whole(unsafe { read::<f32>(ctx.esi + 8) });
    ctx.set_stack_word(0, spell as u32);
}

/// `AIBrain`'s cast orders (thiscall, `ret 0xC`): at a point, and on an object.
type CastAt = unsafe extern "thiscall" fn(brain: u32, spell: i32, point: u32, flag: i32);
type CastOn = unsafe extern "thiscall" fn(brain: u32, spell: i32, target: u32, flag: i32);
const CAST_AT: Site = eix_sites::AI_BRAIN_CAST_AT;
const CAST_ON: Site = eix_sites::AI_BRAIN_CAST_AT__40;

/// `Cast`'s case after the ground height's call (0x4B753C; EBX the script run, ESI the float block, EDI the caster's
/// controller, `st(0)` the height): the caster casts its spell numbered by the third float on the object argument 1 when it
/// is set, else at the point. The original finds the command through a stale stack word and leaves the height on the
/// x87 stack on the object's way; ours takes the run's current command and pops the height on both ways.
extern "C" fn cast_on_object(_: usize, ctx: &mut FpuContext) {
    /// The end of the case.
    const DONE: Site = eix_sites::SCRIPT_RUN_EXECUTE__E04;
    let height = ctx.fpu.pop().map_or(0.0, Ext::to_f32);
    let regs = &mut ctx.regs;
    regs.resume = DONE;
    // SAFETY: EBX is the `ScriptRun`: its command list (+0x0) and the running command's index in it (+0x4).
    let run = unsafe { Obj::new(regs.ebx, 8) };
    // SAFETY: the run's command list is a live `GameArray` (its data at +0x4).
    let list = unsafe { Obj::new(run.get(0), 12) };
    let at = list.get::<u32>(4) + run.get::<u32>(4) * size_of::<ScriptCommand>() as u32;
    // SAFETY: the running command, where the case's fetch (0x4B67D7) found it.
    let cmd = unsafe { command(at) };
    let spell = whole(float_arg(cmd, 2).unwrap_or(0.0));
    let brain = regs.edi + 0x980;
    let target = object_arg(cmd, 1);
    if target != 0 {
        // SAFETY: the game's cast order, of this signature.
        let cast_on = unsafe { hook::as_fn::<CastOn>(CAST_ON) };
        // SAFETY: the caster's live brain and an object a script reference holds, on the game's thread.
        unsafe { cast_on(brain, spell, target, 0) };
        return;
    }
    // SAFETY: the case's frame: the x and y it saved at EBP-0x158 and EBP-0x154, its point at EBP-0x48.
    let frame = unsafe { Obj::new(regs.ebp - 0x158, 0x158) };
    let point = 0x158 - 0x48;
    frame.set(point, frame.get::<u32>(0));
    frame.set(point + 4, frame.get::<u32>(4));
    frame.set(point + 8, height);
    // SAFETY: the game's cast order, of this signature.
    let cast_at = unsafe { hook::as_fn::<CastAt>(CAST_AT) };
    // SAFETY: the caster's live brain and the point in the case's frame, on the game's thread.
    unsafe { cast_at(brain, spell, frame.addr() + point, 0) };
}

/// `castscripttrans`: 1 `Cast(unit, x, y, spell)` ([`cast_spell_number`]); 2 `Cast(unit, x, y, spell, target)`
/// ([`cast_on_object`]), the signature `"offfo"` written into padding at 0x4DA1FA as the original does.
fn cast_script_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    /// The padding the original writes the new signature into.
    const OFFFO: Site = eix_sites::GAME_BUFFER_READ_PAST_END__8A;
    match value {
        1 => Ok(vec![
            common::branch_before(
                eix_sites::SCRIPT_RUN_EXECUTE__DF4,
                &[0x6A, 0x00, 0x8D, 0x8F, 0x80, 0x09, 0x00, 0x00],
                &[0x90, 0x90, 0x90, 0xE9, 0, 0, 0, 0],
                3,
                cast_spell_number,
                0,
            )?,
            resign(
                keyword::CAST,
                keyword::COMMAND,
                signature::OFF,
                signature::OFFF,
            ),
        ]),
        2 => {
            const CASE: [u8; 6] = [0x8B, 0x8D, 0xAC, 0xFE, 0xFF, 0xFF];
            Ok(vec![
                SiteWrite::nop(eix_sites::SCRIPT_RUN_EXECUTE__DDC, &CASE[..1]),
                hook::fpu_hook(
                    eix_sites::SCRIPT_RUN_EXECUTE__DDD,
                    &CASE[1..],
                    &CASE[1..],
                    Mode::Drop,
                    cast_on_object,
                    0,
                )?,
                SiteWrite::plain(OFFFO, &[0x90; 6], b"offfo\0")?,
                resign(keyword::CAST, keyword::COMMAND, signature::OFF, OFFFO),
            ])
        }
        _ => Ok(Vec::new()),
    }
}

/// `CWorldServer::CastSpell`'s caster, the word the replaced instructions last pushed: the command's object argument
/// `arg` (ESI the command), when it is a unit.
extern "C" fn spell_caster(arg: usize, ctx: &mut Context) {
    // SAFETY: ESI is the command `ScriptRun::Execute` runs.
    let caster = unit_arg(unsafe { command(ctx.esi) }, arg);
    ctx.set_stack_word(0, caster);
}

/// `castspellunittrans` = 1: `CastSpellUnit(spell, x, y, target, caster)`.
fn cast_spell_unit_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![
        common::branch_before(
            eix_sites::SCRIPT_RUN_EXECUTE__F0B,
            &[0xB9, 0x60, 0xFF, 0x7A, 0x00, 0x50, 0x6A, 0x00],
            &[0xE9, 0, 0, 0, 0, 0x90, 0x90, 0x90],
            0,
            spell_caster,
            1,
        )?,
        // "sffo" -> "sffoo".
        bytes(eix_sites::DATA_78D5B4, &[0x00], b"o")?,
    ])
}

/// `castspellpointtrans` = 1: `CastSpellPoint(spell, x, y, x2, y2, caster)`; the case reads its floats through EAX so
/// that ESI keeps the command.
fn cast_spell_point_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let mut writes = common::fixed(&[
        (eix_sites::SCRIPT_RUN_EXECUTE__E32, &[0x76], &[0x46]),
        (
            eix_sites::SCRIPT_RUN_EXECUTE__E37,
            &[0x46, 0x04, 0x8B, 0x0E],
            &[0x40, 0x04, 0x8B, 0x08],
        ),
        (
            eix_sites::SCRIPT_RUN_EXECUTE__E52,
            &[0x56, 0x0C, 0xD9, 0x46],
            &[0x50, 0x0C, 0xD9, 0x40],
        ),
    ])?;
    writes.push(common::branch_before(
        eix_sites::SCRIPT_RUN_EXECUTE__E72,
        &[0x6A, 0x00, 0x52, 0x6A, 0x00],
        &[0xE9, 0, 0, 0, 0],
        0,
        spell_caster,
        0,
    )?);
    // "sffff" -> "sffffo".
    writes.push(bytes(eix_sites::DATA_78D5CD, &[0x00], b"o")?);
    Ok(writes)
}

/// `giveunitspelltrans` = 1: `GiveUnitSpell` creates a quick item (`CWorldServer::CreateItemNamed` 0x594AB0) and adds
/// it with `CUnit::AddQuickItem` 0x531180.
fn give_unit_spell_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![
        bytes(
            eix_sites::SCRIPT_RUN_EXECUTE__2DD7,
            &patch::call_rel32(
                eix_sites::SCRIPT_RUN_EXECUTE__2DD7,
                eix_sites::C_WORLD_SERVER_CREATE_SPELL_NAMED.ru(),
            ),
            &patch::call_rel32(
                eix_sites::SCRIPT_RUN_EXECUTE__2DD7,
                eix_sites::C_WORLD_SERVER_CREATE_ITEM_NAMED.ru(),
            ),
        )?,
        bytes(
            eix_sites::SCRIPT_RUN_EXECUTE__2DE7,
            &patch::call_rel32(
                eix_sites::SCRIPT_RUN_EXECUTE__2DE7,
                eix_sites::C_UNIT_ADD_SPELL.ru(),
            ),
            &patch::call_rel32(
                eix_sites::SCRIPT_RUN_EXECUTE__2DE7,
                eix_sites::C_UNIT_ADD_QUICK_ITEM.ru(),
            ),
        )?,
    ])
}

/// What `Run` and `Crawl` do with their new float argument.
#[derive(Debug, Clone, Copy)]
enum MoveArg {
    /// `CUnitServer::SetMoveStyle`.
    Style,
    /// `CUnitServer::SetWantedWeapon`.
    Weapon,
}

/// `Run` (0x4B73E8) and `Crawl` (0x4B73B3), in place of `call SetMoveStyle` (ECX the unit's controller, ESI the
/// command): the command's float argument, rounded, goes to the setter `arg` names.
extern "C" fn move_by_argument(arg: usize, ctx: &mut Context) {
    // SAFETY: ESI is the command `ScriptRun::Execute` runs.
    let value = float_arg(unsafe { command(ctx.esi) }, 0).map_or(0, whole);
    let server = core::ptr::with_exposed_provenance_mut::<UnitServer>(ctx.ecx as usize);
    let setter = if arg == MoveArg::Weapon as usize {
        UnitServer::set_wanted_weapon
    } else {
        UnitServer::set_move_style
    };
    // SAFETY: ECX is the unit's live server controller, which the case tested; on the game's thread.
    unsafe { setter(server, value) };
}

/// The cases' `push n; mov ecx, eax; call SetMoveStyle` become `nop x2; mov ecx, eax; call` [`move_by_argument`].
fn move_trans(
    site: Site,
    style: u8,
    entry: Site,
    arg: MoveArg,
) -> Result<Vec<SiteWrite>, BuildError> {
    let mut was = [0x6A, style, 0x8B, 0xC8, 0xE8, 0, 0, 0, 0];
    was[4..].copy_from_slice(&patch::call_rel32(
        site.offset(4),
        eix_sites::C_UNIT_SERVER_SET_MOVE_STYLE.ru(),
    ));
    let stub = common::call_stub(move_by_argument, arg as usize, site.offset(was.len()))?;
    Ok(vec![
        common::branch(
            site,
            &was,
            &[0x90, 0x90, 0x8B, 0xC8, 0xE8, 0, 0, 0, 0],
            4,
            stub,
        )?,
        resign(entry, keyword::COMMAND, signature::O, signature::OF),
    ])
}

/// `runtrans` = 1: `Run(unit, style)` sets the move style it names.
fn run_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    move_trans(
        eix_sites::SCRIPT_RUN_EXECUTE__C88,
        3,
        keyword::RUN,
        MoveArg::Style,
    )
}

/// `crawltrans` = 1: `Crawl(unit, slot)` sets the weapon slot the unit is to take up, and no move style.
fn crawl_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    move_trans(
        eix_sites::SCRIPT_RUN_EXECUTE__C53,
        1,
        keyword::CRAWL,
        MoveArg::Weapon,
    )
}

/// `KillUnit`'s case at 0x4B7181 and the original's rewrite of it: `KillUnit(a, b)` rounds its two floats and passes
/// them to `CWorldServer::SetWeather` 0x58EE00, so the command sets the weather instead of killing.
const KILL_UNIT: Bytes = (
    eix_sites::SCRIPT_RUN_EXECUTE__A21,
    &[
        0x8B, 0x46, 0x30, 0x8B, 0x08, 0x8B, 0x31, 0x8D, 0x8D, 0x0C, 0xFD, 0xFF, 0xFF, 0xE8, 0xAD,
        0xB9, 0x06, 0x00, 0x85, 0xF6, 0x0F, 0x84, 0x2A, 0x2A, 0x00, 0x00, 0x8B, 0x16, 0x6A, 0x00,
        0x8D, 0x85, 0x0C, 0xFD, 0xFF, 0xFF, 0x6A, 0x00, 0x50, 0x8B, 0xCE, 0xFF, 0x52, 0x7C,
    ],
    &[
        0x83, 0xF8, 0x33, 0x74, 0x0B, 0x3D, 0xA2, 0x00, 0x00, 0x00, 0x90, 0xE9, 0x00, 0x00, 0x00,
        0x00, 0x50, 0x8B, 0x46, 0x08, 0xD9, 0x40, 0x04, 0xDB, 0x1C, 0x24, 0x50, 0xD9, 0x00, 0xDB,
        0x1C, 0x24, 0xB9, 0x60, 0xFF, 0x7A, 0x00, 0xE8, 0x55, 0x7C, 0x0D, 0x00, 0x90, 0x90,
    ],
);

/// `killunittrans` = 1: [`KILL_UNIT`], with the signature `"ff"`.
fn kill_unit_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let mut writes = common::fixed(&[KILL_UNIT])?;
    writes.push(resign(
        keyword::KILL_UNIT,
        keyword::COMMAND,
        signature::O,
        signature::FF,
    ));
    Ok(writes)
}

/// `FixWorldTime`'s case (0x4B98C0, `mov ecx, 0x7aff60` after `push 0`): the time speed it pushed for
/// `CWorldServer::SetTimeSpeedImpl` becomes the second float's bits (ESI the command).
extern "C" fn world_time_speed(_: usize, ctx: &mut Context) {
    // SAFETY: ESI is the command `ScriptRun::Execute` runs.
    let speed = float_arg(unsafe { command(ctx.esi) }, 1).map_or(0, f32::to_bits);
    ctx.set_stack_word(0, speed);
    ctx.ecx = eix_sites::DATA_7AFF60.here32();
}

/// `fixworldtimetrans` = 1: `FixWorldTime(time, speed)`, the signature `"ff"`.
fn fix_world_time_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let mut writes = common::jump_at(
        eix_sites::TEXT_4B98C0,
        &[0xB9, 0x60, 0xFF, 0x7A, 0x00],
        0,
        world_time_speed,
        0,
    )?;
    writes.push(resign(
        keyword::FIX_WORLD_TIME,
        keyword::COMMAND,
        signature::F,
        signature::FF,
    ));
    Ok(writes)
}

/// `MoveToObject`'s case at 0x4B7297 before the original rewrites it.
const MOVE_TO_OBJECT: [u8; 67] = [
    0x83, 0xC1, 0x1C, 0x6A, 0x00, 0x6A, 0x00, 0x8B, 0x11, 0x89, 0x95, 0x14, 0xFE, 0xFF, 0xFF, 0x8B,
    0x51, 0x04, 0xD9, 0x85, 0x14, 0xFE, 0xFF, 0xFF, 0x8B, 0x49, 0x08, 0x89, 0x95, 0x18, 0xFE, 0xFF,
    0xFF, 0xD9, 0x5D, 0xAC, 0xD9, 0x85, 0x18, 0xFE, 0xFF, 0xFF, 0x8D, 0x55, 0xAC, 0x89, 0x8D, 0x1C,
    0xFE, 0xFF, 0xFF, 0xD9, 0x5D, 0xB0, 0x52, 0x8D, 0x88, 0x80, 0x09, 0x00, 0x00, 0xC7, 0x45, 0xB4,
    0x00, 0x00, 0x00,
];

/// Values 1 and 2: the unit's action becomes 5 (thieving) and `CUnitServer::OrderSearchBody` 0x555630 sends it to the
/// target, walking (`push 0`) or running (`push 1`, at offset 1).
const MOVE_TO_OBJECT_STEAL: [u8; 34] = [
    0x6A, 0x00, 0x51, 0x8B, 0xC8, 0xBA, 0x05, 0x00, 0x00, 0x00, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x89, 0x90, 0x84, 0x09, 0x00, 0x00, 0x90, 0x90, 0x90, 0xE8, 0x7B, 0xE3, 0x09, 0x00, 0x90, 0x90,
    0xEB, 0x27,
];

/// Values 3 and up: the action 5 with the four floats of the signature `"ooffff"` pushed for the game's own call.
const MOVE_TO_OBJECT_ITEM: [u8; 67] = [
    0xBA, 0x05, 0x00, 0x00, 0x00, 0x89, 0x90, 0x84, 0x09, 0x00, 0x00, 0x8B, 0x56, 0x08, 0x50, 0xD9,
    0x42, 0x0C, 0xDB, 0x5C, 0x24, 0x00, 0x50, 0xD9, 0x42, 0x08, 0xDB, 0x5C, 0x24, 0x00, 0x50, 0xD9,
    0x42, 0x04, 0xDB, 0x5C, 0x24, 0x00, 0x50, 0xD9, 0x42, 0x00, 0xDB, 0x5C, 0x24, 0x00, 0x51, 0x8B,
    0xC8, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90, 0x90, 0x90,
];

/// `movetoobjecttrans` >= 1: `MoveToObject` makes the unit use thieving on its target. 1 walks, 2 runs; 3 and up take
/// the argument list `"ooffff"`, written into `Terrain::GroundAndWaterDepth`'s padding.
fn move_to_object_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    const SITE: Site = eix_sites::SCRIPT_RUN_EXECUTE__B37;
    const OOFFFF: Site = eix_sites::TERRAIN_GROUND_AND_WATER_DEPTH__3E9;
    match value {
        1 | 2 => {
            let mut new = MOVE_TO_OBJECT_STEAL;
            new[1] = u8::from(value == 2);
            Ok(vec![bytes(SITE, &MOVE_TO_OBJECT[..new.len()], &new)?])
        }
        3.. => Ok(vec![
            bytes(SITE, &MOVE_TO_OBJECT, &MOVE_TO_OBJECT_ITEM)?,
            SiteWrite::plain(OOFFFF, &[0x90; 7], b"ooffff\0")?,
            resign(
                keyword::MOVE_TO_OBJECT,
                keyword::COMMAND,
                signature::OO,
                OOFFFF,
            ),
        ]),
        _ => Ok(Vec::new()),
    }
}

/// `PlayMusic`'s text (0x4B98A5, after the replaced call returned it in EAX): `"0..."` or `"1..."` switches the music
/// itself: the local player's map record's theme (`arg` the record's field, +4 for value 1, +0 for 2) goes to
/// `SetZoneMode` with mode 0, the music system's +0x44 becomes 0 and +0x38 the digit, and the command ends without
/// sending the text. Any other text, or no map or music system, goes the game's way (the original reads through
/// null there).
extern "C" fn music_by_digit(arg: usize, ctx: &mut Context) {
    if ctx.eax == 0 {
        return;
    }
    // SAFETY: EAX is the argument's text, a NUL-terminated game string.
    let digit = match unsafe { read::<u8>(ctx.eax) } {
        b'0' => 0u32,
        b'1' => 1,
        _ => return,
    };
    if common::zone_music(arg as u32, 0, 0, digit) {
        ctx.resume = eix_sites::TEXT_4B9BC5;
    }
}

/// `playmusictrans` 1 or 2: `PlayMusic(player, "0")` and `"1"` switch the local music ([`music_by_digit`]).
fn play_music_trans(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    let field = match value {
        1 => 4,
        2 => 0,
        _ => return Ok(Vec::new()),
    };
    Ok(vec![hook::context_hook(
        eix_sites::TEXT_4B98A5,
        &[0xE8, 0x16, 0xFB, 0xF5, 0xFF],
        Mode::Before,
        music_by_digit,
        field,
    )?])
}

/// `setspellaggressiontrans`: its block writes nothing (`SetSpellAggression` does nothing in the game either).
fn set_spell_aggression_trans(_: &Ctx<'_>, _: i8) -> Result<Vec<SiteWrite>, BuildError> {
    Ok(Vec::new())
}

// --- Script tools --------------------------------------------------------------------------------------------------------

/// The log `scriptchecking` appends to, in the current folder, as the original names it.
const SA_LOG: &str = "SAlog.txt";

/// Appends `line` to [`SA_LOG`]; a failure to open or write it loses the line, as the original's unchecked calls do.
fn sa_log(line: &[u8]) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(SA_LOG)
    {
        // The original ignores WriteFile's result; so does the log here.
        let _ = f.write_all(line);
    }
}

/// `ScriptError` after its `wvsprintfA` (0x4CF48F; the message at ESP+4, the script and the line in the caller's
/// first two arguments): appends `"<message>, line <line>\r\n"` to [`SA_LOG`]. `arg` 1: the error is not recorded in
/// the script, and `ScriptError` returns the message's length; 0: the replaced load runs and the game records it.
extern "C" fn script_error_line(arg: usize, ctx: &mut Context) {
    /// The caller's first argument, the script, as words from the site's ESP.
    const SCRIPT: usize = 0x408 / 4;
    let message_at = ctx.stack() + 4;
    // SAFETY: `wvsprintfA` has just written the NUL-terminated message into ScriptError's 1024-byte buffer there.
    let message = unsafe { CStr::from_ptr(core::ptr::with_exposed_provenance(message_at)) };
    let line = ctx.stack_word(SCRIPT + 1).unwrap_or(0) as i32;
    let mut text = message.to_bytes().to_vec();
    text.extend_from_slice(format!(", line {line}\r\n").as_bytes());
    sa_log(&text);
    if arg == 1 {
        ctx.eax = message.to_bytes().len() as u32;
        ctx.resume = eix_sites::SCRIPT_ERROR__52;
    } else {
        ctx.esi = ctx.stack_word(SCRIPT).unwrap_or(0);
    }
}

/// `LoadMobFile` (0x48168E, `xor ebx, ebx; mov [esp + 0x70], ebx`, the file name in EDI from its only caller):
/// appends `"\r\nLoading <file>\r\n"` to [`SA_LOG`], then does the replaced instructions' work.
extern "C" fn mob_file_line(_: usize, ctx: &mut Context) {
    if ctx.edi != 0 {
        // SAFETY: EDI is the mob file's name, a NUL-terminated string, as LoadMobFile's caller passes it.
        let name = unsafe { CStr::from_ptr(core::ptr::with_exposed_provenance(ctx.edi as usize)) };
        let mut text = b"\r\nLoading ".to_vec();
        text.extend_from_slice(name.to_bytes());
        text.extend_from_slice(b"\r\n");
        sa_log(&text);
    }
    ctx.ebx = 0;
    ctx.set_stack_word(0x70 / 4, 0);
}

/// `ScriptError` from its message on: the recording into the script and the return.
const SCRIPT_ERROR: [u8; 51] = [
    0x8B, 0xB4, 0x24, 0x08, 0x04, 0x00, 0x00, 0x8B, 0x84, 0x24, 0x0C, 0x04, 0x00, 0x00, 0x8D, 0x4C,
    0x24, 0x04, 0x51, 0x8D, 0x8E, 0xEC, 0x01, 0x00, 0x00, 0x89, 0x86, 0xF4, 0x01, 0x00, 0x00, 0xE8,
    0x46, 0x16, 0x23, 0x00, 0x8B, 0x94, 0x24, 0x10, 0x04, 0x00, 0x00, 0x33, 0xC0, 0x89, 0x96, 0xF0,
    0x01, 0x00, 0x00,
];

/// `scriptchecking` 1 to 7: script errors, and with 2 and 4 to 7 the mob files loaded, are appended to `SAlog.txt`.
/// 1, 2 keep the errors out of the script; 3, 4, 6, 7 return non-zero from `ScriptError`; 5 lets eight load errors
/// of `CScript::Load*` pass. 8 and up are skipped (the registry).
fn script_checking(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    // 7 writes what 6 does (the original's sites and code are the same).
    if !(1..=7).contains(&value) {
        return Ok(Vec::new());
    }
    // The strings the original keeps in padding; its own code reads them, ours carries its own.
    let mut writes = vec![SiteWrite::plain(
        eix_sites::TEXT_4CF106,
        &[0x90; 10],
        b"SAlog.txt\0",
    )?];
    if matches!(value, 2 | 4..=7) {
        writes.push(SiteWrite::plain(
            eix_sites::TEXT_481575,
            &[0x90; 11],
            b"Loading %s\0",
        )?);
        writes.extend(common::jump_at(
            eix_sites::LOAD_MOB_FILE__2E,
            &[0x33, 0xDB, 0x89, 0x5C, 0x24, 0x70],
            1,
            mob_file_line,
            0,
        )?);
    }
    if value <= 2 {
        // One write over the whole tail, as the original's: it covers the addon's AddNewScriptCommands hook inside.
        writes.push(hook::context_hook(
            eix_sites::SCRIPT_ERROR__1F,
            &SCRIPT_ERROR,
            Mode::Drop,
            script_error_line,
            1,
        )?);
        return Ok(writes);
    }
    writes.extend(common::jump_at(
        eix_sites::SCRIPT_ERROR__1F,
        &SCRIPT_ERROR[..7],
        2,
        script_error_line,
        0,
    )?);
    if value != 5 {
        writes.push(SiteWrite::nop(eix_sites::SCRIPT_ERROR__4A, &[0x33, 0xC0]));
    } else {
        writes.extend(common::nops(&LOAD_ERRORS));
    }
    Ok(writes)
}

/// The eight load-error branches (`je rel32`) of `CScript::Load*` that `scriptchecking` 5 lets pass.
const LOAD_ERRORS: [(Site, &[u8]); 8] = [
    (
        eix_sites::C_SCRIPT_LOAD_FUNCTION__8E0,
        &[0x0F, 0x84, 0x4F, 0x04, 0x00, 0x00],
    ),
    (
        eix_sites::C_SCRIPT_LOAD_COMMANDS__4A1,
        &[0x0F, 0x84, 0x78, 0x04, 0x00, 0x00],
    ),
    (
        eix_sites::C_SCRIPT_LOAD_VARIABLE_DECLARATIONS__11D,
        &[0x0F, 0x84, 0x01, 0x02, 0x00, 0x00],
    ),
    (
        eix_sites::C_SCRIPT_LOAD__445,
        &[0x0F, 0x84, 0x79, 0x05, 0x00, 0x00],
    ),
    (
        eix_sites::C_SCRIPT_LOAD__59E,
        &[0x0F, 0x84, 0x4D, 0x06, 0x00, 0x00],
    ),
    (
        eix_sites::C_SCRIPT_LOAD__5C1,
        &[0x0F, 0x84, 0xDC, 0x06, 0x00, 0x00],
    ),
    (
        eix_sites::C_SCRIPT_LOAD__8B1,
        &[0x0F, 0x84, 0xB8, 0x05, 0x00, 0x00],
    ),
    (
        eix_sites::C_SCRIPT_LOAD__8F7,
        &[0x0F, 0x84, 0x72, 0x05, 0x00, 0x00],
    ),
];

#[cfg(test)]
mod tests {
    use super::super::testing::{Original, assert_original, no_log, plain, values};
    use super::*;

    fn sites(writes: &[SiteWrite]) -> Vec<(Site, usize)> {
        writes.iter().map(|w| (w.site(), w.size())).collect()
    }

    fn ctx_values() -> eix_core::spelladdon::Values {
        values(&[])
    }

    #[test]
    fn genocide_takes_one_billion_as_a_hero() {
        // Two zeroed objects, large enough for the owner at +0x24C, with only their ids set.
        let object = |id: i32| {
            let mut o = vec![0u8; 0x250];
            let at = offset_of!(EngineObject, id);
            o[at..at + 4].copy_from_slice(&id.to_le_bytes());
            o
        };
        let records = |attacker: i32, victim: i32| {
            let (a, v) = (object(attacker), object(victim));
            // SAFETY: both buffers outlive the call and hold every field it reads.
            unsafe { genocide_records(Genocide::Count, a.as_ptr() as u32, v.as_ptr() as u32) }
        };
        assert!(records(1_000_000_000, 999_999_999));
        assert!(!records(999_999_999, 5));
        assert!(!records(1_000_000_001, 1_000_000_000));
    }

    #[test]
    fn plain_switches_write_the_originals_bytes() {
        let v = ctx_values();
        let ctx = Ctx::new(&v, no_log);
        assert_eq!(
            plain(&remove_frozen_var(&ctx, 1).unwrap()),
            [(eix_sites::ENTER_ZONE__18E, vec![0x75], vec![0xEB])]
        );
        assert_eq!(
            plain(&free_multiplayer_vars(&ctx, 1).unwrap()),
            [
                (
                    eix_sites::CLASS_TEMPLATES_BUILD__37,
                    vec![0x0F, 0x84],
                    vec![0x90, 0xE9]
                ),
                (
                    eix_sites::GAME_FLOW_UPDATE_FROM_SERVER__113,
                    vec![0x77],
                    vec![0xEB]
                )
            ]
        );
        // The redirected calls: E8 84 B6 0D 00 -> E8 74 B5 0D 00, E8 D4 82 07 00 -> E8 34 7C 07 00.
        assert_eq!(
            plain(&give_unit_spell_trans(&ctx, 1).unwrap()),
            [
                (
                    eix_sites::SCRIPT_RUN_EXECUTE__2DD7,
                    vec![0xE8, 0x84, 0xB6, 0x0D, 0x00],
                    vec![0xE8, 0x74, 0xB5, 0x0D, 0x00]
                ),
                (
                    eix_sites::SCRIPT_RUN_EXECUTE__2DE7,
                    vec![0xE8, 0xD4, 0x82, 0x07, 0x00],
                    vec![0xE8, 0x34, 0x7C, 0x07, 0x00]
                )
            ]
        );
        assert_eq!(
            plain(&was_looted_trans(&ctx, 1).unwrap()),
            [(
                eix_sites::SSC_CMD_CALC_FLOAT__49F,
                vec![0xC1, 0xEA, 0x14, 0x83, 0xE2, 0x01],
                vec![0x90; 6]
            )]
        );
        for f in [
            remove_frozen_var,
            free_multiplayer_vars,
            give_unit_spell_trans,
            get_ai_class_trans,
            kill_unit_trans,
            group_see_trans,
            hp_trans,
            mp_trans,
            run_trans,
            crawl_trans,
            cast_spell_unit_trans,
            cast_spell_point_trans,
            gs_get_var,
            gs_set_var,
        ] {
            assert!(f(&ctx, 2).unwrap().is_empty());
            assert!(f(&ctx, 0).unwrap().is_empty());
        }
        assert!(set_spell_aggression_trans(&ctx, 1).unwrap().is_empty());
    }

    #[test]
    fn signatures_point_at_the_games_strings() {
        let v = ctx_values();
        let ctx = Ctx::new(&v, no_log);
        // Cast "off" -> "offf"; the pointer at 0x78BE2C changes in its low two bytes, as the original writes them.
        let w = plain(&cast_script_trans(&ctx, 1).unwrap());
        assert_eq!(
            w[1],
            (
                eix_sites::DATA_78BE2C,
                vec![0xE0, 0xD5, 0x78, 0x00],
                vec![0x4C, 0xD3, 0x78, 0x00]
            )
        );
        let w = plain(&kill_unit_trans(&ctx, 1).unwrap());
        assert_eq!(
            w[1],
            (
                eix_sites::DATA_78C0FC,
                vec![0xD0, 0xBB, 0x78, 0x00],
                vec![0x14, 0xD6, 0x78, 0x00]
            )
        );
        let w = plain(&group_see_trans(&ctx, 1).unwrap());
        assert_eq!(
            w[0],
            (
                eix_sites::SSC_CMD_CALC_GROUP__3C9,
                vec![0x90; 4],
                b"Ggf\0".to_vec()
            )
        );
        assert_eq!(
            w[1],
            (
                eix_sites::DATA_78C908,
                vec![0x28, 0xCC, 0x78, 0x00],
                vec![0xC9, 0x5E, 0x4B, 0x00]
            )
        );
        let w = plain(&move_to_object_trans(&ctx, 4).unwrap());
        assert_eq!(
            w[2],
            (
                eix_sites::DATA_78C15C,
                vec![0xF8, 0xD5, 0x78, 0x00],
                vec![0x19, 0x8D, 0x6C, 0x00]
            )
        );
        assert_eq!(
            w[1],
            (
                eix_sites::TERRAIN_GROUND_AND_WATER_DEPTH__3E9,
                vec![0x90; 7],
                b"ooffff\0".to_vec()
            )
        );
    }

    #[test]
    fn move_to_object_variants() {
        let v = ctx_values();
        let ctx = Ctx::new(&v, no_log);
        let walk = plain(&move_to_object_trans(&ctx, 1).unwrap());
        let run = plain(&move_to_object_trans(&ctx, 2).unwrap());
        assert_eq!(
            (walk[0].0, walk[0].1.len()),
            (eix_sites::SCRIPT_RUN_EXECUTE__B37, 34)
        );
        assert_eq!((walk[0].2[1], run[0].2[1]), (0, 1));
        assert_eq!(move_to_object_trans(&ctx, 3).unwrap().len(), 3);
        assert!(move_to_object_trans(&ctx, 0).unwrap().is_empty());
    }

    #[test]
    fn hooks_take_the_originals_sites() {
        let v = ctx_values();
        let ctx = Ctx::new(&v, no_log);
        assert_eq!(
            sites(&gs_get_var(&ctx, 1).unwrap()),
            [(eix_sites::NAMED_FLOAT_TABLE_GET__78, 5)]
        );
        assert_eq!(
            sites(&gs_set_var(&ctx, 1).unwrap()),
            [(eix_sites::SCRIPT_RUN_EXECUTE__1BB2, 6)]
        );
        assert_eq!(
            sites(&save_control(&ctx, 2).unwrap()),
            [
                (eix_sites::TEXT_62AD51, 5),
                (eix_sites::TEXT_62AEED, 5),
                (eix_sites::BATTLE_SCREEN_KEY_DOWN__BF7, 2),
                (eix_sites::BATTLE_SCREEN_KEY_DOWN__BF9, 5)
            ]
        );
        assert!(save_control(&ctx, 3).unwrap().is_empty());
        assert_eq!(
            sites(&notify_deaths(&ctx, 2).unwrap()),
            [(eix_sites::C_UNIT_SERVER_ON_DAMAGED__C5, 6)]
        );
        assert!(notify_deaths(&ctx, 3).unwrap().is_empty());
        assert_eq!(
            sites(&notify_lootings(&ctx, 3).unwrap()),
            [(eix_sites::C_UNIT_SERVER_USE_SKILL__7C, 5)]
        );
        assert!(notify_lootings(&ctx, 4).unwrap().is_empty());
        assert_eq!(
            sites(&auto_run_mob_file(&ctx, 2).unwrap()),
            [(eix_sites::C_WORLD_SERVER_CREATE_UNITS__B0C, 5)]
        );
        assert_eq!(
            sites(&infinite_add_mob_spawn(&ctx, 1).unwrap()),
            [
                (eix_sites::C_WORLD_SERVER_LOAD_MOB_FILE__B, 1),
                (eix_sites::C_WORLD_SERVER_LOAD_MOB_FILE__C, 5)
            ]
        );
        assert_eq!(
            sites(&was_looted_trans(&ctx, 2).unwrap()),
            [
                (eix_sites::SSC_CMD_CALC_FLOAT__49F, 6),
                (eix_sites::DATA_78CA78, 4)
            ]
        );
        assert_eq!(
            sites(&hp_trans(&ctx, 1).unwrap()),
            [
                (eix_sites::SSC_CMD_CALC_FLOAT__254, 6),
                (eix_sites::DATA_78C7B8, 4)
            ]
        );
        assert_eq!(
            sites(&mp_trans(&ctx, 1).unwrap()),
            [
                (eix_sites::SSC_CMD_CALC_FLOAT__2B7, 1),
                (eix_sites::SSC_CMD_CALC_FLOAT__2BD, 1),
                (eix_sites::SSC_CMD_CALC_FLOAT__2C4, 6),
                (eix_sites::DATA_78C7B8, 4),
                (eix_sites::DATA_78C7D8, 4)
            ]
        );
        assert_eq!(
            sites(&cast_spell_unit_trans(&ctx, 1).unwrap()),
            [
                (eix_sites::SCRIPT_RUN_EXECUTE__F0B, 8),
                (eix_sites::DATA_78D5B4, 1)
            ]
        );
        assert_eq!(
            sites(&cast_spell_point_trans(&ctx, 1).unwrap()),
            [
                (eix_sites::SCRIPT_RUN_EXECUTE__E32, 1),
                (eix_sites::SCRIPT_RUN_EXECUTE__E37, 4),
                (eix_sites::SCRIPT_RUN_EXECUTE__E52, 4),
                (eix_sites::SCRIPT_RUN_EXECUTE__E72, 5),
                (eix_sites::DATA_78D5CD, 1)
            ]
        );
        assert_eq!(
            sites(&run_trans(&ctx, 1).unwrap()),
            [
                (eix_sites::SCRIPT_RUN_EXECUTE__C88, 9),
                (eix_sites::DATA_78C32C, 4)
            ]
        );
        assert_eq!(
            sites(&crawl_trans(&ctx, 1).unwrap()),
            [
                (eix_sites::SCRIPT_RUN_EXECUTE__C53, 9),
                (eix_sites::DATA_78BEAC, 4)
            ]
        );
        assert_eq!(
            sites(&cast_script_trans(&ctx, 2).unwrap()),
            [
                (eix_sites::SCRIPT_RUN_EXECUTE__DDC, 1),
                (eix_sites::SCRIPT_RUN_EXECUTE__DDD, 5),
                (eix_sites::GAME_BUFFER_READ_PAST_END__8A, 6),
                (eix_sites::DATA_78BE2C, 4)
            ]
        );
        assert!(cast_script_trans(&ctx, 3).unwrap().is_empty());
    }

    #[test]
    fn layouts_keep_the_originals_bytes_but_the_targets() {
        let v = ctx_values();
        let ctx = Ctx::new(&v, no_log);
        let w = plain(&gs_set_var(&ctx, 1).unwrap());
        assert_eq!(w[0].2[..2], [0x90, 0xE9]);
        let w = plain(&was_looted_trans(&ctx, 2).unwrap());
        assert_eq!(w[0].2[..2], [0x51, 0xE9]);
        let w = plain(&cast_script_trans(&ctx, 1).unwrap());
        assert_eq!(w[0].2[..4], [0x90, 0x90, 0x90, 0xE9]);
        let w = plain(&run_trans(&ctx, 1).unwrap());
        assert_eq!(w[0].1[..5], [0x6A, 0x03, 0x8B, 0xC8, 0xE8]);
        assert_eq!(w[0].1[5..], [0x7F, 0xDE, 0x09, 0x00]);
        assert_eq!(w[0].2[..5], [0x90, 0x90, 0x8B, 0xC8, 0xE8]);
        let w = plain(&crawl_trans(&ctx, 1).unwrap());
        assert_eq!(w[0].1[5..], [0xB4, 0xDE, 0x09, 0x00]);
    }

    #[test]
    fn game_value_names() {
        let speed = c"GameSpeed";
        let other = c"Save";
        let short = c"Dif";
        assert!(is_difficulty(c"Difficulty".as_ptr() as u32));
        assert!(!is_difficulty(short.as_ptr() as u32));
        assert!(is_speed(c"xxxxpeed".as_ptr() as u32));
        assert!(!is_speed(speed.as_ptr() as u32));
        assert!(!is_speed(other.as_ptr() as u32));
        assert!(!is_speed(0));
    }

    #[test]
    fn auto_run_mob_line() {
        assert_eq!(
            AUTO_RUN_MOB.to_bytes(),
            b"AddMob(\"AutoRunSpellAddon123.mob\")"
        );
    }

    /// The original's code bytes for the features built from behaviour notes (for each value).
    const ORIGINAL: &[Original] = &[
        (
            "gameplay",
            "castscripttrans",
            2,
            &[],
            &[
                (eix_sites::SCRIPT_RUN_EXECUTE__DDC, "90 E9 ?? ?? ?? ??"),
                (
                    eix_sites::GAME_BUFFER_READ_PAST_END__8A,
                    "6F 66 66 66 6F 00",
                ),
            ],
        ),
        (
            "gameplay",
            "fixworldtimetrans",
            1,
            &[],
            &[(eix_sites::TEXT_4B98C0, "E9 ?? ?? ?? ??")],
        ),
        (
            "gameplay",
            "unitseetrans",
            1,
            &[],
            &[
                (eix_sites::SSC_CMD_CALC_GROUP__139, "90 E8 ?? ?? ?? ??"),
                (eix_sites::SSC_CMD_CALC_GROUP__3C5, "47 6F 66 00"),
            ],
        ),
        (
            "gameplay",
            "getplayertrans",
            1,
            &[],
            &[
                // `??` where the byte stays the game's.
                (
                    eix_sites::SSC_CMD_CALC_FLOAT__7E5,
                    "11 ?? D2 ?? ?? 89 D1 ?? 12",
                ),
                (eix_sites::SSC_CMD_CALC_FLOAT__7FC, "D2"),
                (eix_sites::SSC_CMD_CALC_FLOAT__809, "E9 ?? ?? ?? ?? 90"),
            ],
        ),
        (
            "gameplay",
            "setplayertrans",
            1,
            &[],
            &[
                (eix_sites::SCRIPT_RUN_EXECUTE__9A6, "E9 ?? ?? ?? ??"),
                (eix_sites::SCRIPT_ITEM_REPAIR__8, "00 00 04 43 00 00 15 43"),
            ],
        ),
        (
            "gameplay",
            "standtrans",
            1,
            &[],
            &[(eix_sites::SCRIPT_RUN_EXECUTE__C61, "E8 ?? ?? ?? ??")],
        ),
        (
            "tech",
            "drawunitsids",
            1,
            &[],
            &[(
                eix_sites::C_HUD_UNIT_PANEL_DRAW_VANILLA__C1,
                "90 E9 ?? ?? ?? ??",
            )],
        ),
        (
            "gameplay",
            "genocideincvars",
            1,
            &[],
            &[(eix_sites::C_UNIT_SERVER_TAKE_HIT__7B, "E8 ?? ?? ?? ?? 90")],
        ),
        (
            "gameplay",
            "genocideincvars",
            3,
            &[],
            &[(eix_sites::C_UNIT_SERVER_TAKE_HIT__7B, "E8 ?? ?? ?? ?? 90")],
        ),
        (
            "gameplay",
            "genocidedipvars",
            1,
            &[],
            &[(eix_sites::C_UNIT_SERVER_TAKE_HIT__7B, "E8 ?? ?? ?? ?? 90")],
        ),
        (
            "gameplay",
            "genocidedipvars",
            2,
            &[],
            &[(eix_sites::C_UNIT_SERVER_ON_DAMAGED__BE, "E8 ?? ?? ?? ?? 90")],
        ),
        (
            "gameplay",
            "genocidedipvars",
            5,
            &[],
            &[
                (eix_sites::C_UNIT_SERVER_ON_DAMAGED__BE, "E8 ?? ?? ?? ?? 90"),
                (eix_sites::C_UNIT_SERVER_TAKE_HIT__7B, "E8 ?? ?? ?? ?? 90"),
            ],
        ),
        ("tech", "scriptchecking", 2, &[], CHECKING_2),
        ("tech", "scriptchecking", 3, &[], CHECKING_3),
        ("tech", "scriptchecking", 4, &[], CHECKING_4),
        ("tech", "scriptchecking", 5, &[], CHECKING_5),
        // 6 and 7 write what 4 does.
        ("tech", "scriptchecking", 6, &[], CHECKING_4),
        ("tech", "scriptchecking", 7, &[], CHECKING_4),
        (
            "gameplay",
            "legendaryspellsystem",
            1,
            &[],
            &[
                (eix_sites::SSC_CMD_CALC_FLOAT__280, "40 02"),
                (eix_sites::SSC_CMD_CALC_FLOAT__28C, "90 E9 ?? ?? ?? ??"),
                (eix_sites::SCRIPT_RUN_EXECUTE__996, "90 90 E9 ?? ?? ?? ??"),
                (eix_sites::TEXT_4B9EA8, "2A"),
            ],
        ),
    ];

    #[test]
    fn play_music_jumps_from_the_text_call() {
        let v = ctx_values();
        let ctx = Ctx::new(&v, no_log);
        for value in [1, 2] {
            let w = plain(&play_music_trans(&ctx, value).unwrap());
            // The original changes the call's first four bytes (E8 -> E9 into its header code); ours all five.
            assert_eq!(w[0].0, eix_sites::TEXT_4B98A5);
            assert_eq!(w[0].1, [0xE8, 0x16, 0xFB, 0xF5, 0xFF]);
            assert_eq!(w[0].2[0], 0xE9);
        }
        assert!(play_music_trans(&ctx, 3).unwrap().is_empty());
    }

    const SA_LOG_NAME: (Site, &str) = (eix_sites::TEXT_4CF106, "53 41 6C 6F 67 2E 74 78 74 00");
    const LOADING: [(Site, &str); 2] = [
        (eix_sites::TEXT_481575, "4C 6F 61 64 69 6E 67 20 25 73 00"),
        (eix_sites::LOAD_MOB_FILE__2E, "90 E9 ?? ?? ?? ??"),
    ];
    /// Values 1 and 2 replace ScriptError's tail with the original's own code; ours with a jump into ours.
    const ERROR_TAIL: (Site, &str) = (
        eix_sites::SCRIPT_ERROR__1F,
        "?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? \
         ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ??",
    );
    const ERROR_JUMP: (Site, &str) = (eix_sites::SCRIPT_ERROR__1F, "90 90 E9 ?? ?? ?? ??");
    const CHECKING_2: &[(Site, &str)] = &[SA_LOG_NAME, LOADING[0], LOADING[1], ERROR_TAIL];
    const CHECKING_3: &[(Site, &str)] = &[
        SA_LOG_NAME,
        ERROR_JUMP,
        (eix_sites::SCRIPT_ERROR__4A, "90 90"),
    ];
    const CHECKING_4: &[(Site, &str)] = &[
        SA_LOG_NAME,
        LOADING[0],
        LOADING[1],
        ERROR_JUMP,
        (eix_sites::SCRIPT_ERROR__4A, "90 90"),
    ];
    const CHECKING_5: &[(Site, &str)] = &[
        SA_LOG_NAME,
        LOADING[0],
        LOADING[1],
        ERROR_JUMP,
        (eix_sites::C_SCRIPT_LOAD_FUNCTION__8E0, "90 90 90 90 90 90"),
        (eix_sites::C_SCRIPT_LOAD_COMMANDS__4A1, "90 90 90 90 90 90"),
        (
            eix_sites::C_SCRIPT_LOAD_VARIABLE_DECLARATIONS__11D,
            "90 90 90 90 90 90",
        ),
        (eix_sites::C_SCRIPT_LOAD__445, "90 90 90 90 90 90"),
        (eix_sites::C_SCRIPT_LOAD__59E, "90 90 90 90 90 90"),
        (eix_sites::C_SCRIPT_LOAD__5C1, "90 90 90 90 90 90"),
        (eix_sites::C_SCRIPT_LOAD__8B1, "90 90 90 90 90 90"),
        (eix_sites::C_SCRIPT_LOAD__8F7, "90 90 90 90 90 90"),
    ];

    #[test]
    fn noted_features_write_the_originals_bytes() {
        assert_original(ORIGINAL);
    }

    #[test]
    fn script_checking_seven_is_six() {
        let v = ctx_values();
        let ctx = Ctx::new(&v, no_log);
        assert_eq!(
            sites(&script_checking(&ctx, 7).unwrap()),
            sites(&script_checking(&ctx, 6).unwrap())
        );
        assert!(script_checking(&ctx, 0).unwrap().is_empty());
    }

    #[test]
    fn noted_features_change_the_originals_signatures() {
        let v = ctx_values();
        let ctx = Ctx::new(&v, no_log);
        // Each keyword pointer changes as the original writes it: "f" -> "ff", "Go" -> "Gof", "o" -> "of",
        // "of" -> "off".
        let pointer = |w: &[SiteWrite]| plain(w).into_iter().last().unwrap();
        assert_eq!(
            pointer(&fix_world_time_trans(&ctx, 1).unwrap()),
            (
                eix_sites::DATA_78BFBC,
                vec![0xD8, 0xBB, 0x78, 0],
                vec![0x14, 0xD6, 0x78, 0]
            )
        );
        assert_eq!(
            pointer(&unit_see_trans(&ctx, 1).unwrap()),
            (
                eix_sites::DATA_78C8F8,
                vec![0x38, 0xCC, 0x78, 0],
                vec![0xC5, 0x5E, 0x4B, 0]
            )
        );
        assert_eq!(
            pointer(&get_player_trans(&ctx, 1).unwrap()),
            (
                eix_sites::DATA_78C9B8,
                vec![0x44, 0xCD, 0x78, 0],
                vec![0xB8, 0xCC, 0x78, 0]
            )
        );
        assert_eq!(
            pointer(&set_player_trans(&ctx, 1).unwrap()),
            (
                eix_sites::DATA_78C41C,
                vec![0xC4, 0xD6, 0x78, 0],
                vec![0xE0, 0xD5, 0x78, 0]
            )
        );
        for f in [
            fix_world_time_trans,
            unit_see_trans,
            get_player_trans,
            set_player_trans,
            stand_trans,
            draw_units_ids,
        ] {
            assert!(f(&ctx, 2).unwrap().is_empty());
            assert!(f(&ctx, 0).unwrap().is_empty());
        }
    }
}
