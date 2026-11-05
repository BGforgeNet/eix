//! The commands, and the player-variable and file functions.
#![expect(unsafe_code, reason = "changing the game's units, players and world")]

use std::ffi::{CStr, CString};

use eix_core::script;
use eix_engine::float::ftol;
use eix_engine::game::console;
use eix_engine::game::flow::{self, DRAW_ALL};
use eix_engine::game::item::{Item, ItemArmor, ItemWeapon, WeaponRecord};
use eix_engine::game::mfc::{self, FormatArg};
use eix_engine::game::net;
use eix_engine::game::object::{CLASS_UNIT, EngineObject};
use eix_engine::game::player::{NamedFloatTable, Player, PlayerServer};
use eix_engine::game::unit::{MapObject, MapObjectServer, Unit, UnitServer};
use eix_engine::game::world::{CLIENT_WORLD, LOADING_MOVIE_OFF, SERVER_WORLD, World};
use eix_engine::win::{file_exists, from_ansi};

use super::{Call, MOD_DIR, invalid_arguments};
use crate::report;

/// Ids from here on are text keys: the player's and the named units'.
const TEXT_KEY: i32 = 1_000_000_000;

/// The player with the number of float argument `i`, or null.
fn player(call: &Call, i: usize) -> (i32, *mut Player) {
    let number = ftol(f64::from(call.float(i)));
    // SAFETY: script commands run on the game's thread; the server world is constructed.
    (number, unsafe {
        World::get_player(SERVER_WORLD.as_ptr(), number)
    })
}

/// `<command>: There is no player with number <n>`.
fn no_player(command: &str, number: i32) -> bool {
    report::script_error(
        command,
        format!("There is no player with number {number}").as_bytes(),
    )
}

/// Object argument 0 as a unit, with the original's reports for a missing object and a non-unit.
fn unit_argument(call: &Call, command: &str) -> Result<*mut Unit, bool> {
    let object = call.object(0);
    if object.is_null() {
        return Err(report::script_error(command, b"Unit doesn't exists"));
    }
    // SAFETY: a bound object argument holds a live object.
    if unsafe { EngineObject::class_id(object) } != CLASS_UNIT {
        // SAFETY: as above.
        let id = unsafe { (*object).id };
        return Err(report::script_error(
            command,
            format!("Object with ID {id} must be a Unit").as_bytes(),
        ));
    }
    Ok(object.cast())
}

/// `GSSetPlayerVar(player, name, value)`: the player's variable, through its server half.
pub(super) fn gs_set_player_var(call: &Call) -> bool {
    let value = call.float(1);
    let (number, player) = player(call, 0);
    if player.is_null() {
        return no_player("GSSetPlayerVar", number);
    }
    // SAFETY: a live player of the server world.
    let server = unsafe { (*player).server };
    // The original calls a missing server half (a client's player) and fails; nothing happens here.
    if !server.is_null() {
        // SAFETY: the player's live server half.
        unsafe { PlayerServer::set_variable(server, call.string(0), value) };
    }
    true
}

/// `GSGetPlayerVar(player, name)`: the player's variable; the result is left alone without the player.
pub(super) fn gs_get_player_var(call: &Call) -> bool {
    let (number, player) = player(call, 0);
    if player.is_null() {
        return no_player("GSGetPlayerVar", number);
    }
    // SAFETY: a field address of the live player; nothing is read.
    let variables = unsafe { &raw mut (*player).variables };
    // SAFETY: the live player's variable table.
    let value = unsafe { NamedFloatTable::lookup(variables, call.string(0)) };
    call.set_float(value);
    true
}

/// `GiveExp(player, value)`: the world's experience award (no check for the player: null is the network path).
pub(super) fn give_exp(call: &Call) -> bool {
    let (_, player) = player(call, 0);
    // SAFETY: the server world; `player` is null or live.
    unsafe { World::award_experience(SERVER_WORLD.as_ptr(), player, call.float(1)) };
    true
}

