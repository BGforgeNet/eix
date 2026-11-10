//! The host: the state, the main script, the callbacks and the registered functions, with
//! a fake game and the fatal report replaced.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use eix_core::options::AddonSection;
use eix_lua::{Game, Host, SlotKind, Table, TableBounds};
use mlua::Value;

/// Records every call as text and answers with values the tests can recognise.
#[derive(Default)]
struct FakeGame {
    calls: RefCell<Vec<String>>,
    console: RefCell<Vec<Vec<u8>>>,
    vars: RefCell<Vec<(Vec<u8>, f32)>>,
}

impl FakeGame {
    fn log(&self, s: String) {
        self.calls.borrow_mut().push(s);
    }
}

const SPELLS: TableBounds = TableBounds {
    begin: 0x0100_0000,
    end: 0x0100_0000 + 5 * 0x84,
};

impl Game for FakeGame {
    fn effect_value(&self, unit: u32, effect_type: i32) -> f32 {
        self.log(format!("effect_value {unit} {effect_type}"));
        1.5
    }
    fn perk_modifier_for_server(&self, stats: u32, perk: i32) -> u32 {
        self.log(format!("perk_modifier_for_server {stats} {perk}"));
        0xffff_fff0
    }
    fn perk_modifier_for_client(&self, params: u32, perk: i32) -> u32 {
        self.log(format!("perk_modifier_for_client {params} {perk}"));
        7
    }
    fn heal(&self, stats: u32, vitals: u32, amount: f64) {
        self.log(format!("heal {stats} {vitals} {amount}"));
    }
    fn regen_mp(&self, vitals: u32, fraction: f64) {
        self.log(format!("regen_mp {vitals} {fraction}"));
    }
    fn distribute_hp(&self, stats: u32, max_hp: f64) {
        self.log(format!("distribute_hp {stats} {max_hp}"));
    }
    fn collect_hp(&self, stats: u32, max_hp: f64) -> f32 {
        self.log(format!("collect_hp {stats} {max_hp}"));
        42.25
    }
    fn table(&self, table: Table) -> TableBounds {
        match table {
            Table::Spells => SPELLS,
            Table::Perks => TableBounds {
                begin: 0x2000,
                end: 0x2000 + 2 * 0x44,
            },
            Table::Weapons | Table::Armors | Table::Materials | Table::Quicks | Table::Runes => {
                TableBounds { begin: 0, end: 0 }
            }
        }
    }
    fn perk_index(&self, record: u32) -> i32 {
        self.log(format!("perk_index {record}"));
        3
    }
    fn class_id(&self, object: u32) -> i32 {
        self.log(format!("class_id {object}"));
        0x3d
    }
    fn items_weight(&self, unit: u32) -> f32 {
        self.log(format!("items_weight {unit}"));
        12.5
    }
    fn item_shape_entry(&self, item: u32) -> u32 {
        self.log(format!("item_shape_entry {item}"));
        0x0040_0000
    }
    fn slot_item(&self, unit: u32, kind: SlotKind, slot: i32) -> u32 {
        self.log(format!("slot_item {unit} {kind:?} {slot}"));
        if slot == 0 { 0x0900_0000 } else { 0 }
    }
    fn unit_is_host(&self, unit: u32) -> bool {
        self.log(format!("unit_is_host {unit}"));
        unit == 1
    }
    fn console_write_line(&self, text: &[u8]) {
        self.console.borrow_mut().push(text.to_vec());
    }
    fn gs_var(&self, unit: u32, name: &[u8]) -> Option<f32> {
        if unit == 0 {
            return None;
        }
        let vars = self.vars.borrow();
        vars.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
    }
    fn set_gs_var(&self, unit: u32, name: &[u8], value: f32) {
        if unit == 0 {
            return;
        }
        let mut vars = self.vars.borrow_mut();
        match vars.iter_mut().find(|(n, _)| n == name) {
            Some(entry) => entry.1 = value,
            None => vars.push((name.to_vec(), value)),
        }
    }
    fn gs_vars(&self, unit: u32) -> Option<Vec<(Vec<u8>, f32)>> {
        (unit != 0).then(|| self.vars.borrow().clone())
    }
    fn ini_value(&self, file: &[u8], section: &[u8], key: &[u8]) -> Option<Vec<u8>> {
        self.log(format!(
            "ini_value {} {} {}",
            String::from_utf8_lossy(file),
            String::from_utf8_lossy(section),
            String::from_utf8_lossy(key)
        ));
        (file == b"a.ini" && section == b"Main" && key == b"Speed").then(|| b"1,5".to_vec())
    }
}

