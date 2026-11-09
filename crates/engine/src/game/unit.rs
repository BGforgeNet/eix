//! Map objects and units: `CMapObject`, `CUnit`, their server halves and the statistics block.
//!
//! Names follow ei-rebuild.
#![expect(unsafe_code, reason = "calls into game.exe's unit code")]

use core::ffi::c_void;
use core::mem::offset_of;

use super::code;
use super::item::Item;
use super::mfc::{GameArray, GameString};
use super::object::{EngineObject, TransformObject, Vector3};
use super::spell::Spell;

/// The vitals a map object keeps in whole numbers for the client: `UnitVitals::Pack` 0x522C50 rounds the server's
/// into them (from `CMapObject::PackVitals` 0x50E970).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedVitals {
    _unknown0: [u8; 2],
    /// Hit points.
    pub hit_points: u16,
    /// Maximum hit points.
    pub max_hit_points: u16,
    /// Mana: `CUnit::CanCastSpell` 0x52F820 compares it with the spell's cost.
    pub mana: u16,
    /// Maximum mana.
    pub max_mana: u16,
}

const _: () = {
    assert!(offset_of!(PackedVitals, hit_points) == 0x2);
    assert!(offset_of!(PackedVitals, max_hit_points) == 0x4);
    assert!(offset_of!(PackedVitals, mana) == 0x6);
    assert!(offset_of!(PackedVitals, max_mana) == 0x8);
    assert!(size_of::<PackedVitals>() == 0xA);
};

/// `CMapObject`: an object on the map. `CMapObjectManager::CreateObject` 0x512730 allocates 0x238 bytes for one
/// (0x5127A6).
#[repr(C)]
#[derive(Debug)]
pub struct MapObject {
    /// Position and rotation.
    pub transform: TransformObject,
    _unknown54: [u8; 0xC4 - 0x54],
    /// The figure's scale: per raise `UnitStats::RaiseStrength` 0x529090 adds 0.2 to x and `RaiseDexterity` 0x529220
    /// takes 0.2 from y (+0xC8), stopping at 0.
    pub scale: Vector3,
    _unknownd0: [u8; 0x170 - 0xD0],
    /// The server or client half: `PackVitals` 0x50E970 reads the vitals from it when its class id is
    /// [`super::object::CLASS_MAP_OBJECT_SERVER`] ([`MapObjectServer`]).
    pub companion: *mut EngineObject,
    /// Updates left before the object may leave the server's update map: `CWorldServer::UpdateUnits` 0x58D670
    /// counts it down (0x58D8AD) and lets the object sleep at 0.
    pub update_countdown: i32,
    /// The time factor of the object's animation: `CUnitClientSpecific::AdvanceTime` 0x535C7A multiplies the
    /// animation's time step by it, and 0x535C5E resets it to 1.0.
    pub animation_speed: f32,
    /// The client's copy of the vitals.
    pub packed_vitals: PackedVitals,
    _unknown186: [u8; 0x194 - 0x186],
    /// The active spell effects: `CMapObject::AddEffectImpl` 0x50F1E0 grows the array by one (`SetSize` 0x50FE70 on
    /// +0x194) and copies the effect into the new last element; `CMapObject::EffectValue` 0x50F690 walks its data
    /// (+0x198) for its length (+0x19C).
    pub effects: GameArray<MapEffect>,
    _unknown1a8: [u8; 0x1AC - 0x1A8],
    /// The scale the figure grows from: `RaiseStrength` 0x529090 adds 0.2 to x, `RaiseDexterity` 0x529220 takes 0.2
    /// from y, stopping at 0.
    pub base_scale: Vector3,
    _unknown1b8: [u8; 0x238 - 0x1B8],
}

const _: () = {
    assert!(offset_of!(MapObject, transform) == 0x0);
    assert!(offset_of!(MapObject, scale) == 0xC4);
    assert!(offset_of!(MapObject, companion) == 0x170);
    assert!(offset_of!(MapObject, update_countdown) == 0x174);
    assert!(offset_of!(MapObject, animation_speed) == 0x178);
    assert!(offset_of!(MapObject, packed_vitals) == 0x17C);
    assert!(offset_of!(MapObject, effects) == 0x194);
    assert!(offset_of!(MapObject, base_scale) == 0x1AC);
    assert!(size_of::<MapObject>() == 0x238);
};

/// The bit of [`EngineObject::flags`] that marks a map object for removal: `CWorldServer::UpdateUnits` 0x58D670
/// collects the ids of the objects that have it (0x58D7BC).
pub const FLAG_REMOVED: u32 = 0x1;

/// An active spell effect on a map object, copied as five words by `CMapObject::AddEffectImpl` 0x50F1E0
/// (`rep movsd`, ecx 5) and saved field by field by `MapEffect::Save` 0x6885A0.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapEffect {
    /// The spell type: `AddEffectImpl` dispatches on it (0x50F247) and stuns for 0x19 and 0x1C (0x50F277).
    pub kind: u8,
    _unknown1: [u8; 3],
    /// Updates left: `MapEffect::Tick` 0x688530 counts it down and answers whether any are left.
    pub ticks: i32,
    /// The effect's value (a float; `Save` writes it after the ticks).
    pub value: f32,
    /// Its strength (a float).
    pub strength: f32,
    /// The end flag; `Save` leaves it out.
    pub ending: u8,
    _unknown11: [u8; 3],
}

const _: () = {
    assert!(offset_of!(MapEffect, kind) == 0x0);
    assert!(offset_of!(MapEffect, ticks) == 0x4);
    assert!(offset_of!(MapEffect, value) == 0x8);
    assert!(offset_of!(MapEffect, strength) == 0xC);
    assert!(offset_of!(MapEffect, ending) == 0x10);
    assert!(size_of::<MapEffect>() == 0x14);
};

impl MapObject {
    /// The value of the effect `kind` on the object, 0 when it has none: `CMapObject::EffectValue` 0x50F690
    /// (thiscall, `ret 4`).
    ///
    /// # Safety
    /// `this` must be a live map object of game.exe RU 1.06, on the game's thread.
    pub unsafe fn effect_value(this: *mut Self, kind: i32) -> f32 {
        // SAFETY: 0x50F690 is `float __thiscall (int type)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, i32) -> f32>(
                eix_sites::C_MAP_OBJECT_EFFECT_VALUE,
            )
        };
        // SAFETY: the caller guarantees the object and the thread.
        unsafe { f(this, kind) }
    }

    /// Marks `part` of the object changed for the network (bit `part` of +0x1DC, a negative part none) and the
    /// object dirty: `CMapObject::MarkChanged` 0x50E9F0 (thiscall, `ret 4`).
    ///
    /// # Safety
    /// As for [`effect_value`](Self::effect_value).
    pub unsafe fn mark_changed(this: *mut Self, part: i32) {
        // SAFETY: 0x50E9F0 is `void __thiscall (int part)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, i32)>(
                eix_sites::C_MAP_OBJECT_MARK_CHANGED,
            )
        };
        // SAFETY: the caller guarantees the object and the thread.
        unsafe { f(this, part) }
    }
}