/// `GiveUnitExp(unit, value)`: the unit gains the experience; a unit with a player's server half also sends it.
pub(super) fn give_unit_exp(call: &Call) -> bool {
    let object = call.object(0);
    // SAFETY: a bound object argument holds a live object or null.
    if object.is_null() || unsafe { EngineObject::class_id(object) } != CLASS_UNIT {
        return invalid_arguments("GiveUnitExp");
    }
    let value = call.float(0);
    // SAFETY: a unit, as just checked.
    let unit = unsafe { &*object.cast::<Unit>() };
    let data = unit.controller.cast::<UnitServer>();
    if data.is_null() {
        return invalid_arguments("GiveUnitExp");
    }
    // SAFETY: a unit's live controller.
    unsafe { UnitServer::add_experience(data, value) };
    let player = unit.player.cast::<Player>();
    if !player.is_null() {
        // SAFETY: a unit's player is live.
        let server = unsafe { (*player).server };
        if !server.is_null() {
            // SAFETY: the player's live server half.
            unsafe {
                PlayerServer::send_unit_experience(server, unit.map_object.transform.base.id, value)
            };
        }
    }
    true
}

pub(super) fn start_camera_save(call: &Call) -> bool {
    // SAFETY: script commands run on the game's thread.
    unsafe { flow::start_camera_recording(call.string(0)) };
    true
}

pub(super) fn stop_camera_save(_: &Call) -> bool {
    // SAFETY: as above.
    unsafe { flow::stop_camera_recording() };
    true
}

/// `ResetTarget(unit)` (replaces the game's): the brain's perception is emptied, then its alarm record applied again.
pub(super) fn reset_target(call: &Call) -> bool {
    let object = call.object(0);
    // SAFETY: a bound object argument holds a live object or null.
    if object.is_null() || unsafe { EngineObject::class_id(object) } != CLASS_UNIT {
        return invalid_arguments("ResetTarget");
    }
    // SAFETY: a unit, as just checked.
    let unit = unsafe { &*object.cast::<Unit>() };
    let (id, data) = (
        unit.map_object.transform.base.id,
        unit.controller.cast::<UnitServer>(),
    );
    // The original goes on without a controller (a client's unit) and fails; such a call is refused here.
    if data.is_null() {
        return invalid_arguments("ResetTarget");
    }
    // SAFETY: the controller's brain.
    let brain = unsafe { &raw mut (*data).brain };
    // SAFETY: as above.
    let index = i32::from(unsafe { (*brain).alarm_slot });
    // SAFETY: as above; nothing is read.
    let memory = unsafe { &raw mut (*brain).memory };
    // SAFETY: the brain's live memory.
    unsafe { eix_engine::game::unit::AiMemory::reset(memory) };
    // SAFETY: as above.
    let records = unsafe { &(*brain).alarm_records };
    // A negative slot is refused as well: the original read a record before the array.
    let Some(record) = usize::try_from(index)
        .ok()
        .filter(|&i| i32::try_from(i).is_ok_and(|i| i < records.len))
        .map(|i| records.data.wrapping_add(i))
    else {
        return report::script_error(
            "ResetTarget",
            format!("Failed to reset unit {id}").as_bytes(),
        );
    };
    // SAFETY: the brain's record `index`, within its array.
    unsafe { eix_engine::game::unit::AiBrain::apply_behaviour(brain, record, index) };
    true
}

/// `SendCurrentParty(player)`: the player's party is sent again.
pub(super) fn send_current_party(call: &Call) -> bool {
    let (number, player) = player(call, 0);
    if player.is_null() {
        return no_player("SendCurrentParty", number);
    }
    // SAFETY: a live player of the server world.
    let server = unsafe { (*player).server };
    // As in GSSetPlayerVar: a missing server half does nothing here.
    if !server.is_null() {
        // SAFETY: the player's live server half.
        unsafe { PlayerServer::resend_party(server) };
    }
    true
}