struct Fixture {
    host: Host,
    game: Rc<FakeGame>,
    fatal: Rc<RefCell<Vec<String>>>,
}

fn fixture() -> Fixture {
    let game = Rc::new(FakeGame::default());
    let fatal = Rc::new(RefCell::new(Vec::new()));
    let sink = fatal.clone();
    let host = Host::new(
        game.clone(),
        Box::new(move |m: &[u8]| {
            sink.borrow_mut()
                .push(String::from_utf8_lossy(m).into_owned())
        }),
    )
    .unwrap_or_else(|e| panic!("Host::new: {e}"));
    Fixture { host, game, fatal }
}

impl Fixture {
    fn exec(&self, code: &str) {
        self.host
            .lua()
            .load(code)
            .exec()
            .unwrap_or_else(|e| panic!("{code}: {e}"));
    }

    fn eval<R: mlua::FromLuaMulti>(&self, code: &str) -> R {
        self.host
            .lua()
            .load(code)
            .eval()
            .unwrap_or_else(|e| panic!("{code}: {e}"))
    }

    /// The message of a script error, caught with pcall, from a chunk named `t`.
    fn error_of(&self, code: &str) -> String {
        let (ok, msg): (bool, String) = self
            .host
            .lua()
            .load(format!(
                "local ok, e = pcall(function() {code} end) return ok, e"
            ))
            .set_name("=t")
            .eval()
            .unwrap_or_else(|e| panic!("{code}: {e}"));
        assert!(!ok, "{code} did not fail");
        msg
    }

    fn fatal(&self) -> Vec<String> {
        self.fatal.borrow().clone()
    }

    fn calls(&self) -> Vec<String> {
        self.game.calls.borrow().clone()
    }
}

// ---- 1. The state ----

#[test]
fn opens_the_openlibs_set_with_ffi_only_through_require() {
    let f = fixture();
    let libs: String = f.eval(
        "local t = {} for _, n in ipairs({'package', 'table', 'io', 'os', 'string', 'math', 'debug', 'bit', \
         'jit', 'coroutine'}) do t[#t + 1] = type(_G[n]) end return table.concat(t, ' ')",
    );
    assert_eq!(
        libs,
        "table table table table table table table table table table"
    );
    let (global, preload, cdef): (String, String, String) =
        f.eval("return type(ffi), type(package.preload.ffi), type(require('ffi').cdef)");
    assert_eq!(
        (global.as_str(), preload.as_str(), cdef.as_str()),
        ("nil", "function", "function")
    );
    let loaders: i32 = f.eval("return #package.loaders");
    assert_eq!(loaders, 4, "the C module loaders stay");
}

#[test]
fn registers_all_57_functions() {
    let f = fixture();
    let names = [
        "Round",
        "GetEffectValue",
        "GetPerkModifierForServer",
        "GetPerkModifierForClient",
        "Heal",
        "RegenMp",
        "DistributeHp",
        "CollectHp",
        "GetPerkRecord",
        "GetWeaponRecord",
        "GetArmorRecord",
        "GetMaterialRecord",
        "GetQuickRecord",
        "GetSpellRecord",
        "GetRuneRecord",
        "GetPerksTableSize",
        "GetWeaponsTableSize",
        "GetArmorsTableSize",
        "GetMaterialsTableSize",
        "GetQuicksTableSize",
        "GetSpellsTableSize",
        "GetRunesTableSize",
        "GetPerkIndex",
        "GetSpellIndex",
        "GetRuneIndex",
        "GetClassId",
        "GetItemsWeight",
        "GetItemShapeEntry",
        "GetWeapon",
        "GetArmor",
        "GetQuick",
        "GetSpell",
        "ConsoleWriteLine",
        "IDiv",
        "IMod",
        "GetGSVar",
        "SetGSVar",
        "UnitIsHost",
        "TempDumpVars",
        "StrFromMem",
        "FloatFromMem",
        "DoubleFromMem",
        "UInt32FromMem",
        "UInt16FromMem",
        "UInt8FromMem",
        "Int32FromMem",
        "Int16FromMem",
        "Int8FromMem",
        "FloatToMem",
        "DoubleToMem",
        "UInt32ToMem",
        "UInt16ToMem",
        "UInt8ToMem",
        "Int32ToMem",
        "Int16ToMem",
        "Int8ToMem",
        "GetIniValue",
    ];
    assert_eq!(names.len(), 57);
    for name in names {
        let what: String = f.eval(&format!("return debug.getinfo({name}, 'S').what"));
        assert_eq!(what, "C", "{name}");
    }
}

