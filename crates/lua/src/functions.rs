//! The 57 functions registered for scripts: the original's 56 in its order, then `GetIniValue`. The memory family is in
//! [`crate::memory`]; the game-side work goes through [`Game`].

use std::rc::Rc;

use mlua::{Lua, Value};

use crate::bind::register;
use crate::convert::{lua_to_ptr, ptr_to_lua};
use crate::game::{Game, SlotKind, Table};
use crate::memory;

/// `Round`: half away from zero. The integer part of `x`, plus 1 (x > 0) or minus 1 (x <= 0) when the fraction's
/// magnitude is at least 0.5.
fn round(x: f64) -> f64 {
    let whole = x.trunc();
    if (x - whole).abs() >= 0.5 {
        if x > 0.0 { whole + 1.0 } else { whole - 1.0 }
    } else {
        whole
    }
}

/// `Get*Record(index)`: `begin + index * size` when `index < count` (a signed compare, so a negative index gives an
/// address before the table), else 0.
pub fn record(bounds: crate::TableBounds, table: Table, index: i32) -> u32 {
    if index < table_size(bounds, table) {
        bounds
            .begin
            .wrapping_add(index.wrapping_mul(table.record_size()).cast_unsigned())
    } else {
        0
    }
}

/// `Get*TableSize()`: `(end - begin) / size`, a signed division.
fn table_size(bounds: crate::TableBounds, table: Table) -> i32 {
    bounds
        .end
        .wrapping_sub(bounds.begin)
        .cast_signed()
        .wrapping_div(table.record_size())
}

/// `GetSpellIndex` / `GetRuneIndex(record)`: the index of the record at exactly that address, or -1.
fn record_index(bounds: crate::TableBounds, table: Table, record_addr: u32) -> i32 {
    (0..table_size(bounds, table))
        .find(|&i| record(bounds, table, i) == record_addr)
        .unwrap_or(-1)
}

/// One `TempDumpVars` line: `Name: "%s" = %.2f` with the float widened, as the original formats it.
fn dump_var_line(name: &[u8], value: f32) -> Vec<u8> {
    let value = f64::from(value);
    // The spellings of the MSVC runtime the original links (ucrt) for what Rust writes as `inf` and `NaN`.
    let number = if value.is_nan() {
        if value.is_sign_negative() {
            "-nan(ind)".to_string()
        } else {
            "nan".to_string()
        }
    } else if value.is_infinite() {
        if value > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        }
    } else {
        format!("{value:.2}")
    };
    [b"Name: \"", name, b"\" = ", number.as_bytes()].concat()
}

/// A perk record's unsigned modifier as `GetPerkModifierFor*` pushes it: the original stores it in a float before
/// widening it, so large values lose their low bits.
fn perk_modifier(value: u32) -> f64 {
    f64::from(value as f32)
}

/// The integer quotient for `IDiv`. Division by zero and `INT_MIN / -1` raise the CPU's divide error in the
/// original, which ends the game; here they panic, which ends it too (`panic = "abort"`).
fn idiv(a: i32, b: i32) -> i32 {
    match a.checked_div(b) {
        Some(q) => q,
        None => panic!("IDiv({a}, {b}): integer divide error"),
    }
}

/// The remainder for `IMod`, with the sign of `a`; the same faults as [`idiv`].
fn imod(a: i32, b: i32) -> i32 {
    match a.checked_rem(b) {
        Some(r) => r,
        None => panic!("IMod({a}, {b}): integer divide error"),
    }
}

const TABLES: [(Table, &str, &str); 7] = [
    (Table::Perks, "GetPerkRecord", "GetPerksTableSize"),
    (Table::Weapons, "GetWeaponRecord", "GetWeaponsTableSize"),
    (Table::Armors, "GetArmorRecord", "GetArmorsTableSize"),
    (
        Table::Materials,
        "GetMaterialRecord",
        "GetMaterialsTableSize",
    ),
    (Table::Quicks, "GetQuickRecord", "GetQuicksTableSize"),
    (Table::Spells, "GetSpellRecord", "GetSpellsTableSize"),
    (Table::Runes, "GetRuneRecord", "GetRunesTableSize"),
];