/// A map object's server half (`CMapObjectServer`), up to its vitals.
#[repr(C)]
#[derive(Debug)]
pub struct MapObjectServer {
    /// The engine object.
    pub base: EngineObject,
    _unknown1c: [u8; 0x20 - 0x1C],
    /// Hit points and mana: `CUnitServer::Heal` 0x5543B0 heals the unit's companion +0x20.
    pub vitals: UnitVitals,
}

const _: () = {
    assert!(offset_of!(MapObjectServer, vitals) == 0x20);
};

/// `UnitVitals`: hit points and mana as floats. `UnitVitals::Reset` 0x523370 clears 0x58 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitVitals {
    _unknown0: [u8; 4],
    /// Updates since the last regeneration: `UnitVitals::Regenerate` 0x523740 counts it up to 15.
    pub regen_ticks: i32,
    /// Hit points: `Pack` 0x522C50 rounds it into [`PackedVitals::hit_points`].
    pub hit_points: f32,
    /// Maximum hit points (`Pack`).
    pub max_hit_points: f32,
    _unknown10: [u8; 4],
    /// Mana (`Pack`; `Regenerate` raises it to the maximum at most).
    pub mana: f32,
    /// Maximum mana (`Pack`).
    pub max_mana: f32,
    /// Mana regained per regeneration, a fraction of the maximum (`Regenerate`).
    pub mana_regen: f32,
    /// The protection against each of the seven damage types (ei-rebuild's `UnitStatPair protection[7]`; the
    /// addon's `GiveUnitArmor` sets both halves).
    pub protection: [StatPair; 7],
}

/// A value and its current, modified form (ei-rebuild's `UnitStatPair`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatPair {
    /// The value.
    pub base: f32,
    /// What it currently is.
    pub current: f32,
}

const _: () = {
    assert!(offset_of!(StatPair, base) == 0x0);
    assert!(offset_of!(StatPair, current) == 0x4);
};

const _: () = {
    assert!(offset_of!(UnitVitals, regen_ticks) == 0x4);
    assert!(offset_of!(UnitVitals, hit_points) == 0x8);
    assert!(offset_of!(UnitVitals, max_hit_points) == 0xC);
    assert!(offset_of!(UnitVitals, mana) == 0x14);
    assert!(offset_of!(UnitVitals, max_mana) == 0x18);
    assert!(offset_of!(UnitVitals, mana_regen) == 0x1C);
    assert!(offset_of!(UnitVitals, protection) == 0x20);
    assert!(size_of::<UnitVitals>() == 0x58);
};

/// `CUnit`. `CMapObjectManager::CreateObject` 0x512730 allocates 0x4DC bytes for one (0x51276D).
#[repr(C)]
#[derive(Debug)]
pub struct Unit {
    /// The map object.
    pub map_object: MapObject,
    /// The unit's script name: virtual slot 7, `UnitGetName` 0x446820.
    pub script_name: GameString,
    _unknown23c: [u8; 0x240 - 0x23C],
    /// The controller, a [`UnitServer`] on the server: `CUnit::AttachObjectImpl` 0x52E870 stores it and points its
    /// +0x1C back at the unit.
    pub controller: *mut EngineObject,
    /// The sides the unit is hostile to, a bit per side: `CDiplomacy::SetRelation` 0x4554E0 rewrites it for a side's
    /// units; the script `IsEnemy` (0x4B4985) and `CUnit::IsHostileTo` 0x530150 test a side's bit in it.
    pub hostile: u32,
    /// A second side mask the same tests AND with [`hostile`](Self::hostile) (all bits by default).
    pub hostile_limit: u32,
    /// The player that owns the unit, or null: the slot getters (`CUnit::GetWeapon` 0x5318B0) then read the player's
    /// items instead of the unit's.
    pub player: *mut c_void,
    /// The unit's side: `CUnit::SetSide` 0x530120.
    pub side: i32,
    _unknown254: [u8; 0x25C - 0x254],
    /// The motion mode: 1 is paused (`CUnitClientSpecific::AdvanceTime` 0x535C92 then skips the animation step);
    /// `CUnit::ReadNetState` 0x52F3A9 compares it with 1.
    pub motion_mode: i32,
    _unknown260: [u8; 0x26C - 0x260],
    /// The weapon slot in use: `CUnit::HasRangedWeapon` 0x530FA0 indexes the weapons with it.
    pub active_weapon: u32,
    /// The weapon slot to take up next (ei-rebuild's `pendingWeapon`): `CMapObjectManager::GiveStartingEquipment`
    /// 0x51430A stores the slot `AddWeapon` returned.
    pub pending_weapon: i32,
    /// Non-zero when dead: virtual slot 55, `UnitGetFlag274` 0x446880.
    pub dead: i32,
    _unknown278: [u8; 0x330 - 0x278],
    /// The client's rounded copy of the statistics: `UnitStats::Pack` 0x522CE0 fills it (`CUnit::SetFromStats`
    /// 0x532A20 passes +0x330).
    pub params: UnitParams,
    _unknown438: [u8; 0x440 - 0x438],
    /// Weapon slots: `CUnit::GetWeapon` 0x5318B0; `CUnit::TotalItemWeight` 0x5307B0 sums them.
    pub weapons: [*mut Item; 4],
    /// Quick item slots: `CUnit::GetQuickItem` 0x531920 (and 0x5307B0).
    pub quick_items: [*mut Item; 4],
    /// Spell slots: `CUnit::GetSpell` 0x5319A0 (and 0x5307B0).
    pub spells: [*mut Item; 8],
    /// Armour pieces, one per armour type: 0x5307B0 sums the seven.
    pub armor: [*mut Item; 7],
    /// Further items: `CUnit::ReleaseUnit` 0x52E740 releases each and frees the block.
    pub carried: GameArray<*mut Item>,
    _unknown4b0: [u8; 0x4DC - 0x4B0],
}