pub(super) fn player_exists(call: &Call) -> bool {
    let (_, player) = player(call, 0);
    call.set_float(if player.is_null() { 0.0 } else { 1.0 });
    true
}

/// `Loot(player, unit)`: the player loots the dead unit, which is then removed.
pub(super) fn loot(call: &Call) -> bool {
    let (number, player) = player(call, 0);
    if player.is_null() {
        return no_player("Loot", number);
    }
    let object = call.object(0);
    if object.is_null() {
        return report::script_error("Loot", b"Object doesn't exist");
    }
    // SAFETY: a bound object argument holds a live object.
    if unsafe { EngineObject::class_id(object) } != CLASS_UNIT {
        return report::script_error("Loot", b"Object must be a unit");
    }
    // SAFETY: a unit, as just checked.
    if unsafe { EngineObject::is_dead(object) } == 0 {
        return report::script_error("Loot", b"Object must be dead");
    }
    // SAFETY: as above.
    let id = unsafe { (*object).id };
    // SAFETY: the server world, the dead unit and the player, all live.
    unsafe { World::loot(SERVER_WORLD.as_ptr(), object.cast(), player) };
    // SAFETY: as above.
    unsafe { World::remove_object(SERVER_WORLD.as_ptr(), id) };
    true
}

/// Player 0's server half, or null.
fn first_player_server() -> *mut PlayerServer {
    // SAFETY: on the game's thread; the server world is constructed.
    let player = unsafe { World::get_player(SERVER_WORLD.as_ptr(), 0) };
    if player.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: a live player.
    unsafe { (*player).server }
}

/// `GameOver`: the `gameover` string event to player 0's client.
pub(super) fn game_over(_: &Call) -> bool {
    let server = first_player_server();
    if !server.is_null() {
        // SAFETY: a live player's server half.
        unsafe { PlayerServer::send_string_event(server, c"gameover") };
    }
    true
}

/// `GameOverMSG(title, text)`: `gameover <title> <text>` to player 0's client.
pub(super) fn game_over_msg(call: &Call) -> bool {
    let server = first_player_server();
    if !server.is_null() {
        let text = [
            b"gameover ",
            call.string(0).to_bytes(),
            b" ",
            call.string(1).to_bytes(),
        ]
        .concat();
        if let Ok(text) = CString::new(text) {
            // SAFETY: as above.
            unsafe { PlayerServer::send_string_event(server, &text) };
        }
    }
    true
}

/// `IsFileExists(path)`: a file (not a folder) under the mod folder, `\` read as `/`; a climbing path is refused and
/// the result left alone.
pub(super) fn is_file_exists(call: &Call) -> bool {
    let name = call.string(0).to_bytes();
    if script::path_climbs(name) {
        return report::script_error("IsFileExists", b"'..' string literal is forbidden");
    }
    let relative: Vec<u8> = name
        .iter()
        .map(|&c| if c == b'\\' { b'/' } else { c })
        .collect();
    let base = MOD_DIR.get().map(Vec::as_slice).unwrap_or_default();
    let path = [base, &relative].concat();
    call.set_float(if file_exists(&from_ansi(&path)) {
        1.0
    } else {
        0.0
    });
    true
}

/// `GSListPlayerVars(player)`: `<name> = <value>` on the console for each of the player's variables, in the table's
/// order. The original lists without checking the player and fails on a missing one; nothing is listed then here.
pub(super) fn gs_list_player_vars(call: &Call) -> bool {
    let (_, player) = player(call, 0);
    if player.is_null() {
        return true;
    }
    // SAFETY: a field address of the live player; nothing is read.
    let variables = unsafe { &raw const (*player).variables };
    let print = |name: &CStr, value: f32| {
        let args = [
            FormatArg::Str(name.as_ptr().cast()),
            FormatArg::Double(f64::from(value)),
        ];
        // SAFETY: on the game's thread; the format reads the name and the double given.
        let line = unsafe { mfc::format_bytes(c"%s = %f", &args) };
        if let Ok(line) = CString::new(line) {
            // SAFETY: on the game's thread, with the console built.
            unsafe { console::print_line(&line) };
        }
    };
    // SAFETY: the live player's variable table; printing does not change it.
    unsafe { NamedFloatTable::for_each_variable(variables, print) };
    true
}

