//! The addon's Lua: the one state, created at load whatever the config says, the mod's main
//! script run from the config read, and the game access behind the registered functions.
#![expect(
    unsafe_code,
    reason = "the registered functions read and call game.exe's units, tables and console for scripts"
)]

use std::cell::Cell;
use std::ffi::{CStr, CString};
use std::rc::Rc;

use eix_core::lua_callbacks::Defined;
use eix_engine::game::console;
use eix_engine::game::database::{self, SKILL_DATABASE, SkillDatabase};
use eix_engine::game::item::Item;
use eix_engine::game::object::{CLASS_UNIT_CLIENT_SPECIFIC, EngineObject};
use eix_engine::game::player::{ItemList, NamedFloatTable, Player};
use eix_engine::game::unit::{MapObject, Unit, UnitStats, UnitVitals};
use eix_engine::game::world::SERVER_WORLD;
use eix_engine::win::{from_ansi, narrow};
use eix_lua::{FatalReport, Game, Host, SlotKind, Table, TableBounds, error_box_and_exit};

use crate::log;
use crate::setup::Setup;

/// The title of the fatal Lua error box, as the addon's other boxes.
const ERROR_TITLE: &str = "addon.dll";

thread_local! {
    /// The state, on the game's thread. It is never closed: the original closes it at process exit, and a fatal
    /// error box exits from inside a Lua call, where closing would free a state still in use.
    static HOST: Cell<Option<&'static Host>> = const { Cell::new(None) };
}

/// The Lua host, on the thread that created it (the game's).
pub(crate) fn host() -> Option<&'static Host> {
    HOST.with(Cell::get)
}

/// Creates the state with the 57 functions, as addon.dll does at load.
pub(crate) fn create() {
    let show = error_box_and_exit(ERROR_TITLE.to_owned());
    // Logged before the box ends the game, so the message and its traceback survive in a bug report; one line per
    // line of the message.
    let report: FatalReport = Box::new(move |message: &[u8]| {
        let text = narrow(&from_ansi(message));
        for line in text.lines() {
            log::error("lua", line, "fatal error");
        }
        show(message);
    });
    match Host::new(Rc::new(GameAccess), report) {
        Ok(h) => {
            HOST.with(|c| c.set(Some(Box::leak(Box::new(h)))));
            log::info("lua", "Lua state created", "");
        }
        Err(e) => log::error("lua", &format!("cannot create the Lua state: {e}"), ""),
    }
}

/// Runs the mod's main script when `[Addon] EnableLuaScripts` is on, as the original's config read does.
pub(crate) fn run_main_script(setup: &Setup) {
    let addon = &setup.options.ai.addon;
    if addon.enable_lua_scripts == 0 {
        return;
    }
    let Some(host) = host() else {
        log::error(
            "lua",
            "EnableLuaScripts is on but there is no Lua state",
            "",
        );
        return;
    };
    let script = String::from_utf8_lossy(&addon.lua_main_script_path).into_owned();
    log::info("lua", "running the main script", &script);
    host.run_main_script(addon, &setup.mod_dir);
    let defined = Defined::from_fn(|name| host.is_function(name));
    DEFINED.with(|d| d.set(defined));
    log::info(
        "lua",
        "callbacks defined; the game's own code runs for the rest",
        &defined.names().join(", "),
    );
}

thread_local! {
    /// The callbacks the main script defined, read once after it ran: the Lua groups hook only what they reach.
    static DEFINED: Cell<Defined> = const { Cell::new(Defined::EMPTY) };
}

/// The callbacks the main script defined; none without the script.
pub(crate) fn defined() -> Defined {
    DEFINED.with(Cell::get)
}

/// A game address a script or hook passed, as a pointer.
pub(crate) fn ptr<T>(address: u32) -> *mut T {
    core::ptr::with_exposed_provenance_mut(address as usize)
}

/// A pointer as the 32-bit address Lua sees.
pub(crate) fn addr<T>(p: *const T) -> u32 {
    p.expose_provenance() as u32
}

/// The game side of the registered functions. Pointers come from scripts unchecked, as in the original: a wrong one
/// crashes the game in the access.
struct GameAccess;

