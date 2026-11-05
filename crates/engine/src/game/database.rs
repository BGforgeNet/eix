//! The game databases' record tables the addon's Lua functions expose, and the skill database's index lookup.
//!
//! Each table is a game `std::vector` `{begin, end}` of fixed-size records, filled when the databases load.
#![expect(unsafe_code, reason = "calls into game.exe's skill database")]

use core::ffi::c_void;
use core::mem::offset_of;

use super::{Global, code};

/// A game `std::vector`'s first two pointers.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VectorBounds {
    /// The first record.
    pub begin: u32,
    /// One past the last record.
    pub end: u32,
}

const _: () = {
    assert!(offset_of!(VectorBounds, begin) == 0x0);
    assert!(offset_of!(VectorBounds, end) == 0x4);
};

/// The perks' records, 0x44 bytes each: `SkillDatabase::IndexOfLevel` 0x51FF00 walks them from the database's +0xC
/// to +0x10.
pub const PERKS: Global<VectorBounds> = Global::at(eix_sites::DATA_7AF274);
/// The weapons' records, 0xB4 bytes each: `CTextManager::GetItemTexts` 0x4E0150 counts and indexes them (0x4E04E7),
/// as it does the tables below.
pub const WEAPONS: Global<VectorBounds> = Global::at(eix_sites::DATA_7C86F8);
/// The armours' records, 0xC8 bytes each (0x4E031C).
pub const ARMORS: Global<VectorBounds> = Global::at(eix_sites::DATA_7C8718);
/// The materials' records, 0x50 bytes each (0x4E02C7).
pub const MATERIALS: Global<VectorBounds> = Global::at(eix_sites::DATA_7C86D8);
/// The quick items' records, 0x8C bytes each (0x4E0649).
pub const QUICKS: Global<VectorBounds> = Global::at(eix_sites::DATA_7C8738);
/// The spells' records, 0x84 bytes each (0x4E1142).
pub const SPELLS: Global<VectorBounds> = Global::at(eix_sites::DATA_7C8510);
/// The runes' records, 0x24 bytes each (0x4E1251).
pub const RUNES: Global<VectorBounds> = Global::at(eix_sites::DATA_7C8530);

/// The head of `SkillDatabase` up to its perk records.
#[repr(C)]
#[derive(Debug)]
pub struct SkillDatabase {
    _unknown0: [u8; 0xC],
    /// The perk records (0x51FF00).
    pub perks: VectorBounds,
}

const _: () = {
    assert!(offset_of!(SkillDatabase, perks) == 0xC);
};

/// The skill database, whose perk vector is [`PERKS`].
pub const SKILL_DATABASE: Global<SkillDatabase> = Global::at(eix_sites::DATA_7AF268);

impl SkillDatabase {
    /// The index of the perk record at `record`, or -1: `SkillDatabase::IndexOfLevel` 0x51FF00 (thiscall, `ret 4`).
    ///
    /// # Safety
    /// `this` must be [`SKILL_DATABASE`] of game.exe RU 1.06, on the game's thread.
    pub unsafe fn index_of_level(this: *mut Self, record: *const c_void) -> i32 {
        // SAFETY: 0x51FF00 is `int __thiscall (const void* record)`; it only compares addresses.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *const c_void) -> i32>(
                eix_sites::SKILL_DATABASE_INDEX_OF_LEVEL,
            )
        };
        // SAFETY: the caller guarantees the database and the thread.
        unsafe { f(this, record) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_perk_vector_is_the_skill_databases() {
        assert_eq!(
            SKILL_DATABASE.address() + offset_of!(SkillDatabase, perks),
            PERKS.address()
        );
    }
}
