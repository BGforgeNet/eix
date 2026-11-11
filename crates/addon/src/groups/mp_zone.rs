//! MpZone (`[MPZone]` with `Gipath`, `Ingos` and `Suslanger`): the network game's zone tables, which tell
//! `GameFlow::BuildZoneTable` 0x57F560 each zone's side, become the mod's lists. Each table is an array of C string
//! pointers ending in an empty string; the three operands that name game.exe's tables (0x790270, 0x790298,
//! 0x7902C0) point at the mod's.

use super::game_text::string_table;
use super::{BuildError, Labelled, Setup, pointer};

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let z = &setup.options.config.mp_zone;
    let (Some(gipath), Some(ingos), Some(suslanger)) = (&z.gipath, &z.ingos, &z.suslanger) else {
        return Err(BuildError::from(
            "[MPZone] lacks Gipath, Ingos or Suslanger".to_owned(),
        ));
    };
    let gipath = string_table(gipath)?;
    let ingos = string_table(ingos)?;
    let suslanger = string_table(suslanger)?;
    Ok(vec![
        pointer(
            "MPZone Gipath #1",
            eix_sites::GAME_FLOW_BUILD_ZONE_TABLE__159,
            eix_sites::K_ALLOD_MAPS.ru(),
            gipath,
        ),
        pointer(
            "MPZone Gipath #2",
            eix_sites::GAME_FLOW_BUILD_ZONE_TABLE__162,
            eix_sites::K_ALLOD_MAPS.ru(),
            gipath,
        ),
        pointer(
            "MPZone Ingos #1",
            eix_sites::GAME_FLOW_BUILD_ZONE_TABLE__186,
            eix_sites::DATA_790298.ru(),
            ingos,
        ),
        pointer(
            "MPZone Ingos #2",
            eix_sites::GAME_FLOW_BUILD_ZONE_TABLE__18B,
            eix_sites::DATA_790298.ru(),
            ingos,
        ),
        pointer(
            "MPZone Suslanger",
            eix_sites::GAME_FLOW_BUILD_ZONE_TABLE__1BD,
            eix_sites::DATA_7902C0.ru(),
            suslanger,
        ),
    ])
}
