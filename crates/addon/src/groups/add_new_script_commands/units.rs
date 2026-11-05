//! The functions about units, objects and players. A unit's
//! data is its server controller (the unit's +0x240).
#![expect(unsafe_code, reason = "reading the game's units and objects")]

use eix_core::script::{self, ExpKind, Skill};
use eix_engine::float::ftol;
use eix_engine::game::flow;
use eix_engine::game::mfc::PtrList;
use eix_engine::game::object::{CLASS_UNIT, EngineObject, TransformObject, Vector3};
use eix_engine::game::script::{self as game_script, ScriptObjectRef};
use eix_engine::game::unit::{Unit, UnitServer, UnitStats};
use eix_engine::game::world::{OBJECT_MAP, ObjectMap, SECTOR_MAP, SectorMap};

use super::{Call, invalid_arguments};
use crate::report;

/// Ids from here on are text keys: the player's and the named units'.
const TEXT_KEY: i32 = 1_000_000_000;

/// Whether `object` is a unit.
///
/// # Safety
/// `object` must be a live engine object, on the game's thread.
unsafe fn is_unit(object: *mut EngineObject) -> bool {
    // SAFETY: passed on.
    unsafe { EngineObject::class_id(object) == CLASS_UNIT }
}

/// The unit's data (its server controller), null for a missing reference, object or non-unit.
fn unit_data(call: &Call) -> *mut UnitServer {
    let object = call.object(0);
    // SAFETY: a bound object argument holds a live object or null.
    if object.is_null() || !unsafe { is_unit(object) } {
        return core::ptr::null_mut();
    }
    // SAFETY: a unit, as just checked.
    unsafe { (*object.cast::<Unit>()).controller.cast() }
}

/// A statistic of the unit's data, 0 and invalid arguments without one.
fn stat(call: &Call, command: &str, read: fn(&UnitStats) -> f32) -> bool {
    call.set_float(0.0);
    let data = unit_data(call);
    if data.is_null() {
        return invalid_arguments(command);
    }
    // SAFETY: a unit's live controller.
    call.set_float(read(unsafe { &(*data).stats }));
    true
}

/// The unit's player's number; -1 without one.
pub(super) fn get_player_of_unit(call: &Call) -> bool {
    call.set_float(-1.0);
    let object = call.object(0);
    if object.is_null() {
        return report::script_error("GetPlayerOfUnit", b"Unit doesn't exists");
    }
    // SAFETY: a bound object argument holds a live object.
    let id = unsafe { (*object).id };
    // SAFETY: as above.
    if !unsafe { is_unit(object) } {
        return report::script_error(
            "GetPlayerOfUnit",
            format!("Object with ID {id} must be a Unit").as_bytes(),
        );
    }
    // SAFETY: a unit, as just checked.
    let player = unsafe { (*object.cast::<Unit>()).player };
    if player.is_null() {
        return report::script_error(
            "GetPlayerOfUnit",
            format!("Unit doesn't have associated player: ID = {id}").as_bytes(),
        );
    }
    // SAFETY: a unit's player is a live player.
    let number = unsafe { (*player.cast::<eix_engine::game::player::Player>()).number };
    call.set_float(number as f32);
    true
}

/// `GetUnitExp(unit, [what])`: `max` (the default), `spent`, `penalty` or `real` (penalty + experience).
pub(super) fn get_unit_exp(call: &Call) -> bool {
    let data = unit_data(call);
    let what: &[u8] = if call.strings().is_empty() {
        b"max"
    } else {
        call.string(0).to_bytes()
    };
    call.set_float(0.0);
    if data.is_null() {
        return invalid_arguments("GetUnitExp");
    }
    // SAFETY: a unit's live controller.
    let stats = unsafe { &(*data).stats };
    let value = match script::exp_kind(what) {
        Some(ExpKind::Max) => stats.experience,
        Some(ExpKind::Spent) => stats.spent_experience,
        Some(ExpKind::Penalty) => stats.pending_experience,
        Some(ExpKind::Real) => stats.pending_experience + stats.experience,
        None => return invalid_arguments("GetUnitExp"),
    };
    call.set_float(value);
    true
}