const _: () = {
    assert!(offset_of!(Unit, map_object) == 0x0);
    assert!(offset_of!(Unit, script_name) == 0x238);
    assert!(offset_of!(Unit, controller) == 0x240);
    assert!(offset_of!(Unit, hostile) == 0x244);
    assert!(offset_of!(Unit, hostile_limit) == 0x248);
    assert!(offset_of!(Unit, player) == 0x24C);
    assert!(offset_of!(Unit, side) == 0x250);
    assert!(offset_of!(Unit, motion_mode) == 0x25C);
    assert!(offset_of!(Unit, active_weapon) == 0x26C);
    assert!(offset_of!(Unit, pending_weapon) == 0x270);
    assert!(offset_of!(Unit, dead) == 0x274);
    assert!(offset_of!(Unit, params) == 0x330);
    assert!(offset_of!(Unit, weapons) == 0x440);
    assert!(offset_of!(Unit, quick_items) == 0x450);
    assert!(offset_of!(Unit, spells) == 0x460);
    assert!(offset_of!(Unit, armor) == 0x480);
    assert!(offset_of!(Unit, carried) == 0x49C);
    assert!(size_of::<Unit>() == 0x4DC);
};

/// The item slot getters: thiscall, the slot on the stack (`ret 4`), null for an empty or out-of-range slot.
type SlotGetter<T> = unsafe extern "thiscall" fn(*mut Unit, u32) -> *mut T;

impl Unit {
    /// The weapon in `slot` (0..4): `CUnit::GetWeapon` 0x5318B0. A player's unit answers from the player's record,
    /// others from [`weapons`](Self::weapons).
    ///
    /// # Safety
    /// `this` must be a live unit of game.exe RU 1.06 with a controller, on the game's thread.
    pub unsafe fn weapon(this: *mut Self, slot: u32) -> *mut Item {
        // SAFETY: 0x5318B0 is `CItem* __thiscall (unsigned slot)`, `ret 4`.
        let f = unsafe { code::<SlotGetter<Item>>(eix_sites::C_UNIT_GET_WEAPON) };
        // SAFETY: the caller guarantees the unit and the thread; the function checks the slot.
        unsafe { f(this, slot) }
    }

    /// The spell of the spell item in `slot` (the item's +0x44): `CUnit::GetSpellObject` 0x531A10, null when there is
    /// none.
    ///
    /// # Safety
    /// As for [`weapon`](Self::weapon).
    pub unsafe fn spell(this: *mut Self, slot: u32) -> *mut Spell {
        // SAFETY: 0x531A10 has the signature of 0x5318B0 and returns the slot item's `CSpell*`.
        let f = unsafe { code::<SlotGetter<Spell>>(eix_sites::C_UNIT_GET_SPELL_OBJECT) };
        // SAFETY: as for `weapon`.
        unsafe { f(this, slot) }
    }
}

/// `UnitStats`: experience, attributes and skills. Written to a save as 0x704 bytes (0x5287EC).
#[repr(C)]
#[derive(Debug)]
pub struct UnitStats {
    /// Build flags: bit 0 is set once the stats are built from the prototype.
    pub built_flags: u32,
    /// Experience: `CUnitServer::Level` 0x556760 takes its logarithm.
    pub experience: f32,
    /// Experience spent: `UnitStats::Pack` 0x522CE0 packs experience + pending - spent as the unspent amount.
    pub spent_experience: f32,
    /// Strength before effects: `UnitStats::RaiseStrength` 0x529090 adds the raise to it.
    pub strength_base: f32,
    /// Strength: `UnitStats::StrengthRatio` 0x523CB0.
    pub strength: f32,
    /// Dexterity before effects: `UnitStats::RaiseDexterity` 0x529220 adds the raise to it.
    pub dexterity_base: f32,
    /// Dexterity: `UnitStats::DexterityRatio` 0x523CC0.
    pub dexterity: f32,
    /// Intelligence before effects: `UnitStats::RaiseIntelligence` 0x529400 adds the raise to it.
    pub intelligence_base: f32,
    /// Intelligence: `UnitStats::AddScaledExperience` 0x523960 scales experience by `1.04^(intelligence - 25)`.
    pub intelligence: f32,
    _unknown24: [u8; 0x2C - 0x24],
    /// The combat rate before modifiers (ei-rebuild's `rateBase`; the addon's `GetActionsBase` reads it).
    pub rate_base: f32,
    /// The combat rate: `CUnitServer::ScaledAttackValue` 0x54B9C0 scales by `15 / rate` (`CUnitServer`+0x210).
    pub rate: f32,
    _unknown34: [u8; 0x50 - 0x34],
    /// Weight carried (ei-rebuild's `load`; the addon's `GetEncumbrance` reads it).
    pub load: f32,
    /// Weight that can be carried (ei-rebuild's `capacity`; the addon's `GetMaxEncumbrance` reads it).
    pub capacity: f32,
    _unknown58: [u8; 0x70 - 0x58],
    /// Reach, attack, attack spread, defence and speed: `CUnitServer::GetAttackParams` 0x555BA8 copies `combat[2]`
    /// (the controller's +0x258) into [`super::combat::AttackParams::attack_spread`].
    pub combat: [f32; 5],
    /// Damage without a weapon (ei-rebuild's `damage`; the addon's `GiveUnitWeapon` copies a weapon's).
    pub damage: f32,
    /// Its spread (ei-rebuild's `damageSpread`).
    pub damage_spread: f32,
    /// The attack interval without a weapon: `ScaledAttackValue` truncates it when given no weapon (0x54DB7B,
    /// `CUnitServer`+0x26C).
    pub natural_attack_interval: f32,
    /// The natural attack's weights per damage type (ei-rebuild's `damageWeights`).
    pub damage_weights: [f32; 7],
    /// The natural attack's weapon type (ei-rebuild's `naturalWeapon`).
    pub natural_weapon: u32,
    _unknownb0: [u8; 0x110 - 0xB0],
    /// The six hit-point slots (body parts): `UnitStats::Reduce` 0x5259B0 walks them.
    pub slots: [UnitStatSlot; 6],
    /// Attribute bytes: `UnitStats::AttributeBase` 0x525DF0.
    pub attributes: [u8; 3],
    /// Skill bytes: `UnitStats::SkillBase` 0x527030.
    pub skills: [u8; 8],
    /// Perk levels, 1-based, 0 for none: `UnitStats::Pack` 0x522CE0 copies them into [`UnitParams::skill_levels`].
    pub skill_levels: [u8; 0x25],
    /// Flags; 8 is "alive": `UnitStats::RecomputeHitPoints` 0x5243D0 sets it while hit points are positive (0x524549),
    /// and `Pack` 0x522CE0 copies the low byte into [`UnitParams::flags`].
    pub flags: u32,
    /// The maximum hit points of a unit without a text key: `RecomputeHitPoints` 0x5243D0 uses it for them.
    pub fixed_max_hit_points: f32,
    /// Experience not yet added: `UnitStats::AddExperience` 0x523920 adds it to `experience` once it turns positive.
    pub pending_experience: f32,
}

