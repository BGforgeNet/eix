//! SpellsNeedAcceptedSkills: the camp's spell constructor counts only the skills a hero has accepted. The skills page
//! raises a hero's schools, science and perks the moment they are bought, and keeps a record of the hero as last
//! accepted until the action button accepts the purchase; the second button, or leaving the camp, puts the record back
//! (`CTradePanel::Settle` 0x6255A0). The constructor judges a spell by the party's best skill and mana in its school,
//! `CCampScreen::BestPartySkill` 0x610020, read from the heroes' statistics, so a spell made in between needed skill
//! the party never paid for. The method is slot 45 of the camp's vtable and reached only through it; ours puts the
//! record of each hero with an unaccepted purchase in place for the original's call, and the bought values back after.
#![expect(
    unsafe_code,
    reason = "reads the camp's skill records, swaps them with the party's statistics and calls the original method"
)]

use core::ffi::c_void;
use core::mem::{offset_of, swap};

use eix_engine::game::player::Player;
use eix_engine::game::unit::UnitStats;
use eix_engine::group::SiteWrite;
use eix_engine::hook;
use eix_sites::Site;

use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// The camp vtable's `BestPartySkill` slot and the method it holds.
const SLOT: Site = eix_sites::RDATA_7409D0;
const BEST_PARTY_SKILL: Site = eix_sites::C_CAMP_SCREEN_BEST_PARTY_SKILL;
/// The camp's skills page (`CTradePanel`), as `CCampPages::DoAction` 0x6112CC reads it.
const TRADE: usize = 0x1F4;
/// The skills page's records, one per party member in party order: a vector's first and end pointers
/// (`CTradePanel::Settle` 0x6255A0).
const RECORDS: usize = 0x190;

/// `this` (the camp), the spell school, and where the best skill and the most mana units go (`ret 0xC`).
type BestPartySkill = unsafe extern "thiscall" fn(*mut c_void, i32, *mut i32, *mut i32);

/// A party member as the skills page last accepted it (`TradeSnapshotRecord` 0x626F10 fills it).
#[repr(C)]
struct SkillRecord {
    science: u8,
    schools: [u8; 6],
    perks: [u8; 36],
    experience: f32,
    spent_experience: f32,
    pending_experience: f32,
    /// Nonzero once something was bought since the record was taken.
    bought: u8,
}

const _: () = {
    assert!(offset_of!(SkillRecord, science) == 0x0);
    assert!(offset_of!(SkillRecord, schools) == 0x1);
    assert!(offset_of!(SkillRecord, perks) == 0x7);
    assert!(offset_of!(SkillRecord, experience) == 0x2C);
    assert!(offset_of!(SkillRecord, spent_experience) == 0x30);
    assert!(offset_of!(SkillRecord, pending_experience) == 0x34);
    assert!(offset_of!(SkillRecord, bought) == 0x38);
    assert!(size_of::<SkillRecord>() == 0x3C);
};

/// The records' vector: its first and end pointers.
#[repr(C)]
struct Records {
    first: *mut SkillRecord,
    end: *mut SkillRecord,
}

const _: () = {
    assert!(offset_of!(Records, first) == 0x0);
    assert!(offset_of!(Records, end) == 0x4);
};

/// Exchanges the values a record keeps with the hero's: the fields `TradeRestoreRecord` 0x626FA0 copies back.
fn exchange(record: &mut SkillRecord, stats: &mut UnitStats) {
    swap(&mut record.science, &mut stats.attributes[0]);
    for (kept, live) in record.schools.iter_mut().zip(&mut stats.skills) {
        swap(kept, live);
    }
    for (kept, live) in record.perks.iter_mut().zip(&mut stats.skill_levels) {
        swap(kept, live);
    }
    swap(&mut record.experience, &mut stats.experience);
    swap(&mut record.spent_experience, &mut stats.spent_experience);
    swap(
        &mut record.pending_experience,
        &mut stats.pending_experience,
    );
}

