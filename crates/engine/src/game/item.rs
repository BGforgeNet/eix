//! Items: `CItem` and the weapon extension `CItemWeapon`.
//!
//! Names follow ei-rebuild.
#![expect(unsafe_code, reason = "calls into game.exe's item code")]

use core::ffi::c_void;
use core::mem::offset_of;

use super::code;
use super::object::EngineObject;
use super::spell::Spell;

/// A weapon's database record ([`Item::prototype`] of a weapon), as far as the attack code and the addon's
/// `GiveUnitWeapon` read it (ei-rebuild's `itemdb::WeaponRecord` names the rest).
#[repr(C)]
#[derive(Debug)]
pub struct WeaponRecord {
    _unknown0: [u8; 0x6C],
    /// The attack interval before scaling: `CUnitServer::ScaledAttackValue` 0x54BA01 reads it.
    pub attack_interval: u32,
    _unknown70: [u8; 0x84 - 0x70],
    /// Added to the unit prototype's attack.
    pub attack_bonus: f32,
    /// Added to the unit prototype's defence.
    pub defence_bonus: f32,
    /// The weapon's reach.
    pub reach: f32,
    _unknown90: [u8; 0x98 - 0x90],
    /// The weights per damage type.
    pub damage_types: [f32; 7],
}

const _: () = {
    assert!(offset_of!(WeaponRecord, attack_interval) == 0x6C);
    assert!(offset_of!(WeaponRecord, attack_bonus) == 0x84);
    assert!(offset_of!(WeaponRecord, defence_bonus) == 0x88);
    assert!(offset_of!(WeaponRecord, reach) == 0x8C);
    assert!(offset_of!(WeaponRecord, damage_types) == 0x98);
};

/// `CItem`. Its kinds extend it from +0x4C: `CItemWeapon::Construct` 0x6A0540 fills 0x4C..0x5C after the base
/// constructor.
///
/// The kind is [`EngineObject::kind`]; the vtable is 0x743DA4 (`CItem::Construct` 0x69FCA0).
#[repr(C)]
#[derive(Debug)]
pub struct Item {
    /// The engine object.
    pub base: EngineObject,
    _unknown1c: [u8; 0x20 - 0x1C],
    /// Condition: `CCampPages::Price` 0x60FAB0 prices a repair by `condition / max_condition`.
    pub condition: f32,
    /// Condition when new (0x60FAB0).
    pub max_condition: f32,
    /// Magic charge: `ItemRunTickHook` 0x6A0070 refills it by `charge_rate * max_charge` per tick, up to the maximum.
    pub charge: f32,
    /// Maximum charge (0x6A0070).
    pub max_charge: f32,
    /// Charge regained per tick, a fraction of the maximum (0x6A0070).
    pub charge_rate: f32,
    /// Weight: `CUnit::TotalItemWeight` 0x5307B0 sums it over a unit's slots.
    pub weight: f32,
    /// Price: vtable slot 16, 0x4622F0, returns it; `CCampPages::Price` 0x60FAB0 multiplies it.
    pub price: f32,
    _unknown3c: [u8; 0x44 - 0x3C],
    /// The spell the item carries (scrolls, wands), counted: `CItem::SetSpell` 0x6A0520.
    pub spell: *mut Spell,
    _unknown48: [u8; 0x4C - 0x48],
}

const _: () = {
    assert!(offset_of!(Item, base) == 0x0);
    assert!(offset_of!(Item, condition) == 0x20);
    assert!(offset_of!(Item, max_condition) == 0x24);
    assert!(offset_of!(Item, charge) == 0x28);
    assert!(offset_of!(Item, max_charge) == 0x2C);
    assert!(offset_of!(Item, charge_rate) == 0x30);
    assert!(offset_of!(Item, weight) == 0x34);
    assert!(offset_of!(Item, price) == 0x38);
    assert!(offset_of!(Item, spell) == 0x44);
    assert!(size_of::<Item>() == 0x4C);
};

impl Item {
    /// The item's database record by kind (its "shape entry"): `CItem::GetPrototype` 0x6A0110 (thiscall, no
    /// arguments; a jump table on [`EngineObject::kind`]).
    ///
    /// # Safety
    /// `this` must be a live item of a supported game.exe, on the game's thread.
    pub unsafe fn prototype(this: *mut Self) -> *mut c_void {
        // SAFETY: 0x6A0110 is `void* __thiscall ()`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self) -> *mut c_void>(
                eix_sites::C_ITEM_GET_PROTOTYPE,
            )
        };
        // SAFETY: the caller guarantees the item and the thread.
        unsafe { f(this) }
    }

    /// A weapon's type: `CItemWeapon::GetType` 0x6A07F0 (thiscall, plain `ret`).
    ///
    /// # Safety
    /// As for [`prototype`](Self::prototype); the item must be a weapon.
    pub unsafe fn weapon_type(this: *mut Self) -> u32 {
        // SAFETY: 0x6A07F0 is `int __thiscall ()`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self) -> u32>(eix_sites::C_ITEM_WEAPON_GET_TYPE)
        };
        // SAFETY: as for `prototype`.
        unsafe { f(this) }
    }
}

/// `CItemWeapon`: `CItemManager::NewObjectImpl` 0x6AC810 allocates 0x5C bytes for one (0x6AC840) and constructs it
/// with `CItemWeapon::Construct` 0x6A0540.
#[repr(C)]
#[derive(Debug)]
pub struct ItemWeapon {
    /// The item.
    pub item: Item,
    _unknown4c: [u8; 0x50 - 0x4C],
    /// Minimum damage: `CItemWeapon::RollDamage` 0x6A2DE0 rolls between it and `damage + damage_spread`.
    pub damage: f32,
    /// Damage spread above the minimum (0x6A2DE0).
    pub damage_spread: f32,
    _unknown58: [u8; 0x5C - 0x58],
}

const _: () = {
    assert!(offset_of!(ItemWeapon, damage) == 0x50);
    assert!(offset_of!(ItemWeapon, damage_spread) == 0x54);
    assert!(size_of::<ItemWeapon>() == 0x5C);
};

/// `CItemArmor`: `CItemManager::NewObjectImpl` 0x6AC810 allocates 0x8C bytes for one (0x6AC86A) and constructs it
/// with `CItemArmor::Construct` 0x6A2F20, which sets up the two absorb blocks at +0x4C and +0x68.
#[repr(C)]
#[derive(Debug)]
pub struct ItemArmor {
    /// The item.
    pub item: Item,
    /// What the piece takes from each of the seven damage types, first block (ei-rebuild's `absorb[0]`).
    pub absorb: [f32; 7],
    /// The second block (ei-rebuild's `absorb[1]`).
    pub absorb2: [f32; 7],
    _unknown84: [u8; 0x8C - 0x84],
}

const _: () = {
    assert!(offset_of!(ItemArmor, absorb) == 0x4C);
    assert!(offset_of!(ItemArmor, absorb2) == 0x68);
    assert!(size_of::<ItemArmor>() == 0x8C);
};