const _: () = {
    assert!(offset_of!(UnitStats, built_flags) == 0x0);
    assert!(offset_of!(UnitStats, experience) == 0x4);
    assert!(offset_of!(UnitStats, spent_experience) == 0x8);
    assert!(offset_of!(UnitStats, strength_base) == 0xC);
    assert!(offset_of!(UnitStats, strength) == 0x10);
    assert!(offset_of!(UnitStats, dexterity_base) == 0x14);
    assert!(offset_of!(UnitStats, dexterity) == 0x18);
    assert!(offset_of!(UnitStats, intelligence_base) == 0x1C);
    assert!(offset_of!(UnitStats, intelligence) == 0x20);
    assert!(offset_of!(UnitStats, rate_base) == 0x2C);
    assert!(offset_of!(UnitStats, rate) == 0x30);
    assert!(offset_of!(UnitStats, load) == 0x50);
    assert!(offset_of!(UnitStats, capacity) == 0x54);
    assert!(offset_of!(UnitStats, combat) == 0x70);
    assert!(offset_of!(UnitStats, damage) == 0x84);
    assert!(offset_of!(UnitStats, damage_spread) == 0x88);
    assert!(offset_of!(UnitStats, natural_attack_interval) == 0x8C);
    assert!(offset_of!(UnitStats, damage_weights) == 0x90);
    assert!(offset_of!(UnitStats, natural_weapon) == 0xAC);
    assert!(offset_of!(UnitStats, slots) == 0x110);
    assert!(offset_of!(UnitStats, attributes) == 0x6C8);
    assert!(offset_of!(UnitStats, skills) == 0x6CB);
    assert!(offset_of!(UnitStats, skill_levels) == 0x6D3);
    assert!(offset_of!(UnitStats, flags) == 0x6F8);
    assert!(offset_of!(UnitStats, fixed_max_hit_points) == 0x6FC);
    assert!(offset_of!(UnitStats, pending_experience) == 0x700);
    assert!(size_of::<UnitStats>() == 0x704);
};

/// One hit-point slot of [`UnitStats`] (0xF4 bytes: `UnitStats::Reduce` 0x5259B0 steps by it).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitStatSlot {
    _unknown0: [u8; 8],
    /// Its three armour layers (ei-rebuild's `UnitStatLayer layers[3]`; the addon's `GetArmor` sums them).
    pub layers: [UnitStatLayer; 3],
    /// The slot is in use when positive: `Reduce` 0x5259B0 skips a slot below 1, `ScaleSlotMaxima` 0x525A70 one
    /// not above 0.
    pub kind: i32,
    /// Hit points of the slot (`Reduce`).
    pub current: f32,
    /// Maximum hit points of the slot (`Reduce` divides by it).
    pub maximum: f32,
    /// The slot's share of the unit's hit points: `ScaleSlotMaxima` 0x525A70 sets `maximum` from
    /// `max / slots[1].share * share`.
    pub share: f32,
    _unknownf0: [u8; 0xF4 - 0xF0],
}

/// An armour layer of a body slot (ei-rebuild's `UnitStatLayer`, 0x48 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitStatLayer {
    _unknown0: [u8; 4],
    /// What the layer takes from each of the seven damage types.
    pub absorb: [f32; 7],
    _unknown20: [u8; 0x48 - 0x20],
}

const _: () = {
    assert!(offset_of!(UnitStatLayer, absorb) == 0x4);
    assert!(size_of::<UnitStatLayer>() == 0x48);
    assert!(offset_of!(UnitStatSlot, layers) == 0x8);
    assert!(offset_of!(UnitStatSlot, kind) == 0xE0);
    assert!(offset_of!(UnitStatSlot, current) == 0xE4);
    assert!(offset_of!(UnitStatSlot, maximum) == 0xE8);
    assert!(offset_of!(UnitStatSlot, share) == 0xEC);
    assert!(size_of::<UnitStatSlot>() == 0xF4);
};

/// `UnitParams`: the client's rounded statistics. `UnitParams::Reset` 0x523550 clears its 0x108 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitParams {
    _unknown0: [u8; 0x18],
    /// Reach, attack, attack spread, defence and speed as bytes: `CUnitClientSpecific::GetAttackParams` 0x538B83
    /// reads `combat[2]` into the attack parameters.
    pub combat: [u8; 5],
    _unknown1d: [u8; 0xE3 - 0x1D],
    /// Perk levels: `Reset` 0x523550 clears these 0x24 bytes; 0x527232 reads one by index.
    pub skill_levels: [u8; 0x24],
    /// The low byte of [`UnitStats::flags`] (`Reset` clears it).
    pub flags: u8,
}

const _: () = {
    assert!(offset_of!(UnitParams, combat) == 0x18);
    assert!(offset_of!(UnitParams, skill_levels) == 0xE3);
    assert!(offset_of!(UnitParams, flags) == 0x107);
    assert!(size_of::<UnitParams>() == 0x108);
};