impl GameAccess {
    /// `GetPerkModifierFor*`: the level byte at `levels + perk`, then the dword +0x34 of perk record `perk * 3 + level
    /// - 1`.
    fn perk_modifier(levels: u32, perk: i32) -> u32 {
        // SAFETY: the script's address plus its perk index, unchecked as in the original.
        let level = unsafe { ptr::<u8>(levels.wrapping_add(perk.cast_unsigned())).read() };
        if level == 0 {
            return 0;
        }
        let index = perk
            .wrapping_mul(3)
            .wrapping_add(i32::from(level))
            .wrapping_sub(1);
        let record = eix_lua::record(perks(), Table::Perks, index);
        if record == 0 {
            return 0;
        }
        // SAFETY: a record of the loaded perk table (or before it, for a negative index, as in the original).
        unsafe { ptr::<u32>(record.wrapping_add(0x34)).read_unaligned() }
    }
}

fn perks() -> TableBounds {
    bounds(database::PERKS)
}

fn bounds(table: eix_engine::game::Global<database::VectorBounds>) -> TableBounds {
    // SAFETY: the build was checked at load; the vector is a static of game.exe, empty before the databases load.
    let v = unsafe { table.read() };
    TableBounds {
        begin: v.begin,
        end: v.end,
    }
}

fn text(bytes: &[u8]) -> CString {
    // The binding cuts its strings at the first NUL, so none is left.
    CString::new(bytes).unwrap_or_default()
}

impl Game for GameAccess {
    fn effect_value(&self, unit: u32, effect_type: i32) -> f32 {
        // SAFETY: the script's unit, on the game's thread.
        unsafe { MapObject::effect_value(ptr(unit), effect_type) }
    }

    fn perk_modifier_for_server(&self, stats: u32, perk: i32) -> u32 {
        let levels = stats.wrapping_add(core::mem::offset_of!(UnitStats, skill_levels) as u32);
        Self::perk_modifier(levels, perk)
    }

    fn perk_modifier_for_client(&self, params: u32, perk: i32) -> u32 {
        let levels = params
            .wrapping_add(
                core::mem::offset_of!(eix_engine::game::unit::UnitParams, skill_levels) as u32,
            );
        Self::perk_modifier(levels, perk)
    }

    fn heal(&self, stats: u32, vitals: u32, amount: f64) {
        let (stats, vitals) = (ptr::<UnitStats>(stats), ptr::<UnitVitals>(vitals));
        // SAFETY: the script's vitals.
        let max = unsafe { (*vitals).max_hit_points };
        // SAFETY: the script's statistics, on the game's thread.
        unsafe { UnitStats::heal_slots(stats, max, amount as f32) };
        // SAFETY: as above; the maximum is read again, as the original does.
        let max = unsafe { (*vitals).max_hit_points };
        // SAFETY: as above.
        let hp = unsafe { UnitStats::reduce(stats, max) };
        // SAFETY: the script's vitals.
        unsafe { (*vitals).hit_points = hp };
    }

    fn regen_mp(&self, vitals: u32, fraction: f64) {
        // SAFETY: the script's vitals.
        let v = unsafe { &mut *ptr::<UnitVitals>(vitals) };
        if let Some(mana) = eix_core::stats::regenerated_mana(v.mana, v.max_mana, fraction as f32) {
            v.mana = mana;
        }
    }

    fn distribute_hp(&self, stats: u32, max_hp: f64) {
        let max_hp = max_hp as f32;
        // SAFETY: the script's statistics.
        let s = unsafe { &mut *ptr::<UnitStats>(stats) };
        for i in 0..s.slots.len() {
            if s.slots[i].kind > 0 {
                let share1 = s.slots[1].share;
                let slot = &mut s.slots[i];
                (slot.maximum, slot.current) =
                    eix_core::stats::distributed_slot(max_hp, share1, slot.share, slot.current);
            }
        }
    }

    fn collect_hp(&self, stats: u32, max_hp: f64) -> f32 {
        // SAFETY: the script's statistics, on the game's thread.
        unsafe { UnitStats::reduce(ptr(stats), max_hp as f32) }
    }

    fn table(&self, table: Table) -> TableBounds {
        bounds(match table {
            Table::Perks => database::PERKS,
            Table::Weapons => database::WEAPONS,
            Table::Armors => database::ARMORS,
            Table::Materials => database::MATERIALS,
            Table::Quicks => database::QUICKS,
            Table::Spells => database::SPELLS,
            Table::Runes => database::RUNES,
        })
    }

    fn perk_index(&self, record: u32) -> i32 {
        // SAFETY: the skill database is a static of game.exe; the lookup only compares addresses.
        unsafe { SkillDatabase::index_of_level(SKILL_DATABASE.as_ptr(), ptr(record)) }
    }

    fn class_id(&self, object: u32) -> i32 {
        // SAFETY: the script's object, on the game's thread.
        unsafe { EngineObject::class_id(ptr(object)) }.cast_signed()
    }

