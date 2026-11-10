//! The Lua reference (`docs/eix.lua`) names every function the host registers and every callback it calls.

use std::collections::BTreeSet;
use std::rc::Rc;

use eix_docs::LUA_DEFINITIONS;
use eix_docs::luaapi::{CALLBACKS_SECTION, FUNCTIONS_SECTION, LuaApi};
use eix_lua::{Game, Host, SlotKind, Table, TableBounds, new_state};
use mlua::{Lua, Value};

/// A game with nothing in it: the checks here never reach the game.
struct NoGame;

impl Game for NoGame {
    fn effect_value(&self, _: u32, _: i32) -> f32 {
        0.0
    }
    fn perk_modifier_for_server(&self, _: u32, _: i32) -> u32 {
        0
    }
    fn perk_modifier_for_client(&self, _: u32, _: i32) -> u32 {
        0
    }
    fn heal(&self, _: u32, _: u32, _: f64) {}
    fn regen_mp(&self, _: u32, _: f64) {}
    fn distribute_hp(&self, _: u32, _: f64) {}
    fn collect_hp(&self, _: u32, _: f64) -> f32 {
        0.0
    }
    fn table(&self, _: Table) -> TableBounds {
        TableBounds { begin: 0, end: 0 }
    }
    fn perk_index(&self, _: u32) -> i32 {
        -1
    }
    fn class_id(&self, _: u32) -> i32 {
        0
    }
    fn items_weight(&self, _: u32) -> f32 {
        0.0
    }
    fn item_shape_entry(&self, _: u32) -> u32 {
        0
    }
    fn slot_item(&self, _: u32, _: SlotKind, _: i32) -> u32 {
        0
    }
    fn unit_is_host(&self, _: u32) -> bool {
        false
    }
    fn console_write_line(&self, _: &[u8]) {}
    fn gs_var(&self, _: u32, _: &[u8]) -> Option<f32> {
        None
    }
    fn set_gs_var(&self, _: u32, _: &[u8], _: f32) {}
    fn gs_vars(&self, _: u32) -> Option<Vec<(Vec<u8>, f32)>> {
        None
    }
    fn ini_value(&self, _: &[u8], _: &[u8], _: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn host() -> TestResult<Host> {
    Ok(Host::new(Rc::new(NoGame), Box::new(|_| {}))?)
}

fn global_names(lua: &Lua) -> TestResult<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for pair in lua.globals().pairs::<String, Value>() {
        names.insert(pair?.0);
    }
    Ok(names)
}

fn documented(section: &str) -> TestResult<BTreeSet<String>> {
    let api = LuaApi::parse(LUA_DEFINITIONS)?;
    Ok(api
        .function_names(section)
        .into_iter()
        .map(str::to_owned)
        .collect())
}

#[test]
fn every_registered_function_is_documented() -> TestResult {
    let base = global_names(&new_state()?)?;
    let host = host()?;
    let registered: BTreeSet<String> = global_names(host.lua())?
        .difference(&base)
        .cloned()
        .collect();
    assert_eq!(registered, documented(FUNCTIONS_SECTION)?);
    Ok(())
}

#[test]
fn every_callback_the_host_calls_is_documented() -> TestResult {
    let host = host()?;
    // Each call looks its callback up in the globals; none is defined, so the lookup falls to `__index`, which notes
    // the name.
    host.lua()
        .load(
            "__seen = {} setmetatable(_G, {__index = function(_, k) __seen[#__seen + 1] = k end})",
        )
        .exec()?;
    host.get_skill_price_diff(0, 0);
    host.get_perk_price(0, 0);
    host.get_modified_damage_on_server(0, 0, 0.0);
    host.get_modified_damage_on_client(0, 0, 0.0);
    host.calc_base_health(0);
    host.calc_base_mana(0);
    host.get_modified_health(0, 0, 0.0);
    host.recalc_unit_stats(0, 0, 0, 0);
    host.recalc_internal_hero_stats(0, 0.0);
    host.regenerate_hp_and_mp(0, 0, 0, false);
    host.calc_running_mana_cost(0, 0, 0);
    host.calc_attack_params_on_server(0, 0, 0, 0);
    host.calc_attack_params_on_client(0, 0, 0, 0);
    host.get_mana_bonus_on_server(0);
    host.get_mana_bonus_on_client(0);
    host.get_exp_multiplier_on_server(0);
    host.get_exp_multiplier_on_client(0);
    host.recalc_shops_price_multipliers(0, false);
    host.distribute_lmp_exp(0.0, 0);
    let mut seen = BTreeSet::new();
    for name in host
        .lua()
        .globals()
        .raw_get::<mlua::Table>("__seen")?
        .sequence_values::<String>()
    {
        seen.insert(name?);
    }
    assert_eq!(seen, documented(CALLBACKS_SECTION)?);
    Ok(())
}

#[test]
fn the_definitions_file_is_valid_lua() -> TestResult {
    new_state()?.load(LUA_DEFINITIONS).exec()?;
    Ok(())
}