/// The override argument: exactly 1.
fn override_stats(call: &Call) -> bool {
    call.float(0) == 1.0
}

/// `GiveUnitWeapon(unit, item, override)`: the item made from its name goes to the unit; with override 1 (only for
/// units without a text-key id) the unit's natural attack is set from the weapon.
pub(super) fn give_unit_weapon(call: &Call) -> bool {
    let mut stats_from_item = override_stats(call);
    let unit = match unit_argument(call, "GiveUnitWeapon") {
        Ok(u) => u,
        Err(done) => return done,
    };
    let name = call.string(0);
    // SAFETY: the server world, in a zone; `name` is NUL-terminated.
    let item = unsafe { World::create_item(SERVER_WORLD.as_ptr(), name) };
    // SAFETY: a live unit.
    if stats_from_item && unsafe { (*unit).map_object.transform.base.id } >= TEXT_KEY {
        report::script_warning(
            b"GiveUnitWeapon: third param (override stats) can be applyed only on non-heroes units. Ignored",
        );
        stats_from_item = false;
    }
    if item.is_null() {
        return cant_create("GiveUnitWeapon", name);
    }
    // SAFETY: a live unit and a new item.
    let slot = unsafe { Unit::add_weapon(unit, item) };
    if slot == -1 {
        return true;
    }
    // SAFETY: a live unit.
    unsafe { (*unit).pending_weapon = slot };
    // SAFETY: a live unit.
    let data = unsafe { (*unit).controller }.cast::<UnitServer>();
    // Without a controller (a client's unit) the original fails on it; nothing more happens here.
    if data.is_null() {
        return true;
    }
    if stats_from_item {
        // SAFETY: a live unit with its controller, and the new weapon.
        unsafe { set_attack_from(unit, item) };
    }
    // SAFETY: the unit's live controller.
    unsafe { UnitServer::refresh_model(data) };
    // SAFETY: as above.
    unsafe { UnitServer::refresh_equipment(data) };
    true
}

/// `<command>: Can't create item "<name>"`.
fn cant_create(command: &str, name: &CStr) -> bool {
    report::script_error(
        command,
        &[b"Can't create item \"", name.to_bytes(), b"\""].concat(),
    )
}

/// The unit's natural attack from `item`, a weapon, and its unit prototype.
///
/// # Safety
/// `unit` must be a live unit with its controller and `item` a live weapon, on the game's thread.
unsafe fn set_attack_from(unit: *mut Unit, item: *mut Item) {
    // SAFETY: the caller guarantees the weapon.
    let record = unsafe { Item::prototype(item) }.cast::<WeaponRecord>();
    // SAFETY: a weapon's record is the weapon database's.
    let record = unsafe { &*record };
    // SAFETY: the caller guarantees the unit.
    let prototype = unsafe { Unit::prototype(unit) };
    // SAFETY: its prototype record is the unit database's.
    let prototype = unsafe { &*prototype };
    // SAFETY: as above.
    let weapon = unsafe { &*item.cast::<ItemWeapon>() };
    // SAFETY: as above.
    let natural_weapon = unsafe { Item::weapon_type(item) };
    // SAFETY: the caller guarantees the unit.
    let data = unsafe { (*unit).controller }.cast::<UnitServer>();
    // SAFETY: the unit's live controller.
    let stats = unsafe { &mut (*data).stats };
    stats.damage = weapon.damage;
    stats.damage_spread = weapon.damage_spread;
    stats.combat[3] = prototype.combat[3] + record.defence_bonus;
    stats.combat[0] = record.reach;
    stats.combat[1] = prototype.combat[1] + record.attack_bonus;
    stats.natural_attack_interval = record.attack_interval as f32;
    stats.natural_weapon = natural_weapon;
    stats.damage_weights = record.damage_types;
    // SAFETY: the caller guarantees the unit, a map object.
    unsafe { MapObject::mark_changed(unit.cast(), 0) };
}