/// Exchanges every record with an unaccepted purchase with its hero's statistics; run twice, it leaves both as they
/// were.
///
/// # Safety
/// `camp` must be the live camp, on the game's thread.
unsafe fn exchange_bought(camp: *mut c_void) {
    // SAFETY: the caller guarantees the camp, which holds its skills page.
    let trade = unsafe { camp.wrapping_byte_add(TRADE).cast::<*mut u8>().read() };
    if trade.is_null() {
        return;
    }
    // SAFETY: the skills page holds its records' vector.
    let vector = unsafe { trade.wrapping_add(RECORDS).cast::<Records>().read() };
    if vector.first.is_null() {
        return;
    }
    // SAFETY: both pointers bound one vector of records.
    let count = usize::try_from(unsafe { vector.end.offset_from(vector.first) }).unwrap_or(0);
    // SAFETY: the vector holds `count` records; nothing else holds them during the call.
    let records = unsafe { core::slice::from_raw_parts_mut(vector.first, count) };
    // SAFETY: a camp shows the local player's party.
    let player = unsafe { Player::local() };
    // SAFETY: the local player is live while its camp is.
    let player = unsafe { &*player };
    let (party, heroes) = (player.party, player.party_count);
    if party.is_null() {
        return;
    }
    // SAFETY: the player holds `heroes` records from `party`; nothing else holds them during the call.
    let heroes =
        unsafe { core::slice::from_raw_parts_mut(party, usize::try_from(heroes).unwrap_or(0)) };
    // The records follow the party's order (`TradeSnapshotRecord` copies record i from hero i).
    for (record, hero) in records.iter_mut().zip(heroes) {
        if record.bought != 0 {
            exchange(record, &mut hero.stats);
        }
    }
}

/// `CCampScreen::BestPartySkill` on the heroes as last accepted.
extern "thiscall" fn best_party_skill(
    camp: *mut c_void,
    school: i32,
    skill: *mut i32,
    mana: *mut i32,
) {
    // SAFETY: the address is the original method, of this type.
    let original = unsafe { hook::as_fn::<BestPartySkill>(BEST_PARTY_SKILL) };
    // SAFETY: the game calls the slot on its live camp, on its thread.
    unsafe { exchange_bought(camp) };
    // SAFETY: the game's arguments, passed on.
    unsafe { original(camp, school, skill, mana) };
    // SAFETY: as above; the original changes neither the records nor the statistics.
    unsafe { exchange_bought(camp) };
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "SpellsNeedAcceptedSkills".to_owned(),
        write: SiteWrite::pointer(
            SLOT,
            BEST_PARTY_SKILL.ru(),
            best_party_skill as *const () as usize,
        ),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exchange_twice_leaves_record_and_hero_as_they_were() {
        // SAFETY: both are plain data, valid as all zeros.
        let (mut record, mut stats): (SkillRecord, UnitStats) = unsafe { core::mem::zeroed() };
        record.science = 1;
        record.schools = [2; 6];
        record.perks = [3; 36];
        record.experience = 100.0;
        record.spent_experience = 40.0;
        record.pending_experience = 5.0;
        stats.attributes = [7, 8, 9];
        stats.skills = [6; 8];
        stats.skill_levels = [4; 0x25];
        stats.experience = 100.0;
        stats.spent_experience = 90.0;

        exchange(&mut record, &mut stats);
        assert_eq!(stats.attributes, [1, 8, 9]);
        assert_eq!(stats.skills, [2, 2, 2, 2, 2, 2, 6, 6]);
        assert_eq!(stats.skill_levels[..36], [3; 36]);
        assert_eq!(stats.skill_levels[36], 4);
        assert_eq!(
            (
                stats.experience,
                stats.spent_experience,
                stats.pending_experience
            ),
            (100.0, 40.0, 5.0)
        );

        exchange(&mut record, &mut stats);
        assert_eq!(
            (record.science, record.schools, record.perks),
            (1, [2; 6], [3; 36])
        );
        assert_eq!(stats.attributes, [7, 8, 9]);
        assert_eq!(stats.skills, [6; 8]);
        assert_eq!(stats.skill_levels, [4; 0x25]);
        assert_eq!(
            (
                stats.experience,
                stats.spent_experience,
                stats.pending_experience
            ),
            (100.0, 90.0, 0.0)
        );
    }
}