#[test]
fn get_ini_value_gives_the_string_or_nil() {
    let f = fixture();
    let (value, number): (String, f64) = f.eval(
        "local v = GetIniValue('a.ini', 'Main', 'Speed') return v, tonumber((v:gsub(',', '.')))",
    );
    assert_eq!((value.as_str(), number), ("1,5", 1.5));
    assert!(f.eval::<bool>("return GetIniValue('a.ini', 'Main', 'Other') == nil"));
    assert!(f.eval::<bool>("return GetIniValue('b.ini', 'Main', 'Speed') == nil"));
    assert_eq!(f.calls()[0], "ini_value a.ini Main Speed");
    assert_eq!(
        f.error_of("GetIniValue('a.ini', 'Main')"),
        "t:1: bad argument #3 to 'GetIniValue' (string expected, got no value)"
    );
}

// ---- 2. The main script ----

/// A fresh mod folder under the temp folder, with the given files.
fn mod_dir(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("eix-lua-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("lua")).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for (file, content) in files {
        std::fs::write(dir.join(file), content).unwrap_or_else(|e| panic!("{file}: {e}"));
    }
    dir
}

fn addon(path: &str, enabled: i32) -> AddonSection {
    AddonSection {
        lua_main_script_path: path.as_bytes().to_vec(),
        enable_lua_scripts: enabled,
        ..AddonSection::default()
    }
}

fn dir_bytes(dir: &std::path::Path) -> Vec<u8> {
    dir.to_str()
        .unwrap_or_else(|| panic!("{} is not UTF-8", dir.display()))
        .as_bytes()
        .to_vec()
}

#[test]
fn runs_main_lua_from_the_mod_folder_with_package_path() {
    let dir = mod_dir(
        "main",
        &[
            (
                "main.lua",
                b"ran = (ran or 0) + 1 helper = require('helper')",
            ),
            ("lua/helper.lua", b"return 'from lua/'"),
        ],
    );
    let f = fixture();
    let before: String = f.eval("return package.path");
    f.host
        .run_main_script(&addon("main.lua", 1), &dir_bytes(&dir));
    assert_eq!(f.fatal(), Vec::<String>::new());
    let slashed = format!("{}/", dir.to_str().unwrap().replace('\\', "/"));
    let after: String = f.eval("return package.path");
    assert_eq!(after, format!("{before};{slashed}?.lua;{slashed}lua/?.lua"));
    // The original's path reads `.\?.lua;<exe>\lua\?.lua;<exe>\lua\?\init.lua;;C:/.../?.lua;...`: LuaJIT's
    // default ends in `;`, so the mod folder follows `;;`.
    assert!(before.starts_with(".\\?.lua;"), "{before}");
    assert!(before.ends_with("\\lua\\?\\init.lua;"), "{before}");
    assert!(
        after.contains(&format!("init.lua;;{slashed}?.lua;")),
        "{after}"
    );
    let (ran, helper): (i32, String) = f.eval("return ran, helper");
    assert_eq!((ran, helper.as_str()), (1, "from lua/"));
    // `addon moddir` reads the config again: main.lua runs a second time in the same state.
    f.host
        .run_main_script(&addon("main.lua", 1), &dir_bytes(&dir));
    assert_eq!(f.eval::<i32>("return ran"), 2);
}

#[test]
fn keeps_backslashes_of_the_script_path() {
    let dir = mod_dir("sub", &[("lua/main.lua", b"ran = true")]);
    let f = fixture();
    f.host
        .run_main_script(&addon("lua\\main.lua", 1), &dir_bytes(&dir));
    assert_eq!(f.fatal(), Vec::<String>::new());
    assert!(f.eval::<bool>("return ran"));
}

#[test]
fn does_nothing_when_lua_scripts_are_off() {
    let dir = mod_dir("off", &[("main.lua", b"ran = true")]);
    let f = fixture();
    let before: String = f.eval("return package.path");
    f.host
        .run_main_script(&addon("main.lua", 0), &dir_bytes(&dir));
    assert_eq!(f.eval::<String>("return package.path"), before);
    assert_eq!(f.eval::<Value>("return ran"), Value::Nil);
}

#[test]
fn loads_bytecode() {
    let f = fixture();
    let bytecode: mlua::LuaString = f.eval("return string.dump(function() ran = 'bytecode' end)");
    assert_eq!(bytecode.as_bytes()[0], 0x1b);
    let dir = mod_dir("bc", &[("main.luac", &bytecode.as_bytes())]);
    f.host
        .run_main_script(&addon("main.luac", 1), &dir_bytes(&dir));
    assert_eq!(f.fatal(), Vec::<String>::new());
    assert_eq!(f.eval::<String>("return ran"), "bytecode");
}