/// The weights of the four protection blocks GiveUnitArmor sums.
const ARMOUR_WEIGHTS: [f32; 4] = [0.1, 0.3, 0.3, 0.3];

/// `GiveUnitArmor(unit, item, override)`: the armour piece made from its name goes on; with override 1 (only for
/// units without a text-key id) the unit's protection is set from the pieces it wears.
pub(super) fn give_unit_armor(call: &Call) -> bool {
    let mut stats_from_items = override_stats(call);
    let unit = match unit_argument(call, "GiveUnitArmor") {
        Ok(u) => u,
        Err(done) => return done,
    };
    let name = call.string(0);
    // SAFETY: the server world, in a zone; `name` is NUL-terminated.
    let item = unsafe { World::create_item(SERVER_WORLD.as_ptr(), name) };
    // SAFETY: a live unit.
    if stats_from_items && unsafe { (*unit).map_object.transform.base.id } >= TEXT_KEY {
        report::script_warning(
            b"GiveUnitArmor: third param (override stats) can be applyed only on non-heroes units. Ignored",
        );
        stats_from_items = false;
    }
    if item.is_null() {
        return cant_create("GiveUnitArmor", name);
    }
    // SAFETY: a live unit and a new item.
    unsafe { Unit::equip_armour(unit, item) };
    // SAFETY: a live unit.
    let u = unsafe { &*unit };
    let (data, companion) = (u.controller.cast::<UnitServer>(), u.map_object.companion);
    // Without a server half the original fails on it; nothing more happens here.
    if data.is_null() || companion.is_null() {
        return true;
    }
    if stats_from_items {
        // SAFETY: a live unit with its server half.
        unsafe { set_protection_from_armour(unit) };
    }
    // SAFETY: the unit's live controller.
    unsafe { UnitServer::refresh_equipment(data) };
    true
}

/// The four blocks of seven values the worn armour adds up to: the head (slot 0) in block 0; slots 1 and 3 in block
/// 1, with their second blocks in block 2; slot 6 in block 2; slots 2, 4 and 5 in block 3.
fn armour_blocks(pieces: &[Option<([f32; 7], [f32; 7])>; 7]) -> [[f32; 7]; 4] {
    let mut blocks = [[0.0f32; 7]; 4];
    let add = |block: &mut [f32; 7], from: &[f32; 7]| {
        for (b, f) in block.iter_mut().zip(from) {
            *b += *f;
        }
    };
    for (slot, piece) in pieces.iter().enumerate() {
        let Some((first, second)) = piece else {
            continue;
        };
        match slot {
            0 => add(&mut blocks[0], first),
            1 | 3 => {
                add(&mut blocks[1], first);
                add(&mut blocks[2], second);
            }
            6 => add(&mut blocks[2], first),
            _ => add(&mut blocks[3], first),
        }
    }
    blocks
}

/// Each protection value: the weighted blocks summed, plus the prototype's scale.
fn protection(blocks: &[[f32; 7]; 4], scale: f32) -> [f32; 7] {
    core::array::from_fn(|c| {
        let sum = blocks
            .iter()
            .zip(ARMOUR_WEIGHTS)
            .fold(0.0f32, |s, (block, w)| w * block[c] + s);
        scale + sum
    })
}