impl UnitStats {
    /// Heals the slots by `amount` within `max_hit_points`; true when nothing was left to heal:
    /// `UnitStats::HealSlots` 0x525800 (thiscall, `ret 8`).
    ///
    /// # Safety
    /// `this` must be a live unit's statistics of game.exe RU 1.06, on the game's thread.
    pub unsafe fn heal_slots(this: *mut Self, max_hit_points: f32, amount: f32) -> bool {
        // SAFETY: 0x525800 is `bool __thiscall (float max, float amount)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, f32, f32) -> bool>(
                eix_sites::UNIT_STATS_HEAL_SLOTS,
            )
        };
        // SAFETY: the caller guarantees the statistics and the thread.
        unsafe { f(this, max_hit_points, amount) }
    }

    /// The hit points left of `max_hit_points` after the slots' losses: `UnitStats::Reduce` 0x5259B0 (thiscall,
    /// `ret 4`).
    ///
    /// # Safety
    /// As for [`heal_slots`](Self::heal_slots).
    pub unsafe fn reduce(this: *mut Self, max_hit_points: f32) -> f32 {
        // SAFETY: 0x5259B0 is `float __thiscall (float max)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, f32) -> f32>(eix_sites::UNIT_STATS_REDUCE)
        };
        // SAFETY: the caller guarantees the statistics and the thread.
        unsafe { f(this, max_hit_points) }
    }

    /// Sets each slot's maximum from `max_hit_points` and its share, keeping its ratio of hit points:
    /// `UnitStats::ScaleSlotMaxima` 0x525A70 (thiscall, `ret 4`).
    ///
    /// # Safety
    /// As for [`heal_slots`](Self::heal_slots).
    pub unsafe fn scale_slot_maxima(this: *mut Self, max_hit_points: f32) {
        // SAFETY: 0x525A70 is `void __thiscall (float max)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, f32)>(
                eix_sites::UNIT_STATS_SCALE_SLOT_MAXIMA,
            )
        };
        // SAFETY: the caller guarantees the statistics and the thread.
        unsafe { f(this, max_hit_points) }
    }

    /// Rounds the statistics into `params` for `unit`: `UnitStats::Pack` 0x522CE0 (thiscall, `ret 8`).
    ///
    /// # Safety
    /// As for [`heal_slots`](Self::heal_slots); `unit` must be the live unit `params` belongs to.
    pub unsafe fn pack(this: *mut Self, unit: *mut Unit, params: *mut UnitParams) {
        // SAFETY: 0x522CE0 is `void __thiscall (CUnit*, UnitParams*)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *mut Unit, *mut UnitParams)>(
                eix_sites::UNIT_STATS_PACK,
            )
        };
        // SAFETY: the caller guarantees the statistics, the unit and the thread.
        unsafe { f(this, unit, params) }
    }

    /// Adds `amount` experience scaled by intelligence: `UnitStats::AddScaledExperience` 0x523960 (thiscall,
    /// `ret 4`), through whatever patch its entry holds.
    ///
    /// # Safety
    /// As for [`heal_slots`](Self::heal_slots).
    pub unsafe fn add_scaled_experience(this: *mut Self, amount: f32) {
        // SAFETY: 0x523960 is `void __thiscall (float amount)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, f32)>(
                eix_sites::UNIT_STATS_ADD_SCALED_EXPERIENCE,
            )
        };
        // SAFETY: the caller guarantees the statistics and the thread.
        unsafe { f(this, amount) }
    }

    /// The value of the level the unit has of perk `perk` (its database record's +0x34), 0 without one:
    /// `UnitStats::SkillLevelValue` 0x5267E0 (thiscall, the perk on the stack, `ret 4`, the result in ST0).
    ///
    /// # Safety
    /// As for [`heal_slots`](Self::heal_slots); `perk` must index [`skill_levels`](Self::skill_levels).
    pub unsafe fn skill_level_value(this: *mut Self, perk: u32) -> f32 {
        // SAFETY: 0x5267E0 is `float __thiscall (int perk)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, u32) -> f32>(
                eix_sites::UNIT_STATS_SKILL_LEVEL_VALUE,
            )
        };
        // SAFETY: the caller guarantees the statistics, the perk and the thread.
        unsafe { f(this, perk) }
    }

    /// The database record of the level the unit has of perk `perk`, null without one: `UnitStats::SkillLevelRecord`
    /// 0x528270 (thiscall, the perk on the stack, `ret 4`; it reads the perk's level byte and indexes the level
    /// records by it). The record's +0x34 is the level's value as an integer.
    ///
    /// # Safety
    /// As for [`heal_slots`](Self::heal_slots); `perk` must index [`skill_levels`](Self::skill_levels).
    pub unsafe fn skill_level_record(this: *mut Self, perk: u32) -> *const u8 {
        // SAFETY: 0x528270 is `const SkillLevelRecord* __thiscall (int perk)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, u32) -> *const u8>(
                eix_sites::UNIT_STATS_SKILL_LEVEL_RECORD,
            )
        };
        // SAFETY: the caller guarantees the statistics, the perk and the thread.
        unsafe { f(this, perk) }
    }

    /// Recomputes the statistics from base, perks and effects: `UnitStats::Recompute` 0x524560 (thiscall,
    /// `ret 8`), through whatever patch its entry holds.
    ///
    /// # Safety
    /// As for [`heal_slots`](Self::heal_slots); `vitals` and `unit` must be the unit's.
    pub unsafe fn recompute(this: *mut Self, vitals: *mut UnitVitals, unit: *mut Unit) {
        // SAFETY: 0x524560 is `void __thiscall (UnitVitals*, CUnit*)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *mut UnitVitals, *mut Unit)>(
                eix_sites::UNIT_STATS_RECOMPUTE,
            )
        };
        // SAFETY: the caller guarantees the statistics, vitals, unit and thread.
        unsafe { f(this, vitals, unit) }
    }
}

/// The vitals of `unit`'s server half, null without one: the unit's companion when its class id is
/// [`super::object::CLASS_MAP_OBJECT_SERVER`], as `UnitStats::GainExperience` 0x5239B0 and the raises (0x529090)
/// find them.
///
/// # Safety
/// `unit` must be a live unit of game.exe RU 1.06, on the game's thread.
pub unsafe fn server_vitals(unit: *mut Unit) -> *mut UnitVitals {
    // SAFETY: the caller guarantees a live unit.
    let companion = unsafe { (*unit).map_object.companion };
    if companion.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: a live unit's non-null companion is a live engine object.
    if unsafe { EngineObject::class_id(companion) } != super::object::CLASS_MAP_OBJECT_SERVER {
        return core::ptr::null_mut();
    }
    let server = companion.cast::<MapObjectServer>();
    // SAFETY: the class id says the companion is a `CMapObjectServer`.
    unsafe { &raw mut (*server).vitals }
}

/// The head every unit controller shares (`CUnitServer`, `CUnitClientSpecific`): the engine object, then the unit,
/// which `CUnit::AttachObjectImpl` 0x52E870 stores at +0x1C (`CUnitClientSpecific::GetAttackParams` reads it at
/// 0x538B74).
#[repr(C)]
#[derive(Debug)]
pub struct UnitController {
    /// The engine object.
    pub base: EngineObject,
    /// The unit it controls.
    pub unit: *mut Unit,
}

const _: () = {
    assert!(offset_of!(UnitController, unit) == 0x1C);
};