#[test]
fn reports_a_missing_main_script() {
    let dir = mod_dir("missing", &[]);
    let f = fixture();
    f.host
        .run_main_script(&addon("main.lua", 1), &dir_bytes(&dir));
    let fatal = f.fatal();
    assert_eq!(fatal.len(), 1);
    assert!(fatal[0].starts_with("cannot open "), "{fatal:?}");
    assert!(fatal[0].contains("main.lua"), "{fatal:?}");
}

#[test]
fn reports_a_main_script_error_with_its_traceback_and_percent_literally() {
    let dir = mod_dir("error", &[("main.lua", b"\nerror('boom %d %s')\n")]);
    let f = fixture();
    f.host
        .run_main_script(&addon("main.lua", 1), &dir_bytes(&dir));
    let fatal = f.fatal();
    assert_eq!(fatal.len(), 1, "{fatal:?}");
    let script = format!("{}/main.lua", dir.to_str().unwrap().replace('\\', "/"));
    let lines: Vec<&str> = fatal[0].lines().collect();
    assert!(lines[0].ends_with("main.lua:2: boom %d %s"), "{fatal:?}");
    assert_eq!(lines[1], "stack traceback:");
    assert_eq!(lines[2], "\t[C]: in function 'error'");
    assert!(lines[3].ends_with("main.lua:2: in main chunk"), "{fatal:?}");
    assert_eq!(
        lines.len(),
        4,
        "the traceback ends at the script: {fatal:?}"
    );
    assert!(fatal[0].contains(&script[script.len() - 20..]), "{fatal:?}");
}

#[test]
fn ignores_error_nil_in_main_script() {
    let dir = mod_dir(
        "nil",
        &[("main.lua", b"before = true error(nil) after = true")],
    );
    let f = fixture();
    f.host
        .run_main_script(&addon("main.lua", 1), &dir_bytes(&dir));
    assert_eq!(f.fatal(), Vec::<String>::new());
    assert!(f.eval::<bool>("return before"));
    assert_eq!(f.eval::<Value>("return after"), Value::Nil);
}

#[test]
fn reports_a_syntax_error_without_running() {
    let dir = mod_dir("syntax", &[("main.lua", b"x = = 1")]);
    let f = fixture();
    f.host
        .run_main_script(&addon("main.lua", 1), &dir_bytes(&dir));
    let fatal = f.fatal();
    assert_eq!(fatal.len(), 1);
    assert!(fatal[0].contains("main.lua:1:"), "{fatal:?}");
    assert!(
        !fatal[0].contains("stack traceback"),
        "a load error has no traceback: {fatal:?}"
    );
}

// ---- 3. Calls into Lua ----

