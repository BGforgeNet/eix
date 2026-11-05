//! The ai.reg groups that only change game.exe's bytes: a branch taken or dropped, an immediate, a check NOPed.

use super::{BuildError, Labelled, Setup, bytes};

/// NOPs over `original` at `site`.
fn nops(label: &str, site: usize, original: &[u8]) -> Result<Labelled, BuildError> {
    bytes(label, site, original, &vec![0x90; original.len()])
}

/// MPShowAllQuests: `PickZones`' grouping loop at 0x57F946 leaves after the first entry (`jmp` 0x57F99B), so every
/// zone of the table is a group of its own and is offered.
pub(super) fn mp_show_all_quests(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![bytes(
        "MPShowAllQuests",
        eix_sites::PICK_ZONES__142,
        &[0x02],
        &[0x08],
    )?])
}

/// DisableRunaway: `CUnitServer::UpdateState` no longer ends an action whose target left its reach (`jne` becomes
/// `jmp`).
pub(super) fn disable_runaway(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![bytes(
        "DisableRunaway",
        eix_sites::C_UNIT_SERVER_UPDATE_STATE__1F,
        &[0x75],
        &[0xEB],
    )?])
}

/// EnableArchersDefence: the server's and the client's `GetAttackParams` skip zeroing the defence of a bow or
/// crossbow wielder.
pub(super) fn enable_archers_defence(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![
        bytes(
            "EnableArchersDefence #1",
            eix_sites::C_UNIT_SERVER_GET_ATTACK_PARAMS__16C,
            &[0x74, 0x05],
            &[0xEB, 0x0C],
        )?,
        bytes(
            "EnableArchersDefence #2",
            eix_sites::C_UNIT_CLIENT_SPECIFIC_GET_ATTACK_PARAMS__1DA,
            &[0x74, 0x05],
            &[0xEB, 0x0C],
        )?,
    ])
}

/// DisableDistanceModifier: `CUnitServer::AttackRoll` no longer subtracts the distance penalty.
pub(super) fn disable_distance_modifier(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![nops(
        "DisableDistanceModifier",
        eix_sites::C_UNIT_SERVER_ATTACK_ROLL__FD,
        &[0xD8, 0x65, 0xFC],
    )?])
}

/// DisableWeightBonus: `AttackRoll` no longer adds the weapon weight ratio.
pub(super) fn disable_weight_bonus(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![nops(
        "DisableWeightBonus",
        eix_sites::C_UNIT_SERVER_ATTACK_ROLL__102,
        &[0xD8, 0x45, 0xF8],
    )?])
}

/// FastTeleport: a teleport's casting state lasts `max(duration, 1) + 1` ticks instead of `+ 30` (the immediate of
/// `add eax, 0x1E` at 0x681617).
pub(super) fn fast_teleport(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![bytes(
        "FastTeleport #0",
        eix_sites::C_SPELL_EFFECT_START__1DA9,
        &[0x1E],
        &[0x01],
    )?])
}

/// DisableCrippledHL: a severing hit no longer cripples the hit location; `UnitStats::DamageSlot` goes on to its
/// "at or below 0" test (`jle +0x45` becomes `jmp +0x5F`).
pub(super) fn disable_crippled_hl(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![bytes(
        "DisableCrippledHL #0",
        eix_sites::TEXT_525247,
        &[0x7E, 0x45],
        &[0xEB, 0x5F],
    )?])
}

/// UnlockExperimentalSpells: `CWorldServer::CastSpell` no longer returns at once for possession, link and charm.
pub(super) fn unlock_experimental_spells(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![nops(
        "UnlockExperimentalSpells #0",
        eix_sites::C_WORLD_SERVER_CAST_SPELL__47,
        &[
            0x8B, 0x47, 0x18, 0xC1, 0xE8, 0x10, 0x83, 0xF8, 0x15, 0x0F, 0x84, 0x1D, 0x04, 0x00,
            0x00, 0x83, 0xF8, 0x1B, 0x0F, 0x84, 0x14, 0x04, 0x00, 0x00, 0x83, 0xF8, 0x24, 0x0F,
            0x84, 0x0B, 0x04, 0x00, 0x00,
        ],
    )?])
}

/// RemoveSpellConstructReqs: the mana and skill checks of `CCampPages::CanMakeSpell` and `CanEquip` pass.
pub(super) fn remove_spell_construct_reqs(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    const SETGE_AL: [u8; 3] = [0x0F, 0x9D, 0xC0];
    const MOV_AL_1_NOP: [u8; 3] = [0xB0, 0x01, 0x90];
    Ok(vec![
        bytes(
            "RemoveSpellConstructReqs #0",
            eix_sites::C_CAMP_PAGES_CAN_MAKE_SPELL__253,
            &[0x32, 0xC0],
            &[0xB0, 0x01],
        )?,
        bytes(
            "RemoveSpellConstructReqs #1",
            eix_sites::C_CAMP_PAGES_CAN_MAKE_SPELL__266,
            &SETGE_AL,
            &MOV_AL_1_NOP,
        )?,
        nops(
            "RemoveSpellConstructReqs #2",
            eix_sites::CAN_DROP_ITEM__CE,
            &[0x0F, 0x8C, 0x3F, 0x07, 0x00, 0x00],
        )?,
        nops(
            "RemoveSpellConstructReqs #3",
            eix_sites::CAN_DROP_ITEM__263,
            &[0x0F, 0x8C, 0xAA, 0x05, 0x00, 0x00],
        )?,
        nops(
            "RemoveSpellConstructReqs #4",
            eix_sites::CAN_DROP_ITEM__491,
            &[0x0F, 0x8C, 0x7C, 0x03, 0x00, 0x00],
        )?,
        bytes(
            "RemoveSpellConstructReqs #5",
            eix_sites::CAN_DROP_ITEM__102,
            &SETGE_AL,
            &MOV_AL_1_NOP,
        )?,
        bytes(
            "RemoveSpellConstructReqs #6",
            eix_sites::CAN_DROP_ITEM__274,
            &SETGE_AL,
            &MOV_AL_1_NOP,
        )?,
        bytes(
            "RemoveSpellConstructReqs #7",
            eix_sites::CAN_DROP_ITEM__4A6,
            &SETGE_AL,
            &MOV_AL_1_NOP,
        )?,
    ])
}

/// AddWeaponsFromDbToAllUnits: `GiveStartingEquipment` gives every unit, not only heroes (map type 0x32), its
/// prototype's armour and weapons.
pub(super) fn add_weapons_from_db_to_all_units(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![nops(
        "AddWeaponsFromDbToAllUnits #0",
        eix_sites::C_MAP_OBJECT_MANAGER_GIVE_STARTING_EQUIPMENT__53,
        &[0x83, 0x78, 0x10, 0x32, 0x0F, 0x85, 0x15, 0x01, 0x00, 0x00],
    )?])
}

/// UnlockCorpses: `CUnitServer::ExecuteOrder` no longer refuses to search a body another unit is searching or owns.
pub(super) fn unlock_corpses(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![nops(
        "UnlockCorpses #0",
        eix_sites::C_UNIT_SERVER_EXECUTE_ORDER__86,
        &[
            0x8B, 0xCE, 0x50, 0xE8, 0x72, 0xED, 0xFF, 0xFF, 0x84, 0xC0, 0x0F, 0x85, 0x07, 0x02,
            0x00, 0x00,
        ],
    )?])
}