/// Registers all 57 functions as globals.
pub(crate) fn register_all(lua: &Lua, game: &Rc<dyn Game>) -> mlua::Result<()> {
    register(lua, "Round", |_, a| Ok(round(a.number(1)?)))?;
    let g = game.clone();
    register(lua, "GetEffectValue", move |_, a| {
        let unit = lua_to_ptr(a.int(1)?);
        Ok(f64::from(g.effect_value(unit, a.int(2)?)))
    })?;
    let g = game.clone();
    register(lua, "GetPerkModifierForServer", move |_, a| {
        let stats = lua_to_ptr(a.int(1)?);
        Ok(perk_modifier(g.perk_modifier_for_server(stats, a.int(2)?)))
    })?;
    let g = game.clone();
    register(lua, "GetPerkModifierForClient", move |_, a| {
        let params = lua_to_ptr(a.int(1)?);
        Ok(perk_modifier(g.perk_modifier_for_client(params, a.int(2)?)))
    })?;
    let g = game.clone();
    register(lua, "Heal", move |_, a| {
        let stats = lua_to_ptr(a.int(1)?);
        let vitals = lua_to_ptr(a.int(2)?);
        g.heal(stats, vitals, a.number(3)?);
        Ok(())
    })?;
    let g = game.clone();
    register(lua, "RegenMp", move |_, a| {
        let vitals = lua_to_ptr(a.int(1)?);
        g.regen_mp(vitals, a.number(2)?);
        Ok(())
    })?;
    let g = game.clone();
    register(lua, "DistributeHp", move |_, a| {
        let stats = lua_to_ptr(a.int(1)?);
        g.distribute_hp(stats, a.number(2)?);
        Ok(())
    })?;
    let g = game.clone();
    register(lua, "CollectHp", move |_, a| {
        let stats = lua_to_ptr(a.int(1)?);
        Ok(f64::from(g.collect_hp(stats, a.number(2)?)))
    })?;
    for (table, getter, _) in TABLES {
        let g = game.clone();
        register(lua, getter, move |_, a| {
            let index = a.int(1)?;
            Ok(ptr_to_lua(record(g.table(table), table, index)))
        })?;
    }
    for (table, _, size) in TABLES {
        let g = game.clone();
        register(lua, size, move |_, _| Ok(table_size(g.table(table), table)))?;
    }
    let g = game.clone();
    register(lua, "GetPerkIndex", move |_, a| {
        Ok(g.perk_index(lua_to_ptr(a.int(1)?)))
    })?;
    for (table, name) in [
        (Table::Spells, "GetSpellIndex"),
        (Table::Runes, "GetRuneIndex"),
    ] {
        let g = game.clone();
        register(lua, name, move |_, a| {
            let addr = lua_to_ptr(a.int(1)?);
            Ok(record_index(g.table(table), table, addr))
        })?;
    }
    let g = game.clone();
    register(lua, "GetClassId", move |_, a| {
        Ok(g.class_id(lua_to_ptr(a.int(1)?)))
    })?;
    let g = game.clone();
    register(lua, "GetItemsWeight", move |_, a| {
        Ok(f64::from(g.items_weight(lua_to_ptr(a.int(1)?))))
    })?;
    let g = game.clone();
    register(lua, "GetItemShapeEntry", move |_, a| {
        Ok(ptr_to_lua(g.item_shape_entry(lua_to_ptr(a.int(1)?))))
    })?;
    for (kind, name) in [
        (SlotKind::Weapon, "GetWeapon"),
        (SlotKind::Armor, "GetArmor"),
        (SlotKind::Quick, "GetQuick"),
        (SlotKind::Spell, "GetSpell"),
    ] {
        let g = game.clone();
        register(lua, name, move |_, a| {
            let unit = lua_to_ptr(a.int(1)?);
            Ok(ptr_to_lua(g.slot_item(unit, kind, a.int(2)?)))
        })?;
    }
    let g = game.clone();
    register(lua, "ConsoleWriteLine", move |_, a| {
        g.console_write_line(&a.bytes(1)?);
        Ok(())
    })?;
    register(lua, "IDiv", |_, a| {
        let x = a.int(1)?;
        Ok(idiv(x, a.int(2)?))
    })?;
    register(lua, "IMod", |_, a| {
        let x = a.int(1)?;
        Ok(imod(x, a.int(2)?))
    })?;
    let g = game.clone();
    register(lua, "GetGSVar", move |_, a| {
        let unit = lua_to_ptr(a.int(1)?);
        let name = a.bytes(2)?;
        Ok(g.gs_var(unit, &name)
            .map_or(Value::Nil, |v| Value::Number(f64::from(v))))
    })?;
    let g = game.clone();
    register(lua, "SetGSVar", move |_, a| {
        let unit = lua_to_ptr(a.int(1)?);
        let name = a.bytes(2)?;
        g.set_gs_var(unit, &name, a.number(3)? as f32);
        Ok(())
    })?;
    let g = game.clone();
    register(lua, "UnitIsHost", move |_, a| {
        Ok(g.unit_is_host(lua_to_ptr(a.int(1)?)))
    })?;
    let g = game.clone();
    register(lua, "TempDumpVars", move |_, a| {
        let unit = lua_to_ptr(a.int(1)?);
        for (name, value) in g.gs_vars(unit).unwrap_or_default() {
            g.console_write_line(&dump_var_line(&name, value));
        }
        Ok(())
    })?;
    memory::register_all(lua)?;
    let g = game.clone();
    register(lua, "GetIniValue", move |lua, a| {
        let (file, section, key) = (a.bytes(1)?, a.bytes(2)?, a.bytes(3)?);
        match g.ini_value(&file, &section, &key) {
            Some(value) => Ok(Value::String(lua.create_string(value)?)),
            None => Ok(Value::Nil),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TableBounds;

    #[test]
    fn rounds_half_away_from_zero() {
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(2.49), 2.0);
        assert_eq!(round(-2.5), -3.0);
        assert_eq!(round(-0.5), -1.0);
        assert_eq!(round(0.4), 0.0);
        assert!(round(-0.4).is_sign_negative());
        assert!(round(f64::NAN).is_nan());
    }

    #[test]
    fn table_arithmetic_follows_the_signed_compares() {
        let b = TableBounds {
            begin: 0x1000,
            end: 0x1000 + 3 * 0x84,
        };
        assert_eq!(table_size(b, Table::Spells), 3);
        assert_eq!(record(b, Table::Spells, 2), 0x1000 + 2 * 0x84);
        assert_eq!(record(b, Table::Spells, 3), 0);
        assert_eq!(record(b, Table::Spells, -1), 0x1000 - 0x84);
        assert_eq!(record_index(b, Table::Spells, 0x1000 + 0x84), 1);
        assert_eq!(record_index(b, Table::Spells, 0x1001), -1);
        assert_eq!(record_index(b, Table::Spells, 0x1000 + 3 * 0x84), -1);
        let empty = TableBounds { begin: 0, end: 0 };
        assert_eq!(table_size(empty, Table::Perks), 0);
        assert_eq!(record(empty, Table::Perks, 0), 0);
    }

    #[test]
    fn dump_lines_use_two_decimals() {
        assert_eq!(dump_var_line(b"gold", 1.5), b"Name: \"gold\" = 1.50");
        assert_eq!(dump_var_line(b"x", -0.004), b"Name: \"x\" = -0.00");
        assert_eq!(dump_var_line(b"x", f32::INFINITY), b"Name: \"x\" = inf");
    }
}