/// `CUnitServer`: a unit's server controller. Allocated as 0xB20 bytes (0x4D6A4A).
#[repr(C)]
#[derive(Debug)]
pub struct UnitServer {
    /// The engine object.
    pub base: EngineObject,
    /// The unit: `CUnitServer::Heal` 0x5543B0 reads its companion through it.
    pub unit: *mut Unit,
    _unknown20: [u8; 0x190 - 0x20],
    /// The action in progress: 0x54AACE resets it with `UnitAction::Reset` 0x54B230.
    pub action: UnitAction,
    _unknown1b4: [u8; 0x1E0 - 0x1B4],
    /// The statistics: `Heal` 0x5543B0 passes +0x1E0 to `UnitStats::Heal`.
    pub stats: UnitStats,
    _unknown8e4: [u8; 0x980 - 0x8E4],
    /// The AI (ei-rebuild's `AIBrain brain`, 0x190 bytes).
    pub brain: AiBrain,
    _unknownb10: [u8; 0xB20 - 0xB10],
}

const _: () = {
    assert!(offset_of!(UnitServer, unit) == 0x1C);
    assert!(offset_of!(UnitServer, action) == 0x190);
    assert!(offset_of!(UnitServer, stats) == 0x1E0);
    assert!(offset_of!(UnitServer, brain) == 0x980);
    assert!(size_of::<UnitServer>() == 0xB20);
};

/// A unit's AI (`AIBrain`), as far as the addon's `ResetTarget` reaches.
#[repr(C)]
#[derive(Debug)]
pub struct AiBrain {
    _unknown0: [u8; 0x36],
    /// The alarm record in use (ei-rebuild's `alarmSlot`, a signed byte).
    pub alarm_slot: i8,
    _unknown37: [u8; 0x90 - 0x37],
    /// The behaviour records, 0x4C bytes each (ei-rebuild's `alarmRecords`, set from the .mob file).
    pub alarm_records: GameArray<[u8; 0x4C]>,
    _unknowna4: [u8; 0xD0 - 0xA4],
    /// What the unit perceives (ei-rebuild's `AIMemory memory`).
    pub memory: AiMemory,
    _unknown118: [u8; 0x190 - 0x118],
}

/// The perception memory of an [`AiBrain`], up to the units it sees.
#[repr(C)]
#[derive(Debug)]
pub struct AiMemory {
    _unknown0: [u8; 0x44],
    /// The units the unit sees, a chain from its head: `AIMemory::IsEnemy` 0x5CB7F0 walks it (0x5CB81A), the script
    /// `PlayerSee` adds each party unit's (0x4B5D5A).
    pub visible: *mut SeenNode,
}

/// A node of [`AiMemory::visible`].
#[repr(C)]
#[derive(Debug)]
pub struct SeenNode {
    /// The next node, or null (0x5CB826).
    pub next: *mut SeenNode,
    /// The previous node: `AIViewer::See` links a new node at the head (0x5CA927).
    pub prev: *mut SeenNode,
    /// The unit seen (0x5CB821).
    pub unit: *mut Unit,
}

const _: () = {
    assert!(offset_of!(AiBrain, alarm_slot) == 0x36);
    assert!(offset_of!(AiBrain, alarm_records) == 0x90);
    assert!(offset_of!(AiBrain, memory) == 0xD0);
    assert!(size_of::<AiBrain>() == 0x190);
    assert!(offset_of!(AiMemory, visible) == 0x44);
    assert!(offset_of!(SeenNode, unit) == 0x8);
};

impl AiBrain {
    /// Applies behaviour record `index` (`record` is its address): `AIBrain::ApplyBehaviour` 0x5D6630 (thiscall,
    /// the record and the index on the stack, `ret 8`).
    ///
    /// # Safety
    /// `this` must be a live unit's brain of game.exe RU 1.06 and `record` its record `index`, on the game's thread.
    pub unsafe fn apply_behaviour(this: *mut Self, record: *mut [u8; 0x4C], index: i32) {
        // SAFETY: 0x5D6630 is `void __thiscall (const BehaviourRecord*, int)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *mut [u8; 0x4C], i32)>(
                eix_sites::AI_BRAIN_APPLY_BEHAVIOUR,
            )
        };
        // SAFETY: the caller guarantees the brain, the record and the thread.
        unsafe { f(this, record, index) }
    }

    /// The object the brain's aggression is aimed at, or null: `AIBrain::AggressionEnemy` 0x5D5F20 (thiscall, plain
    /// `ret`), the enemy of its first aggression mode while that mode is in charge. `CPlayerServer::SendPartyState`
    /// calls it for its danger state (0x66E666).
    ///
    /// # Safety
    /// `this` must be a live unit's brain of game.exe RU 1.06, on the game's thread.
    pub unsafe fn aggression_enemy(this: *mut Self) -> *mut EngineObject {
        // SAFETY: 0x5D5F20 is `CEngineObject* __thiscall ()`; it reads the mode list only.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self) -> *mut EngineObject>(
                eix_sites::AI_BRAIN_AGGRESSION_ENEMY,
            )
        };
        // SAFETY: the caller guarantees the brain and the thread.
        unsafe { f(this) }
    }
}

impl AiMemory {
    /// Forgets what was perceived: `AIMemory::Reset` 0x5CB0F0 (thiscall, plain `ret`).
    ///
    /// # Safety
    /// `this` must be a live brain's memory of game.exe RU 1.06, on the game's thread.
    pub unsafe fn reset(this: *mut Self) {
        // SAFETY: 0x5CB0F0 is `void __thiscall ()`.
        let f =
            unsafe { code::<unsafe extern "thiscall" fn(*mut Self)>(eix_sites::AI_MEMORY_RESET) };
        // SAFETY: the caller guarantees the memory and the thread.
        unsafe { f(this) }
    }
}

/// A unit prototype record of the unit database, which `CUnit::GetPrototype` 0x530930 returns.
#[repr(C)]
#[derive(Debug)]
pub struct UnitPrototype {
    /// The prototype's name (the addon's `GetPrototype` returns it).
    pub name: GameString,
    /// The model's name (ei-rebuild's `modelName`; the addon's `GetRace` returns it).
    pub model: GameString,
    _unknown8: [u8; 0x58 - 0x8],
    /// The protection scale added to each protection value (ei-rebuild's `protectionScale`).
    pub protection_scale: f32,
    _unknown5c: [u8; 0x64 - 0x5C],
    /// Reach, attack, attack spread, defence (ei-rebuild's `combat`).
    pub combat: [f32; 4],
}

const _: () = {
    assert!(offset_of!(UnitPrototype, name) == 0x0);
    assert!(offset_of!(UnitPrototype, model) == 0x4);
    assert!(offset_of!(UnitPrototype, protection_scale) == 0x58);
    assert!(offset_of!(UnitPrototype, combat) == 0x64);
};