/// # Safety
/// `unit` must be a live unit whose companion is its server half, on the game's thread.
unsafe fn set_protection_from_armour(unit: *mut Unit) {
    // SAFETY: the caller guarantees the unit; its armour slots hold live armour pieces or null.
    let pieces: [Option<([f32; 7], [f32; 7])>; 7] = core::array::from_fn(|slot| {
        // SAFETY: as above.
        let piece = unsafe { (*unit).armor[slot] }.cast::<ItemArmor>();
        (!piece.is_null()).then(|| {
            // SAFETY: a worn piece is an armour item.
            let p = unsafe { &*piece };
            (p.absorb, p.absorb2)
        })
    });
    // SAFETY: the caller guarantees the unit.
    let prototype = unsafe { Unit::prototype(unit) };
    // SAFETY: the unit's prototype record of the unit database.
    let scale = unsafe { (*prototype).protection_scale };
    let values = protection(&armour_blocks(&pieces), scale);
    // SAFETY: the caller guarantees the unit.
    let companion = unsafe { (*unit).map_object.companion }.cast::<MapObjectServer>();
    // SAFETY: the unit's companion is its server half (as the original takes it).
    let vitals = unsafe { &mut (*companion).vitals };
    for (pair, value) in vitals.protection.iter_mut().zip(values) {
        pair.base = value;
        pair.current = value;
    }
    // SAFETY: the caller guarantees the unit, a map object.
    unsafe { MapObject::mark_changed(unit.cast(), 0) };
}

/// `ShowAllUnits(0)` sets the draw-all flag, anything else clears it.
pub(super) fn show_all_units(call: &Call) -> bool {
    // SAFETY: on the game's thread; the flag is a plain word.
    unsafe { DRAW_ALL.write(u32::from(call.float(0) == 0.0)) };
    true
}

/// `PauseGame(value)`: outside network games, the client's pause request with (value == 0).
pub(super) fn pause_game(call: &Call) -> bool {
    // SAFETY: on the game's thread.
    if !unsafe { net::is_multiplayer_client() } {
        // SAFETY: the client world, constructed with its session.
        unsafe { World::send_pause(CLIENT_WORLD.as_ptr(), u32::from(call.float(0) == 0.0)) };
    }
    true
}

/// `SetWeather("rain" | "clear" | "snow")`.
pub(super) fn set_weather(call: &Call) -> bool {
    let (kind, strength) = script::weather(call.string(0).to_bytes());
    // SAFETY: the server world, on the game's thread.
    unsafe { World::set_weather(SERVER_WORLD.as_ptr(), kind, strength) };
    true
}

/// `AddMobEx(file, [skip scripts])`: loads a .mob file's objects with the loading movie off meanwhile.
pub(super) fn add_mob_ex(call: &Call) -> bool {
    let file = call.string(0);
    let skip_scripts = !call.floats().is_empty() && call.float(0) == 1.0;
    // SAFETY: on the game's thread; the flag is a plain byte the movie reads.
    unsafe { LOADING_MOVIE_OFF.write(1) };
    // SAFETY: the server world, in a zone.
    let loaded = unsafe { World::load_map_objects(SERVER_WORLD.as_ptr(), file, skip_scripts) };
    // SAFETY: as above.
    unsafe { LOADING_MOVIE_OFF.write(0) };
    if loaded == 0 {
        return report::script_error(
            "AddMobEx",
            &[b"Can't load .mob file \"", file.to_bytes(), b"\""].concat(),
        );
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armour_adds_up_by_slot_and_block() {
        let one = [1.0f32; 7];
        let two = [2.0f32; 7];
        let mut pieces = [None; 7];
        pieces[0] = Some((one, two));
        pieces[1] = Some((one, two));
        pieces[6] = Some((one, two));
        pieces[4] = Some((two, one));
        let blocks = armour_blocks(&pieces);
        assert_eq!(blocks[0], one);
        assert_eq!(blocks[1], one);
        assert_eq!(blocks[2], [3.0; 7]);
        assert_eq!(blocks[3], two);
        let values = protection(&blocks, 5.0);
        // 0.1 * 1 + 0.3 * 1 + 0.3 * 3 + 0.3 * 2, each step rounded to float, plus 5.
        let sum = 0.3f32 * 2.0 + (0.3f32 * 3.0 + (0.3f32 * 1.0 + (0.1f32 * 1.0 + 0.0)));
        assert_eq!(values, [5.0 + sum; 7]);
    }
}
