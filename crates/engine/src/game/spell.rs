//! Spells: `CSpell`.
//!
//! Names follow ei-rebuild.

#![expect(unsafe_code, reason = "calls into game.exe's spell code")]

use core::mem::offset_of;

use super::code;
use super::object::EngineObject;

/// `CSpell`: `CSpellManager::NewObjectImpl` 0x688CD0 allocates 0x5C bytes for one (0x688D2B) and constructs it with
/// `CSpell::Construct` 0x68B1F0.
#[repr(C)]
#[derive(Debug)]
pub struct Spell {
    /// The engine object; the spell type is the high word of [`EngineObject::prototype`] (`CSpell::IsOffensive`
    /// 0x68B770).
    pub base: EngineObject,
    _unknown1c: [u8; 0x24 - 0x1C],
    /// Damage: `CItemInfoPanel::DescribeCarriedSpell` 0x5EF8F9 shows it truncated as the damage line.
    pub damage: f32,
    _unknown28: [u8; 0x40 - 0x28],
    /// Mana cost: `CUnit::CanCastSpell` 0x52F820 compares it, scaled, with the caster's mana.
    pub mana_cost: f32,
    _unknown44: [u8; 0x4C - 0x44],
    /// Cast time in ticks: `CUnitServer::PlanAction` 0x54CB6D scales it by `15 / rate` into the cast's length.
    pub cast_time: u32,
    _unknown50: [u8; 0x54 - 0x50],
    /// Modifier indices + 1, 0 ends the list: `CSpell::GetModifierCount` 0x68BA60 counts up to 8.
    pub modifiers: [u8; 8],
}

const _: () = {
    assert!(offset_of!(Spell, base) == 0x0);
    assert!(offset_of!(Spell, damage) == 0x24);
    assert!(offset_of!(Spell, mana_cost) == 0x40);
    assert!(offset_of!(Spell, cast_time) == 0x4C);
    assert!(offset_of!(Spell, modifiers) == 0x54);
    assert!(size_of::<Spell>() == 0x5C);
};

impl Spell {
    /// The spell type: the high word of the prototype (`CSpell::IsOffensive` 0x68B770 shifts it down by 16).
    pub fn spell_type(&self) -> u32 {
        self.base.prototype >> 16
    }

    /// The spell's database record: `CSpell::GetPrototype` 0x68BA20 (thiscall, no arguments) indexes the record
    /// table 0x7C8510 (0x84 bytes each) by the prototype word.
    ///
    /// # Safety
    /// `this` must be a live spell of game.exe RU 1.06, on the game's thread.
    pub unsafe fn record(this: *mut Self) -> *mut SpellRecord {
        // SAFETY: 0x68BA20 is `SpellRecord* __thiscall ()` with a plain `ret`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self) -> *mut SpellRecord>(
                eix_sites::C_SPELL_GET_PROTOTYPE,
            )
        };
        // SAFETY: the caller guarantees the spell and the thread.
        unsafe { f(this) }
    }
}

/// A spell's database record (0x84 bytes, [`Spell::record`]), as far as the patch groups read it.
#[repr(C)]
#[derive(Debug)]
pub struct SpellRecord {
    _unknown0: [u8; 0x14],
    /// The school: `CUnitServer::PlanAction` 0x54D0EF takes it as the cast animation's kind.
    pub school: u32,
    _unknown18: [u8; 0x1C - 0x18],
    /// The spell's slots (the database's "Slots", ei-rebuild's `complexity`); no game reader checked: only
    /// LoadSpellAnimationFromSlotsField reads it, as a cast animation's kind.
    pub slots: u32,
    _unknown20: [u8; 0x2C - 0x20],
    /// Damage: `CItemInfoPanel::DescribeSpellRecord` 0x5F0E38 shows it truncated as the damage line.
    pub damage: f32,
    _unknown30: [u8; 0x3C - 0x30],
    /// Cast time in ticks: `CSpellContainer::Build` 0x68BFAD copies it into the built spell.
    pub cast_time: u32,
    _unknown40: [u8; 0x60 - 0x40],
    /// The icon's number: `CHudUnitPanel::ShowEffects` 0x646B90 shows an effect's icon as the picture `spell%04d` of
    /// it (0x646CEF, 0x646F2F).
    pub icon: u32,
    _unknown64: [u8; 0x7B - 0x64],
    /// Non-zero when the damage is a number: the info windows show the damage line for it (0x5EF8DD, 0x63CFCC),
    /// else a text.
    pub shows_damage: u8,
    _unknown7c: [u8; 0x84 - 0x7C],
}

const _: () = {
    assert!(offset_of!(SpellRecord, school) == 0x14);
    assert!(offset_of!(SpellRecord, slots) == 0x1C);
    assert!(offset_of!(SpellRecord, damage) == 0x2C);
    assert!(offset_of!(SpellRecord, cast_time) == 0x3C);
    assert!(offset_of!(SpellRecord, icon) == 0x60);
    assert!(offset_of!(SpellRecord, shows_damage) == 0x7B);
    assert!(size_of::<SpellRecord>() == 0x84);
};

/// A rune or modifier record the spell constructor builds from (`SpellModifierRecord`), as `CSpellContainer::Build`
/// reads it.
#[repr(C)]
#[derive(Debug)]
pub struct ModifierRecord {
    _unknown0: [u8; 4],
    /// The code letters: `Build` 0x68C042 switches on the first.
    pub code: [u8; 2],
    _unknown6: [u8; 0x14 - 6],
    /// Strength: `Build` 0x68C06E multiplies the prototype's values by it.
    pub strength: f32,
    /// Skill needed: `Build` 0x68C22A adds it to the container's total.
    pub skill: u32,
}

const _: () = {
    assert!(offset_of!(ModifierRecord, code) == 0x4);
    assert!(offset_of!(ModifierRecord, strength) == 0x14);
    assert!(offset_of!(ModifierRecord, skill) == 0x18);
};

/// The spell constructor's builder (`CSpellContainer`), as far as its spell's cast time.
#[repr(C)]
#[derive(Debug)]
pub struct SpellContainer {
    _unknown0: [u8; 0x7C],
    /// The built spell's cast time: `Build` 0x68BFAD copies the record's +0x3C there.
    pub cast_time: i32,
}

const _: () = {
    assert!(offset_of!(SpellContainer, cast_time) == 0x7C);
};