#[test]
fn callbacks_get_their_arguments_and_results_are_read() {
    let f = fixture();
    f.exec(
        r#"
        seen = {}
        local function keep(name, ...) seen[name] = {...} end
        function GetSkillPriceDiff(...) keep('GetSkillPriceDiff', ...) return 12.9 end
        function GetPerkPrice(...) keep('GetPerkPrice', ...) return '0x20' end
        function GetModifiedDamageOnServer(...) keep('GetModifiedDamageOnServer', ...) return 2.5 end
        function GetModifiedDamageOnClient(...) keep('GetModifiedDamageOnClient', ...) return '3.25' end
        function CalcBaseHealth(...) keep('CalcBaseHealth', ...) return 100 end
        function CalcBaseMana(...) keep('CalcBaseMana', ...) return 50 end
        function GetModifiedHealth(...) keep('GetModifiedHealth', ...) return 1/3 end
        function RecalcUnitStats(...) keep('RecalcUnitStats', ...) return 1 end
        function RecalcInternalHeroStats(...) keep('RecalcInternalHeroStats', ...) end
        function RegenerateHpAndMp(...) keep('RegenerateHpAndMp', ...) return 0 end
        function CalcRunningManaCost(...) keep('CalcRunningManaCost', ...) end
        function CalcAttackParamsOnServer(...) keep('CalcAttackParamsOnServer', ...) end
        function CalcAttackParamsOnClient(...) keep('CalcAttackParamsOnClient', ...) end
        function GetManaBonusOnServer(...) keep('GetManaBonusOnServer', ...) return 1.25 end
        function GetManaBonusOnClient(...) keep('GetManaBonusOnClient', ...) return 'x' end
        function GetExpMultiplierOnServer(...) keep('GetExpMultiplierOnServer', ...) return 2 end
        function GetExpMultiplierOnClient(...) keep('GetExpMultiplierOnClient', ...) end
        function RecalcShopsPriceMultipliers(...) keep('RecalcShopsPriceMultipliers', ...) end
        function DistributeLmpExp(...) keep('DistributeLmpExp', ...) return 4e9 end
        "#,
    );
    let h = &f.host;
    assert_eq!(h.get_skill_price_diff(3, 4), 12);
    assert_eq!(h.get_perk_price(0x1000, 0x2000), 0x20);
    assert_eq!(h.get_modified_damage_on_server(1, 2, 0.5), 2.5);
    assert_eq!(h.get_modified_damage_on_client(1, 2, 0.5), 3.25);
    assert_eq!(h.calc_base_health(5), 100.0);
    assert_eq!(h.calc_base_mana(5), 50.0);
    assert_eq!(h.get_modified_health(5, 6, 7.5), (1.0f64 / 3.0) as f32);
    h.recalc_unit_stats(1, 2, 3, 4);
    h.recalc_internal_hero_stats(1, 0.75);
    assert!(h.regenerate_hp_and_mp(1, 2, 3, true), "0 is true in Lua");
    h.calc_running_mana_cost(1, 2, 3);
    h.calc_attack_params_on_server(1, 2, 3, 4);
    h.calc_attack_params_on_client(5, 6, 7, 8);
    assert_eq!(h.get_mana_bonus_on_server(9), 1.25);
    assert_eq!(
        h.get_mana_bonus_on_client(9),
        0.0,
        "a non-numeric string reads as 0.0"
    );
    assert_eq!(h.get_exp_multiplier_on_server(9), 2.0);
    assert_eq!(
        h.get_exp_multiplier_on_client(9),
        0.0,
        "no result reads as 0.0"
    );
    h.recalc_shops_price_multipliers(0x3000, false);
    assert_eq!(h.distribute_lmp_exp(10.5, 3), 4e9);
    assert_eq!(f.fatal(), Vec::<String>::new());

    let seen = |name: &str| -> String {
        f.eval(&format!(
            "local t = {{}} for i, v in ipairs(seen.{name}) do t[i] = type(v) .. ':' .. tostring(v) end \
             return table.concat(t, ' ')"
        ))
    };
    assert_eq!(seen("GetSkillPriceDiff"), "number:3 number:4");
    assert_eq!(seen("GetPerkPrice"), "number:4096 number:8192");
    assert_eq!(
        seen("GetModifiedDamageOnServer"),
        "number:1 number:2 number:0.5"
    );
    assert_eq!(
        seen("GetModifiedDamageOnClient"),
        "number:1 number:2 number:0.5"
    );
    assert_eq!(seen("CalcBaseHealth"), "number:5");
    assert_eq!(seen("CalcBaseMana"), "number:5");
    assert_eq!(seen("GetModifiedHealth"), "number:5 number:6 number:7.5");
    assert_eq!(
        seen("RecalcUnitStats"),
        "number:1 number:2 number:3 number:4"
    );
    assert_eq!(seen("RecalcInternalHeroStats"), "number:1 number:0.75");
    assert_eq!(
        seen("RegenerateHpAndMp"),
        "number:1 number:2 number:3 boolean:true"
    );
    assert_eq!(seen("CalcRunningManaCost"), "number:1 number:2 number:3");
    assert_eq!(
        seen("CalcAttackParamsOnServer"),
        "number:1 number:2 number:3 number:4"
    );
    assert_eq!(
        seen("CalcAttackParamsOnClient"),
        "number:5 number:6 number:7 number:8"
    );
    assert_eq!(seen("GetManaBonusOnServer"), "number:9");
    assert_eq!(seen("GetManaBonusOnClient"), "number:9");
    assert_eq!(seen("GetExpMultiplierOnServer"), "number:9");
    assert_eq!(seen("GetExpMultiplierOnClient"), "number:9");
    assert_eq!(
        seen("RecalcShopsPriceMultipliers"),
        "number:12288 boolean:false"
    );
    assert_eq!(seen("DistributeLmpExp"), "number:10.5 number:3");
}

