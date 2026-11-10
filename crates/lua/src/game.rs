//! What the registered functions need from the running game: one method per need, implemented by the addon.
//!
//! Pointers arrive as the 32-bit values a script passed; none is checked, as in addon.dll 0.12.4.2, where a wrong
//! address crashes the game in the access.

/// A game database table: a game `std::vector` `{begin, end}` of fixed-size records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Table {
    /// Perks: `{begin, end}` at 0x7af274, records of 0x44 bytes.
    Perks,
    /// Weapons: 0x7c86f8, 0xb4 bytes.
    Weapons,
    /// Armours: 0x7c8718, 0xc8 bytes.
    Armors,
    /// Materials: 0x7c86d8, 0x50 bytes.
    Materials,
    /// Quick items: 0x7c8738, 0x8c bytes.
    Quicks,
    /// Spells: 0x7c8510, 0x84 bytes.
    Spells,
    /// Runes: 0x7c8530, 0x24 bytes.
    Runes,
}

impl Table {
    /// The size of one record in bytes.
    pub const fn record_size(self) -> i32 {
        match self {
            Self::Perks => 0x44,
            Self::Weapons => 0xb4,
            Self::Armors => 0xc8,
            Self::Materials => 0x50,
            Self::Quicks => 0x8c,
            Self::Spells => 0x84,
            Self::Runes => 0x24,
        }
    }
}

/// The two pointers of a table's vector, read when a function needs them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableBounds {
    /// The first record.
    pub begin: u32,
    /// One past the last record.
    pub end: u32,
}

/// Which item array of a unit a slot getter reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    /// `GetWeapon`: 4 slots, unit +0x440 or party record +0x730.
    Weapon,
    /// `GetArmor`: 7 slots, unit +0x480 or party record +0x750.
    Armor,
    /// `GetQuick`: 4 slots, unit +0x450 or party record +0x740.
    Quick,
    /// `GetSpell`: 8 slots, unit +0x460 or party record +0x760.
    Spell,
}

/// The game-side work of the registered functions. Each method documents the original's semantics.
pub trait Game {
    /// `GetEffectValue(unit, type)`: `CMapObject::EffectValue(unit, type)` (0x50f690).
    fn effect_value(&self, unit: u32, effect_type: i32) -> f32;

    /// `GetPerkModifierForServer(stats, perk)`: `level` = the byte at stats+0x6d3+perk; 0 if `level` is 0; else the
    /// dword +0x34 of perk record `perk * 3 + level - 1`, or 0 if that index is out of the table.
    fn perk_modifier_for_server(&self, stats: u32, perk: i32) -> u32;

    /// `GetPerkModifierForClient(params, perk)`: as the server one with the level byte at params+0xe3+perk.
    fn perk_modifier_for_client(&self, params: u32, perk: i32) -> u32;

    /// `Heal(stats, vitals, amount)`: `UnitStats::HealSlots(stats, vitals.maxHitPoints, amount)` (0x525800), then
    /// `vitals.hitPoints = UnitStats::Reduce(stats, vitals.maxHitPoints)` (0x5259b0).
    fn heal(&self, stats: u32, vitals: u32, amount: f64);

    /// `RegenMp(vitals, fraction)`: if `mana < maxMana` (vitals +0x14, +0x18),
    /// `mana = min(maxMana, mana + maxMana * fraction)`.
    fn regen_mp(&self, vitals: u32, fraction: f64);

    /// `DistributeHp(stats, maxHp)`: for each of the 6 slots at stats+0x110 (0xf4 bytes each) with a positive
    /// `kind` (+0xe0): `maximum` (+0xe8) = `maxHp / slots[1].share * slot.share` (share +0xec), `current` (+0xe4)
    /// = `min(current, maximum)`.
    fn distribute_hp(&self, stats: u32, max_hp: f64);

    /// `CollectHp(stats, maxHp)`: `UnitStats::Reduce(stats, maxHp)` (0x5259b0).
    fn collect_hp(&self, stats: u32, max_hp: f64) -> f32;

    /// The vector of `table` as the game holds it now (empty before the databases load).
    fn table(&self, table: Table) -> TableBounds;

    /// `GetPerkIndex(record)`: `SkillDatabase::IndexOfLevel(0x7af268, record)` (0x51ff00).
    fn perk_index(&self, record: u32) -> i32;

    /// `GetClassId(obj)`: virtual slot 12 (vtable +0x30) of `obj`.
    fn class_id(&self, object: u32) -> i32;

    /// `GetItemsWeight(unit)`: the sum of `item+0x34` (a float) over the non-null pointers of `weapons[4]`
    /// (+0x440), `quickItems[4]` (+0x450), `spells[8]` (+0x460) and `armor[7]` (+0x480).
    fn items_weight(&self, unit: u32) -> f32;

    /// `GetItemShapeEntry(item)`: `CItem::GetPrototype(item)` (0x6a0110).
    fn item_shape_entry(&self, item: u32) -> u32;

    /// `GetWeapon` / `GetArmor` / `GetQuick` / `GetSpell(unit, slot)`. If the class id of `unit.controller`
    /// (+0x240) is 0x3d, the item comes from the party record in `unit.player` (+0x24c) whose +0x1c equals `unit.id`
    /// (+0xc): its array at the kind's offset (pointer, then count), `slot` checked unsigned against the count.
    /// Otherwise the unit's own array, `slot` checked unsigned against the slot count. Out of range, no player or no
    /// record: 0.
    fn slot_item(&self, unit: u32, kind: SlotKind, slot: i32) -> u32;

    /// `UnitIsHost(unit)`: `p = CWorldServer::GetPlayer(TheServerWorld, 0)` (0x594180); true when `unit.id` (+0xc)
    /// equals the +0x1c of one of `p`'s party records (at p+0xa0, count p+0xa4, 0x784 bytes each); false if `p` is
    /// null.
    fn unit_is_host(&self, unit: u32) -> bool;

    /// Writes `text`, then `\n`, then flushes, on the console's ostream at 0x7c55a8. `ConsoleWriteLine` and each
    /// line of `TempDumpVars` come here.
    fn console_write_line(&self, text: &[u8]);

    /// `GetGSVar(unit, name)`: if `unit.player` (+0x24c) is set, the value of `name` in the player's variable table
    /// at player+0xc4 (case-sensitive). `None` when there is no player or no such variable.
    fn gs_var(&self, unit: u32, name: &[u8]) -> Option<f32>;

    /// `SetGSVar(unit, name, value)`: if `unit.player` is set, `NamedFloatTable::Set(player+0xc4, name, value)`
    /// (0x460bc0); else nothing.
    fn set_gs_var(&self, unit: u32, name: &[u8], value: f32);

    /// `TempDumpVars(unit)`: every variable of the player's table in the table's walk order, or `None` when `unit.player` is not set.
    fn gs_vars(&self, unit: u32) -> Option<Vec<(Vec<u8>, f32)>>;

    /// `GetIniValue(file, section, key)`, an addition to the original's functions: the value of `key` in
    /// `[section]` of the INI file `file`, a path below the mod folder. `None` when the path is refused or the file,
    /// section or key is missing.
    fn ini_value(&self, file: &[u8], section: &[u8], key: &[u8]) -> Option<Vec<u8>>;
}