    fn items_weight(&self, unit: u32) -> f32 {
        // SAFETY: the script's unit.
        let u = unsafe { &*ptr::<Unit>(unit) };
        let mut sum = 0.0f32;
        let slots = u
            .weapons
            .iter()
            .chain(&u.quick_items)
            .chain(&u.spells)
            .chain(&u.armor);
        for &item in slots {
            if !item.is_null() {
                // SAFETY: a non-null slot of a live unit holds a live item.
                sum += unsafe { (*item).weight };
            }
        }
        sum
    }

    fn item_shape_entry(&self, item: u32) -> u32 {
        // SAFETY: the script's item, on the game's thread.
        addr(unsafe { Item::prototype(ptr(item)) })
    }

    fn slot_item(&self, unit: u32, kind: SlotKind, slot: i32) -> u32 {
        let unit = ptr::<Unit>(unit);
        // SAFETY: the script's unit.
        let u = unsafe { &*unit };
        let slot = slot.cast_unsigned() as usize;
        // SAFETY: a live unit's controller is a live engine object.
        let client = !u.controller.is_null()
            && unsafe { EngineObject::class_id(u.controller) } == CLASS_UNIT_CLIENT_SPECIFIC;
        if !client {
            let own: &[*mut Item] = match kind {
                SlotKind::Weapon => &u.weapons,
                SlotKind::Armor => &u.armor,
                SlotKind::Quick => &u.quick_items,
                SlotKind::Spell => &u.spells,
            };
            return own.get(slot).map_or(0, |&p| addr(p));
        }
        let player = u.player.cast::<Player>();
        if player.is_null() {
            return 0;
        }
        // SAFETY: a unit's player is a live player.
        let record = unsafe { Player::party_record(player, u.map_object.transform.base.id) };
        if record.is_null() {
            return 0;
        }
        // SAFETY: a record of the player's party.
        let r = unsafe { &*record };
        let list: &ItemList = match kind {
            SlotKind::Weapon => &r.weapons,
            SlotKind::Armor => &r.armor,
            SlotKind::Quick => &r.quick_items,
            SlotKind::Spell => &r.spells,
        };
        if slot >= list.count as usize {
            return 0;
        }
        // SAFETY: `slot` is below the list's count.
        addr(unsafe { list.items.wrapping_add(slot).read() })
    }

    fn unit_is_host(&self, unit: u32) -> bool {
        // SAFETY: the server's world is constructed by game.exe's static initialisers, before any script runs.
        let host = unsafe { eix_engine::game::world::World::get_player(SERVER_WORLD.as_ptr(), 0) };
        if host.is_null() {
            return false;
        }
        // SAFETY: the script's unit.
        let id = unsafe { (*ptr::<Unit>(unit)).map_object.transform.base.id };
        // SAFETY: a player GetPlayer returned is live.
        !unsafe { Player::party_record(host, id) }.is_null()
    }

    fn console_write_line(&self, text_bytes: &[u8]) {
        // SAFETY: the console stream is a static of game.exe, constructed before WinMain; on the game's thread.
        unsafe { console::print_line(&text(text_bytes)) };
    }

    fn gs_var(&self, unit: u32, name: &[u8]) -> Option<f32> {
        let table = variables(unit)?;
        // SAFETY: the unit's player's table, on the game's thread.
        unsafe { NamedFloatTable::get(table, &text(name)) }
    }

    fn set_gs_var(&self, unit: u32, name: &[u8], value: f32) {
        if let Some(table) = variables(unit) {
            // SAFETY: as in `gs_var`.
            unsafe { NamedFloatTable::set(table, &text(name), value) };
        }
    }

    fn gs_vars(&self, unit: u32) -> Option<Vec<(Vec<u8>, f32)>> {
        let table = variables(unit)?;
        let mut all = Vec::new();
        // SAFETY: as in `gs_var`; the closure only copies.
        unsafe {
            NamedFloatTable::for_each_variable(table, |n: &CStr, v| {
                all.push((n.to_bytes().to_vec(), v))
            });
        }
        Some(all)
    }

    fn ini_value(&self, file: &[u8], section: &[u8], key: &[u8]) -> Option<Vec<u8>> {
        crate::ini::read(file, section, key)
    }
}

/// The variables of `unit`'s player, or `None` when the unit has no player.
fn variables(unit: u32) -> Option<*mut NamedFloatTable> {
    // SAFETY: the script's unit.
    let player = unsafe { (*ptr::<Unit>(unit)).player }.cast::<Player>();
    // SAFETY: a unit's non-null player is live.
    (!player.is_null()).then(|| unsafe { &raw mut (*player).variables })
}