/// `GetArmor(unit, kind, part)`: part 0..6; kind 0..5 sums body slot `kind`'s three layers for the damage type
/// `part`; a negative kind takes `part` from the protection the unit's parameters compute.
pub(super) fn get_armor(call: &Call) -> bool {
    let data = unit_data(call);
    let kind = ftol(f64::from(call.float(0)));
    let part = ftol(f64::from(call.float(1)));
    call.set_float(0.0);
    let Ok(part) = usize::try_from(part) else {
        return invalid_arguments("GetArmor");
    };
    if part > 6 || kind > 5 || data.is_null() {
        return invalid_arguments("GetArmor");
    }
    if let Ok(slot) = usize::try_from(kind) {
        // SAFETY: a unit's live controller.
        let stats = unsafe { &(*data).stats };
        let sum = stats.slots[slot]
            .layers
            .iter()
            .fold(0.0f32, |s, layer| layer.absorb[part] + s);
        call.set_float(sum);
    } else {
        // SAFETY: as above; the controller's unit is live.
        let unit = unsafe { (*data).unit };
        // SAFETY: as above.
        let text_key = unsafe { (*unit).map_object.transform.base.id } >= TEXT_KEY;
        // SAFETY: as above.
        let block = unsafe { Unit::protection_block(unit, text_key) };
        call.set_float(block[part]);
    }
    true
}

/// The filter of NearestUnit and UnitsNearPoint: units pass with bit 0,
/// other objects with bit 1, and with bit 2 any living object with a text-key id.
#[derive(Debug, Clone, Copy)]
struct Filter {
    units: bool,
    others: bool,
    living: bool,
}

impl Filter {
    fn from(value: f32) -> Self {
        // The original keeps the low 32 bits where `ftol` saturates; they differ only from 2^31, where every f32 is a
        // multiple of 256, so bits 0-2 are 0 either way.
        let bits = ftol(f64::from(value));
        Self {
            units: bits & 1 != 0,
            others: bits >> 1 & 1 != 0,
            living: bits >> 2 & 1 != 0,
        }
    }

    /// # Safety
    /// `object` must be a live map object, on the game's thread.
    unsafe fn passes(self, object: *mut EngineObject) -> bool {
        // SAFETY: passed on.
        let unit = unsafe { is_unit(object) };
        // SAFETY: as above.
        let text_key = unsafe { (*object).id } >= TEXT_KEY;
        // SAFETY: as above; the dead test is asked only of text-key objects, as in the original.
        let alive = text_key && unsafe { EngineObject::is_dead(object) } == 0;
        (if unit { self.units } else { self.others }) || (alive && self.living)
    }
}

/// The objects the sector map finds within `radius` of (x, y), in its order.
///
/// # Safety
/// On the game's thread, with a zone loaded.
unsafe fn objects_near(x: f32, y: f32, radius: f32) -> Vec<*mut EngineObject> {
    let mut list = PtrList::new();
    let point = Vector3 { x, y, z: 0.0 };
    // SAFETY: the server world's sector map; `list` is a constructed list.
    unsafe { SectorMap::find_in_radius(SECTOR_MAP.as_ptr(), &point, &raw mut list, radius) };
    // SAFETY: the list's nodes are the game's, unchanged until it is destroyed.
    let objects = unsafe { list.elements() }
        .into_iter()
        .map(|o| o.cast())
        .collect();
    // SAFETY: the list is not used afterwards.
    unsafe { PtrList::destroy(&raw mut list) };
    objects
}

/// `NearestUnit(x, y, radius, filter)`: the nearest passing object found within the radius, strictly nearer each
/// time (a NaN distance never wins).
pub(super) fn nearest_unit(call: &Call) -> bool {
    let (x, y) = (call.float(0), call.float(1));
    let mut radius = call.float(2);
    let filter = Filter::from(call.float(3));
    let mut best = core::ptr::null_mut();
    // SAFETY: script functions run on the game's thread in a zone.
    for object in unsafe { objects_near(x, y, radius) } {
        // SAFETY: the sector map holds live map objects.
        if !unsafe { filter.passes(object) } {
            continue;
        }
        // SAFETY: a map object is a transform object.
        let at = unsafe { (*object.cast::<TransformObject>()).position };
        let distance = script::distance(at.x, at.y, x, y);
        if radius > distance {
            best = object;
            radius = distance;
        }
    }
    // SAFETY: CalcObject's live temporary; `best` is null or live.
    unsafe { ScriptObjectRef::assign(call.result_ref(), best) };
    true
}

