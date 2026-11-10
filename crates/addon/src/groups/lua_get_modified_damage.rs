//! LuaGetModifiedDamage: the two `ScaleDamageByEffects` (server and client) return the
//! script's damage, without the game's floor of 1.
//!
//! Both are `stdcall(unit, value)` in the game, hooked as thiscall as the original does, so the script also gets the
//! caller's ECX (the stats every caller loads).

use eix_core::lua_callbacks::Callback::{GetModifiedDamageOnClient, GetModifiedDamageOnServer};

use super::lua_common::{replace, require_host, wanted, with_host};
use super::{BuildError, Labelled, Setup};

/// `ScaleDamageByEffects` 0x523D30: `GetModifiedDamageOnServer(ecx, unit, damage)`.
extern "thiscall" fn server(ecx: u32, unit: u32, damage: f32) -> f32 {
    with_host(0.0, |h| h.get_modified_damage_on_server(ecx, unit, damage))
}

/// `ScaleDamageByEffects` 0x5285D0: `GetModifiedDamageOnClient(ecx, unit, damage)`.
extern "thiscall" fn client(ecx: u32, unit: u32, damage: f32) -> f32 {
    with_host(0.0, |h| h.get_modified_damage_on_client(ecx, unit, damage))
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    require_host()?;
    let entry = [0x55, 0x8B, 0xEC, 0x51, 0x56];
    let mut sites = Vec::new();
    if wanted(&[GetModifiedDamageOnServer]) {
        sites.push(replace(
            "LuaGetModifiedDamage #0",
            eix_sites::SCALE_DAMAGE_BY_EFFECTS_AT_523D30,
            &entry,
            server as *const () as usize,
        )?);
    }
    if wanted(&[GetModifiedDamageOnClient]) {
        sites.push(replace(
            "LuaGetModifiedDamage #1",
            eix_sites::SCALE_DAMAGE_BY_EFFECTS_AT_5285D0,
            &entry,
            client as *const () as usize,
        )?);
    }
    Ok(sites)
}