impl Unit {
    /// The unit's prototype record: `CUnit::GetPrototype` 0x530930 (thiscall, plain `ret`).
    ///
    /// # Safety
    /// `this` must be a live unit of game.exe RU 1.06, on the game's thread.
    pub unsafe fn prototype(this: *mut Self) -> *mut UnitPrototype {
        // SAFETY: 0x530930 is `const UnitPrototypeRecord* __thiscall ()`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self) -> *mut UnitPrototype>(
                eix_sites::C_UNIT_GET_PROTOTYPE,
            )
        };
        // SAFETY: the caller guarantees the unit and the thread.
        unsafe { f(this) }
    }

    /// Gives the unit a weapon: its weapon slot, or -1 when it has no room: `CUnit::AddWeapon` 0x531070 (thiscall,
    /// `ret 4`).
    ///
    /// # Safety
    /// As for [`prototype`](Self::prototype); `item` a live weapon not held elsewhere.
    pub unsafe fn add_weapon(this: *mut Self, item: *mut Item) -> i32 {
        // SAFETY: 0x531070 is `int __thiscall (CItem*)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *mut Item) -> i32>(
                eix_sites::C_UNIT_ADD_WEAPON,
            )
        };
        // SAFETY: the caller guarantees the unit, the item and the thread.
        unsafe { f(this, item) }
    }

    /// Puts an armour piece on: `CUnit::EquipArmour` 0x530730 (thiscall, `ret 4`).
    ///
    /// # Safety
    /// As for [`add_weapon`](Self::add_weapon).
    pub unsafe fn equip_armour(this: *mut Self, item: *mut Item) {
        // SAFETY: 0x530730 is `void __thiscall (CItem*)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *mut Item)>(
                eix_sites::C_UNIT_EQUIP_ARMOUR,
            )
        };
        // SAFETY: as for `add_weapon`.
        unsafe { f(this, item) }
    }

    /// The seven protection values the unit's parameters compute from its packed vitals (`text_key` for an id of
    /// 1000000000 and above): `UnitParams::ProtectionBlock` 0x5270E0 (thiscall on [`params`](Self::params), the
    /// vitals, the output and the flag on the stack, `ret 0xC`).
    ///
    /// # Safety
    /// As for [`prototype`](Self::prototype).
    pub unsafe fn protection_block(this: *mut Self, text_key: bool) -> [f32; 7] {
        let mut out = [0.0f32; 7];
        // SAFETY: 0x5270E0 is `void __thiscall (const PackedVitals*, float* out7, bool)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut UnitParams, *mut PackedVitals, *mut f32, u32)>(
                eix_sites::UNIT_PARAMS_PROTECTION_BLOCK,
            )
        };
        // SAFETY: a field address of the live unit; nothing is read.
        let params = unsafe { &raw mut (*this).params };
        // SAFETY: as above.
        let vitals = unsafe { &raw mut (*this).map_object.packed_vitals };
        // SAFETY: the caller guarantees the unit; `out` has room for the seven floats.
        unsafe { f(params, vitals, out.as_mut_ptr(), u32::from(text_key)) };
        out
    }
}

/// `UnitAction`: what a unit is doing. `UnitAction::Assign` 0x54B320 copies kind, source, the counted target, the
/// point (+0xC, 12 bytes) and the words up to +0x20; `UnitAction::Reset` 0x54B230 sets kind 9 and releases the target.
#[repr(C)]
#[derive(Debug)]
pub struct UnitAction {
    /// The action kind (0x54B236 stores 9, "none").
    pub kind: i32,
    /// What the action uses (a skill, a spell), by kind.
    pub source: i32,
    /// The target, counted: `Assign` takes a reference (0x54B351) and releases the old one through vtable slot 4.
    pub target: *mut EngineObject,
    _unknown0c: [u8; 0x24 - 0xC],
}

const _: () = {
    assert!(offset_of!(UnitAction, source) == 0x4);
    assert!(offset_of!(UnitAction, target) == 0x8);
    assert!(size_of::<UnitAction>() == 0x24);
};

/// `CUnitServer::ScaledAttackValue` 0x54B9C0's kind of an attack interval.
pub const VALUE_ATTACK_INTERVAL: i32 = 3;

impl UnitStats {
    /// The experience factor `1.04 ^ (intelligence - 25)` as `AddScaledExperience` 0x523960 computes it: the game's
    /// constants 0x73E128 (1.04) and 0x73E124 (25.0), the game's `_CIpow` 0x6E81A0, the result stored as an `f32`.
    ///
    /// # Safety
    /// `this` must be a live statistics block of game.exe RU 1.06, on the game's thread.
    pub unsafe fn experience_factor(this: *const Self) -> f32 {
        // SAFETY: the caller guarantees a live block.
        let intelligence = unsafe { (*this).intelligence };
        let mut out = 0f32;
        // SAFETY: 0x73E128 and 0x73E124 are game.exe's read-only constants; `_CIpow` takes its base in ST1 and its
        // exponent in ST0, leaves the power in ST0 and keeps EDI (the CRT's callee-saved registers), so `out` survives.
        unsafe {
            core::arch::asm!(
                "fld qword ptr [{base}]",
                "fld dword ptr [{i}]",
                "fsub dword ptr [{k}]",
                "call {pow}",
                "fstp dword ptr [edi]",
                base = in(reg) eix_sites::RDATA_73E128,
                i = in(reg) &intelligence,
                k = in(reg) eix_sites::RDATA_73E124,
                pow = in(reg) eix_sites::TEXT_6E81A0,
                in("edi") &mut out,
                clobber_abi("C"),
            );
        }
        out
    }
}

/// An entry of a unit's animation table (`FigureAnimation`), as far as the attack and cast code reads it.
#[repr(C)]
#[derive(Debug)]
pub struct AnimationEntry {
    _unknown0: [u8; 0x14],
    /// The key: `CUnit::ActionSpeed` 0x5327D4 picks the speed by `key & 0x3C0000`.
    pub key: u32,
    _unknown18: [u8; 0x20 - 0x18],
    /// The animation's frame count: `CUnitServer::BuildCastSteps` 0x550709 divides it by `ActionSpeed`.
    pub frame_count: u32,
}

const _: () = {
    assert!(offset_of!(AnimationEntry, key) == 0x14);
    assert!(offset_of!(AnimationEntry, frame_count) == 0x20);
};