/// `UnitsNearPoint(x, y, radius, filter)`: every passing object found within the radius.
pub(super) fn units_near_point(call: &Call) -> bool {
    let (x, y, radius) = (call.float(0), call.float(1), call.float(2));
    let filter = Filter::from(call.float(3));
    let group = call.result_group();
    // SAFETY: CalcGroup's live temporary group.
    unsafe { game_script::group_clear(group) };
    // SAFETY: script functions run on the game's thread in a zone.
    for object in unsafe { objects_near(x, y, radius) } {
        // SAFETY: the sector map holds live map objects.
        if unsafe { filter.passes(object) } {
            // SAFETY: as above.
            unsafe { game_script::group_insert(group, object) };
        }
    }
    true
}

/// A member of the group, by the generator's next value modulo its size; an empty group leaves the result.
pub(super) fn rand_obj(call: &Call) -> bool {
    let group = call.group(0);
    if group.is_null() {
        return true;
    }
    // SAFETY: a bound group argument is a live group.
    let group = unsafe { &*group };
    // SAFETY: as above.
    let members = unsafe { group.as_slice() };
    let Ok(count) = u32::try_from(members.len()) else {
        return true;
    };
    if count != 0 {
        // SAFETY: on the game's thread, with the generator constructed.
        let index = unsafe { flow::random_next() } % count;
        if let Some(member) = members.get(index as usize) {
            // SAFETY: CalcObject's live temporary; the member holds a live object or null.
            unsafe { ScriptObjectRef::assign(call.result_ref(), member.object) };
        }
    }
    true
}

pub(super) fn get_dexterity(call: &Call) -> bool {
    stat(call, "GetDexterity", |s| s.dexterity)
}

pub(super) fn get_intelligence(call: &Call) -> bool {
    stat(call, "GetIntelligence", |s| s.intelligence)
}

pub(super) fn get_strength(call: &Call) -> bool {
    stat(call, "GetStrength", |s| s.strength)
}

pub(super) fn get_actions(call: &Call) -> bool {
    stat(call, "GetActions", |s| s.rate)
}

pub(super) fn get_actions_base(call: &Call) -> bool {
    stat(call, "GetActionsBase", |s| s.rate_base)
}

pub(super) fn get_encumbrance(call: &Call) -> bool {
    stat(call, "GetEncumbrance", |s| s.load)
}

pub(super) fn get_max_encumbrance(call: &Call) -> bool {
    stat(call, "GetMaxEncumbrance", |s| s.capacity)
}

/// `GetSkill(unit, name)`: an attribute or skill byte by name.
pub(super) fn get_skill(call: &Call) -> bool {
    let data = unit_data(call);
    let name = call.string(0).to_bytes();
    call.set_float(0.0);
    if data.is_null() {
        return invalid_arguments("GetSkill");
    }
    // SAFETY: a unit's live controller.
    let stats = unsafe { &(*data).stats };
    let value = match script::skill(name) {
        Some(Skill::Attribute(i)) => stats.attributes[i],
        Some(Skill::Skill(i)) => stats.skills[i],
        None => return invalid_arguments("GetSkill"),
    };
    call.set_float(f32::from(value));
    true
}

/// `GetPerk(unit, name)`: a perk's level byte by name.
pub(super) fn get_perk(call: &Call) -> bool {
    let data = unit_data(call);
    let name = call.string(0).to_bytes();
    call.set_float(0.0);
    if data.is_null() {
        return invalid_arguments("GetPerk");
    }
    let Some(i) = script::perk(name) else {
        return invalid_arguments("GetPerk");
    };
    // SAFETY: a unit's live controller.
    call.set_float(f32::from(unsafe { (*data).stats.skill_levels[i] }));
    true
}