#[test]
fn integer_results_truncate_and_overflow_to_int_min() {
    let f = fixture();
    f.exec("function GetSkillPriceDiff(from, to) return result end");
    for (value, expected) in [
        ("-7.9", -7),
        ("4294967295", i32::MIN),
        ("0/0", i32::MIN),
        ("'12'", 12),
        ("'twelve'", 0),
        ("nil", 0),
        ("true", 0),
    ] {
        f.exec(&format!("result = {value}"));
        assert_eq!(f.host.get_skill_price_diff(0, 1), expected, "{value}");
    }
    f.exec("function RegenerateHpAndMp() return result end");
    for (value, expected) in [("nil", false), ("false", false), ("0", true), ("''", true)] {
        f.exec(&format!("result = {value}"));
        assert_eq!(
            f.host.regenerate_hp_and_mp(0, 0, 0, false),
            expected,
            "{value}"
        );
    }
}

#[test]
fn callbacks_are_looked_up_on_every_call() {
    let f = fixture();
    f.exec("function CalcBaseMana() return 1 end");
    assert_eq!(f.host.calc_base_mana(0), 1.0);
    f.exec("function CalcBaseMana() return 2 end");
    assert_eq!(f.host.calc_base_mana(0), 2.0);
}

#[test]
fn only_a_function_global_counts_as_a_defined_callback() {
    let f = fixture();
    f.exec("function CalcBaseMana() return 1 end CalcBaseHealth = 5 GetPerkPrice = {}");
    assert!(f.host.is_function("CalcBaseMana"));
    assert!(!f.host.is_function("CalcBaseHealth"));
    assert!(!f.host.is_function("GetPerkPrice"));
    assert!(!f.host.is_function("GetModifiedHealth"));
    // A registered C function counts too: the check reads the global's type, not who made it.
    assert!(f.host.is_function("Round"));
}

#[test]
fn a_missing_callback_is_a_fatal_error() {
    let f = fixture();
    assert_eq!(f.host.get_perk_price(1, 2), 0);
    let fatal = f.fatal();
    assert_eq!(fatal.len(), 1);
    assert!(
        fatal[0].starts_with("attempt to call a nil value"),
        "{fatal:?}"
    );
    assert!(fatal[0].contains("\nstack traceback:"), "{fatal:?}");
}

#[test]
fn a_callback_error_is_reported_with_its_traceback() {
    let f = fixture();
    f.host
        .lua()
        .load("function CalcBaseHealth(stats)\n  local x = nil\n  return x.field\nend")
        .set_name("@cb.lua")
        .exec()
        .unwrap();
    assert_eq!(f.host.calc_base_health(1), 0.0);
    let fatal = f.fatal();
    assert_eq!(fatal.len(), 1);
    assert_eq!(
        fatal[0],
        "cb.lua:3: attempt to index local 'x' (a nil value)\nstack traceback:\n\tcb.lua:3: in function <cb.lua:1>"
    );
}

#[test]
fn error_nil_in_a_callback_reads_as_the_default() {
    let f = fixture();
    f.exec(
        "function GetPerkPrice() error(nil) end \
         function RegenerateHpAndMp() error() end \
         function CalcBaseMana() error(nil) end",
    );
    assert_eq!(f.host.get_perk_price(1, 2), 0);
    assert!(!f.host.regenerate_hp_and_mp(1, 2, 3, false));
    assert_eq!(f.host.calc_base_mana(1), 0.0);
    assert_eq!(f.fatal(), Vec::<String>::new());
}

#[test]
fn error_objects_with_tostring_are_reported_through_it() {
    let f = fixture();
    f.exec(
        "function CalcBaseMana() error(setmetatable({}, {__tostring = function() return 'custom' end})) end \
         function CalcBaseHealth() error(42) end",
    );
    f.host.calc_base_mana(1);
    f.host.calc_base_health(1);
    let fatal = f.fatal();
    assert_eq!(fatal.len(), 2);
    assert!(
        fatal[0].starts_with("custom\nstack traceback:\n\t[C]: in function 'error'"),
        "{fatal:?}"
    );
    // A number gets the position like a string, and becomes text.
    assert!(
        fatal[1].contains(": 42\nstack traceback:\n\t[C]: in function 'error'"),
        "{fatal:?}"
    );
}

#[test]
fn an_error_value_without_text_is_reported_empty() {
    let f = fixture();
    f.exec("function CalcBaseMana() error({}) end");
    f.host.calc_base_mana(1);
    assert_eq!(f.fatal(), [String::new()]);
}

// ---- 4. Registered functions ----

#[test]
fn round_and_integer_arithmetic() {
    let f = fixture();
    let r: (f64, f64, f64, f64, f64) =
        f.eval("return Round(2.5), Round(-2.5), Round(2.49), Round(-0.5), Round('7.5')");
    assert_eq!(r, (3.0, -3.0, 2.0, -1.0, 8.0));
    let d: (i32, i32, i32, i32) =
        f.eval("return IDiv(-7, 2), IMod(-7, 2), IDiv(7.9, 2), IMod(7, -3)");
    assert_eq!(d, (-3, -1, 3, 1));
}