/// [`AnimationEntry::key`] bits of the action kind.
pub const ANIMATION_ACTION_MASK: u32 = 0x3C_0000;
/// An attack animation: `ActionSpeed` 0x5327DC returns the unit's attack speed (+0x4B0) for it.
pub const ANIMATION_ATTACK: u32 = 0x8_0000;
/// A cast animation: `ActionSpeed` returns the unit's cast speed (+0x4B4) for it.
pub const ANIMATION_CAST: u32 = 0xC_0000;

/// The order plan `CUnitServer::PlanAction` fills, as far as its cast kind.
#[repr(C)]
#[derive(Debug)]
pub struct OrderPlan {
    _unknown0: [u8; 0x34],
    /// Which cast animation plays: `PlanAction` 0x54D0F2 stores the spell prototype's school, 1 or 0 for two spell
    /// types, 8 without a spell.
    pub cast_kind: i32,
}

const _: () = {
    assert!(offset_of!(OrderPlan, cast_kind) == 0x34);
};

/// What `CUnitServer::ScaledItemValue` 0x54B760 scales: the word at +0x4C of the item it is given, a quick item's
/// action value or a spell's cast time (`PlanAction`'s inlined copy reads it at 0x54CB6D).
#[repr(C)]
#[derive(Debug)]
pub struct ActionItem {
    _unknown0: [u8; 0x4C],
    /// The value.
    pub value: u32,
}

const _: () = {
    assert!(offset_of!(ActionItem, value) == 0x4C);
};

impl UnitServer {
    /// `CUnitServer::ScaledAttackValue` 0x54B9C0 (thiscall, `ret 8`): for [`VALUE_ATTACK_INTERVAL`], the attack
    /// interval in ticks with `weapon` (or none), else 0. Called at its address, so a patch group's replacement of
    /// the function answers.
    ///
    /// # Safety
    /// `this` must be a live unit controller of game.exe RU 1.06 and `weapon` null or a live item, on the game's
    /// thread.
    pub unsafe fn scaled_attack_value(this: *mut Self, kind: i32, weapon: *mut Item) -> i32 {
        // SAFETY: 0x54B9C0 is `int __thiscall (int kind, CItem* weapon)`, `ret 8`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, i32, *mut Item) -> i32>(
                eix_sites::C_UNIT_SERVER_SCALED_ATTACK_VALUE,
            )
        };
        // SAFETY: the caller guarantees the controller, the item and the thread.
        unsafe { f(this, kind, weapon) }
    }
    /// The unit's level from its experience: `CUnitServer::Level` 0x556760 (thiscall, no arguments).
    ///
    /// # Safety
    /// `this` must be a live unit controller of game.exe RU 1.06, on the game's thread.
    pub unsafe fn level(this: *mut Self) -> i32 {
        // SAFETY: 0x556760 is `int __thiscall ()` with a plain `ret`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self) -> i32>(eix_sites::C_UNIT_SERVER_LEVEL)
        };
        // SAFETY: the caller guarantees the controller and the thread.
        unsafe { f(this) }
    }

    /// Adds experience: `CUnitServer::AddExperience` 0x553380 (thiscall, the float on the stack, `ret 4`).
    ///
    /// # Safety
    /// As for [`level`](Self::level).
    pub unsafe fn add_experience(this: *mut Self, value: f32) {
        // SAFETY: 0x553380 is `void __thiscall (float)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, f32)>(
                eix_sites::C_UNIT_SERVER_ADD_EXPERIENCE,
            )
        };
        // SAFETY: as for `level`.
        unsafe { f(this, value) }
    }

    /// Sets how the unit moves (1 crawl, 3 run in the script commands `Crawl` and `Run`, 0x4B73B3 and 0x4B73E8):
    /// `CUnitServer::SetMoveStyle` 0x555270 (thiscall, the style on the stack, `ret 4`).
    ///
    /// # Safety
    /// As for [`level`](Self::level).
    pub unsafe fn set_move_style(this: *mut Self, style: i32) {
        // SAFETY: 0x555270 is `void __thiscall (int)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, i32)>(
                eix_sites::C_UNIT_SERVER_SET_MOVE_STYLE,
            )
        };
        // SAFETY: as for `level`.
        unsafe { f(this, style) }
    }

    /// Sets the weapon slot the unit is to take up: `CUnitServer::SetWantedWeapon` 0x5552E0 (thiscall, the slot on the
    /// stack, `ret 4`), which stores it in the unit's pending weapon (+0x270).
    ///
    /// # Safety
    /// As for [`level`](Self::level).
    pub unsafe fn set_wanted_weapon(this: *mut Self, slot: i32) {
        // SAFETY: 0x5552E0 is `void __thiscall (int)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, i32)>(
                eix_sites::C_UNIT_SERVER_SET_WANTED_WEAPON,
            )
        };
        // SAFETY: as for `level`.
        unsafe { f(this, slot) }
    }

    /// Calls the controller's virtual slot `slot`, a thiscall without arguments.
    ///
    /// # Safety
    /// As for [`level`](Self::level); slot `slot` of the controller's vtable must be such a function.
    unsafe fn call_slot(this: *mut Self, slot: usize) {
        // SAFETY: a live object's first word is its vtable.
        let vtable = unsafe { (*this).base.vtable }.cast::<usize>();
        // SAFETY: the caller guarantees the vtable has the slot.
        let address = unsafe { *vtable.wrapping_add(slot) };
        // SAFETY: the slot is `void __thiscall ()`, as the caller guarantees.
        let f = unsafe { code::<unsafe extern "thiscall" fn(*mut Self)>(address) };
        // SAFETY: as for `level`.
        unsafe { f(this) }
    }

    /// Rebuilds the unit's model from its equipment: the controller's virtual slot 30 (ei-rebuild's
    /// `RefreshModel`), which the addon's `GiveUnitWeapon` calls.
    ///
    /// # Safety
    /// As for [`level`](Self::level).
    pub unsafe fn refresh_model(this: *mut Self) {
        // SAFETY: slot 30 (+0x78) of a unit controller's vtable is `void __thiscall ()`.
        unsafe { Self::call_slot(this, 30) }
    }

    /// Recomputes what the unit's equipment gives: the controller's virtual slot 29 (ei-rebuild's
    /// `RefreshEquipment`), which the addon's `GiveUnitWeapon` and `GiveUnitArmor` call.
    ///
    /// # Safety
    /// As for [`level`](Self::level).
    pub unsafe fn refresh_equipment(this: *mut Self) {
        // SAFETY: slot 29 (+0x74) of a unit controller's vtable is `void __thiscall ()`.
        unsafe { Self::call_slot(this, 29) }
    }
}