/// The unit's prototype record's name (`model` false) or model name.
fn prototype_text(call: &Call, command: &str, model: bool) -> bool {
    let data = unit_data(call);
    if data.is_null() {
        return invalid_arguments(command);
    }
    // SAFETY: a unit's live controller.
    let unit = unsafe { (*data).unit };
    // SAFETY: the controller's live unit.
    let prototype = unsafe { Unit::prototype(unit) };
    // The original reads the record without checking it; a unit without one gets an empty text here.
    if prototype.is_null() {
        call.set_text(b"");
        return true;
    }
    // SAFETY: the unit database's live record.
    let record = unsafe { &*prototype };
    let text = if model { &record.model } else { &record.name };
    // SAFETY: its texts are constructed game strings.
    call.set_text(unsafe { text.as_bytes() });
    true
}

pub(super) fn get_prototype(call: &Call) -> bool {
    prototype_text(call, "GetPrototype", false)
}

pub(super) fn get_race(call: &Call) -> bool {
    prototype_text(call, "GetRace", true)
}

/// The object's class id, -1 without an object.
pub(super) fn get_object_type_id(call: &Call) -> bool {
    call.set_float(-1.0);
    let object = call.object(0);
    if !object.is_null() {
        // SAFETY: a bound object argument holds a live object.
        let class = unsafe { EngineObject::class_id(object) } as i32;
        call.set_float(class as f32);
    }
    true
}

/// `GetObjectsOnMap(-1)`: every object of the server world's object map; `GetObjectsOnMap(class, ...)`: those
/// whose class id is one of the (truncated) arguments; in the map's bucket order.
pub(super) fn get_objects_on_map(call: &Call) -> bool {
    let group = call.result_group();
    // SAFETY: CalcGroup's live temporary group.
    unsafe { game_script::group_clear(group) };
    let classes = call.floats();
    let all = classes.len() == 1 && classes[0] == -1.0;
    // SAFETY: the server world's object map, unchanged while walked (inserting into a script group changes it not).
    for node in unsafe { ObjectMap::nodes(OBJECT_MAP.as_ptr()) } {
        // SAFETY: a live node of the map.
        let object = unsafe { (*node).object }.cast::<EngineObject>();
        if object.is_null() {
            continue;
        }
        let take = all || {
            // SAFETY: the map holds live objects.
            let class = unsafe { EngineObject::class_id(object) } as i32;
            classes.iter().any(|&c| ftol(f64::from(c)) == class)
        };
        if take {
            // SAFETY: as above.
            unsafe { game_script::group_insert(group, object) };
        }
    }
    true
}

/// A unit with a text-key id and a player.
pub(super) fn is_player(call: &Call) -> bool {
    call.set_float(0.0);
    let object = call.object(0);
    // SAFETY: a bound object argument holds a live object or null.
    if !object.is_null() && unsafe { is_unit(object) } {
        // SAFETY: a unit, as just checked.
        let unit = unsafe { &*object.cast::<Unit>() };
        if unit.map_object.transform.base.id >= TEXT_KEY && !unit.player.is_null() {
            call.set_float(1.0);
        }
    }
    true
}

/// The unit's dead flag compared with `want`, 0 for a missing object or a non-unit.
fn dead_flag(call: &Call, want: i32) -> bool {
    call.set_float(0.0);
    let object = call.object(0);
    // SAFETY: a bound object argument holds a live object or null.
    if !object.is_null() && unsafe { is_unit(object) } {
        // SAFETY: a unit, as just checked.
        let dead = unsafe { EngineObject::is_dead(object) };
        call.set_float(if dead == want { 1.0 } else { 0.0 });
    }
    true
}

/// `IsDead` (replaces the game's): 1 when the unit's dead flag is 1.
pub(super) fn is_dead(call: &Call) -> bool {
    dead_flag(call, 1)
}

/// `IsAlive` (replaces the game's): 1 when the unit's dead flag is 0.
pub(super) fn is_alive(call: &Call) -> bool {
    dead_flag(call, 0)
}