#[test]
fn argument_errors_are_luajits() {
    let f = fixture();
    for (code, message) in [
        (
            "Round('x')",
            "t:1: bad argument #1 to 'Round' (number expected, got string)",
        ),
        (
            "Round()",
            "t:1: bad argument #1 to 'Round' (number expected, got no value)",
        ),
        (
            "UInt8ToMem(0, {})",
            "t:1: bad argument #2 to 'UInt8ToMem' (number expected, got table)",
        ),
        (
            "Int32ToMem()",
            "t:1: bad argument #1 to 'Int32ToMem' (number expected, got no value)",
        ),
        (
            "ConsoleWriteLine(nil)",
            "t:1: bad argument #1 to 'ConsoleWriteLine' (string expected, got nil)",
        ),
        (
            "GetGSVar(1, true)",
            "t:1: bad argument #2 to 'GetGSVar' (string expected, got boolean)",
        ),
        (
            "SetGSVar(1, 'n')",
            "t:1: bad argument #3 to 'SetGSVar' (number expected, got no value)",
        ),
    ] {
        assert_eq!(f.error_of(code), message);
    }
    // Called through pcall, LuaJIT has no name for the function.
    let (kind, msg): (String, String) = f.eval("local ok, e = pcall(Round) return type(e), e");
    assert_eq!(kind, "string");
    assert_eq!(
        msg,
        "bad argument #1 to '?' (number expected, got no value)"
    );
}

#[test]
fn argument_errors_leave_rust_state_intact() {
    let f = fixture();
    // Each failing call goes through mlua's callback and the trampoline; a skipped destructor would leak or
    // corrupt the state over many calls.
    let n: i32 = f.eval(
        "local n = 0 for i = 1, 20000 do if not pcall(GetGSVar, 1, {}) then n = n + 1 end end \
         collectgarbage() return n",
    );
    assert_eq!(n, 20000);
    assert_eq!(f.eval::<f64>("return Round(1.5)"), 2.0);
}

#[test]
fn an_argument_error_in_a_callback_is_reported_with_the_c_frame() {
    let f = fixture();
    f.host
        .lua()
        .load("function CalcBaseMana()\n  local r = Round({})\n  return r\nend")
        .set_name("@cb.lua")
        .exec()
        .unwrap();
    f.host.calc_base_mana(1);
    assert_eq!(
        f.fatal(),
        vec![
            "cb.lua:2: bad argument #1 to 'Round' (number expected, got table)\nstack traceback:\n\t[C]: in \
             function 'Round'\n\tcb.lua:2: in function <cb.lua:1>"
                .to_string()
        ]
    );
}

#[test]
#[should_panic(expected = "integer divide error")]
fn idiv_by_zero_ends_the_game() {
    let f = fixture();
    let _: Value = f.eval("return IDiv(1, 0)");
}

#[test]
fn game_functions_reach_the_game() {
    let f = fixture();
    let r: (f64, f64, f64, f64) = f.eval(
        "return GetEffectValue(10, 3), GetPerkModifierForServer(11, 2), GetPerkModifierForClient(12, 1), \
         CollectHp(13, 99.5)",
    );
    // 0xfffffff0 passes through a float: it reads 2^32.
    assert_eq!(r, (1.5, 4_294_967_296.0, 7.0, 42.25));
    f.exec("Heal(1, 2, 3.5) RegenMp(4, 0.25) DistributeHp(5, 80)");
    let r: (i32, i32, f64, i32) = f
        .eval("return GetPerkIndex(20), GetClassId(21), GetItemsWeight(22), GetItemShapeEntry(23)");
    assert_eq!(r, (3, 0x3d, 12.5, 0x0040_0000));
    let r: (i32, i32, i32, i32, i32) =
        f.eval("return GetWeapon(30, 0), GetArmor(31, 6), GetQuick(32, 0), GetSpell(33, 7), GetWeapon(34, -1)");
    assert_eq!(r, (0x0900_0000, 0, 0x0900_0000, 0, 0));
    let r: (bool, bool) = f.eval("return UnitIsHost(1), UnitIsHost(2)");
    assert_eq!(r, (true, false));
    assert_eq!(
        f.calls(),
        [
            "effect_value 10 3",
            "perk_modifier_for_server 11 2",
            "perk_modifier_for_client 12 1",
            "collect_hp 13 99.5",
            "heal 1 2 3.5",
            "regen_mp 4 0.25",
            "distribute_hp 5 80",
            "perk_index 20",
            "class_id 21",
            "items_weight 22",
            "item_shape_entry 23",
            "slot_item 30 Weapon 0",
            "slot_item 31 Armor 6",
            "slot_item 32 Quick 0",
            "slot_item 33 Spell 7",
            "slot_item 34 Weapon -1",
            "unit_is_host 1",
            "unit_is_host 2",
        ]
    );
}

