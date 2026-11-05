//! LuaRegenerateHpAndMp: `UnitVitals::Regenerate` is the script's `RegenerateHpAndMp`.

use eix_engine::game::unit::{Unit, UnitStats, UnitVitals};

use super::lua_common::{replace, require_host, with_host};
use super::{BuildError, Labelled, Setup};
use crate::lua::addr;

/// `UnitVitals::Regenerate` 0x523740 (thiscall; stats, unit and a `char` flag on the stack): whether anything
/// changed, as the script's result is truthy.
extern "thiscall" fn regenerate(
    vitals: *mut UnitVitals,
    stats: *mut UnitStats,
    unit: *mut Unit,
    in_combat: u32,
) -> bool {
    // The flag is a `char`: only its low byte counts, as the original's `cmp BYTE PTR [ebp+0x14]`.
    let in_combat = in_combat & 0xFF != 0;
    with_host(false, |h| {
        h.regenerate_hp_and_mp(addr(vitals), addr(stats), addr(unit), in_combat)
    })
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    require_host()?;
    Ok(vec![replace(
        "LuaRegenerateHpAndMp #0",
        eix_sites::UNIT_VITALS_REGENERATE,
        &[0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x0C],
        regenerate as *const () as usize,
    )?])
}