#[test]
fn table_getters_use_the_game_vectors() {
    let f = fixture();
    let begin = SPELLS.begin as i32;
    let r: (i32, i32, i32, i32, i32, i32) = f.eval(&format!(
        "return GetSpellsTableSize(), GetSpellRecord(1), GetSpellRecord(5), GetSpellRecord(-1), \
         GetSpellIndex({b2}), GetRuneIndex({b2})",
        b2 = begin + 2 * 0x84
    ));
    assert_eq!(r, (5, begin + 0x84, 0, begin - 0x84, 2, -1));
    let r: (i32, i32, i32, i32) = f.eval(
        "return GetPerksTableSize(), GetPerkRecord(1), GetWeaponsTableSize(), GetWeaponRecord(0)",
    );
    assert_eq!(r, (2, 0x2000 + 0x44, 0, 0));
}

#[test]
fn console_and_player_variables() {
    let f = fixture();
    f.exec(
        "ConsoleWriteLine('hello') ConsoleWriteLine(12.5) SetGSVar(1, 'gold', 1.5) SetGSVar(1, 'x', -2) \
         SetGSVar(0, 'lost', 1)",
    );
    let r: (f64, Value, Value) =
        f.eval("return GetGSVar(1, 'gold'), GetGSVar(1, 'none'), GetGSVar(0, 'gold')");
    assert_eq!(r, (1.5, Value::Nil, Value::Nil));
    f.exec("TempDumpVars(1) TempDumpVars(0)");
    let console: Vec<String> = f
        .game
        .console
        .borrow()
        .iter()
        .map(|l| String::from_utf8_lossy(l).into_owned())
        .collect();
    assert_eq!(
        console,
        [
            "hello",
            "12.5",
            "Name: \"gold\" = 1.50",
            "Name: \"x\" = -2.00"
        ]
    );
}

#[test]
fn memory_functions_read_and_write_the_given_address() {
    let f = fixture();
    let mut buf = [0u8; 32];
    let a = buf.as_mut_ptr() as usize as i32;
    f.exec(&format!(
        "local a = {a}
         Int32ToMem(a, -2) UInt16ToMem(a + 4, 70000) Int8ToMem(a + 6, -1) UInt8ToMem(a + 7, 0x1ff)
         FloatToMem(a + 9, 0.1) DoubleToMem(a + 13, 0.1) UInt32ToMem(a + 21, 4294967295)"
    ));
    assert_eq!(&buf[0..4], &(-2i32).to_le_bytes());
    assert_eq!(&buf[4..6], &(70000u32 as u16).to_le_bytes());
    assert_eq!(buf[6], 0xff);
    assert_eq!(buf[7], 0xff);
    assert_eq!(&buf[9..13], &0.1f32.to_le_bytes(), "unaligned float");
    assert_eq!(&buf[13..21], &0.1f64.to_le_bytes(), "unaligned double");
    assert_eq!(
        &buf[21..25],
        &0x8000_0000u32.to_le_bytes(),
        "4294967295 overflows to 0x80000000"
    );
    let r: (i32, i32, i32, i32, i32, i32, i32, f64, f64) = f.eval(&format!(
        "local a = {a} return Int32FromMem(a), UInt32FromMem(a), UInt16FromMem(a + 4), Int16FromMem(a + 4), \
         UInt8FromMem(a + 6), Int8FromMem(a + 6), UInt32FromMem(a + 21), FloatFromMem(a + 9), DoubleFromMem(a + 13)"
    ));
    assert_eq!(
        r,
        (
            -2,
            -2,
            4464,
            4464,
            255,
            -1,
            i32::MIN,
            f64::from(0.1f32),
            0.1
        )
    );
}

#[test]
fn str_from_mem_dereferences_a_char_pointer() {
    let f = fixture();
    let text = b"Zone1\0rest";
    let slots: [usize; 2] = [text.as_ptr() as usize, 0];
    let a = slots.as_ptr() as usize as i32;
    let r: (String, Value) = f.eval(&format!("return StrFromMem({a}), StrFromMem({a} + 4)"));
    assert_eq!(r, ("Zone1".to_string(), Value::Nil));
}
