//! UsePrototypeActionsForMobAttack and UsePrototypeActionsForMobCast: units without a text key (monsters) attack and
//! cast at their prototype's pace. Each group replaces one of `CUnitServer::ScaledAttackValue` and `ScaledItemValue`
//! and the copy of it the game inlined into its caller.
#![expect(
    unsafe_code,
    reason = "reading the unit controller and items the game passes"
)]

use eix_core::rules::TEXT_KEY_IDS;
use eix_engine::float::ftol;
use eix_engine::game::item::{Item, WeaponRecord};
use eix_engine::game::unit::{ActionItem, UnitServer, VALUE_ATTACK_INTERVAL};
use eix_engine::group::SiteWrite;
use eix_engine::patch;
use eix_engine::x87::Ext;
use eix_sites::Site;

use super::{BuildError, Labelled, Setup, bytes};

/// `15 / rate` when the rate is not about 0, else the rate itself, as an `f32`.
fn rate_factor(rate: f32) -> f32 {
    if f64::from(rate.abs()) > 1e-5 {
        Ext::new(15.0).fdiv(rate.into()).to_f32()
    } else {
        rate
    }
}

/// `trunc(base * factor)`, the product unrounded, as `_ftol2` takes it.
fn scaled(base: f32, factor: f32) -> i32 {
    ftol(Ext::new(base.into()).fmul(factor.into()).to_f64())
}

/// Whether the controller's unit has a text key (a hero or another named unit).
///
/// # Safety
/// `us` must be a live unit controller with its unit, on the game's thread.
unsafe fn has_text_key(us: *mut UnitServer) -> bool {
    // SAFETY: the caller guarantees the controller.
    let unit = unsafe { (*us).unit };
    // SAFETY: and its unit.
    unsafe { (*unit).map_object.transform.base.id >= TEXT_KEY_IDS }
}

/// The new `ScaledAttackValue(kind, weapon)`.
extern "system" fn attack_value(us: *mut UnitServer, kind: i32, weapon: *mut Item) -> i32 {
    // SAFETY: the game calls it with a live controller, on its thread.
    let stats = unsafe { &(*us).stats };
    let factor = rate_factor(stats.rate);
    if kind != VALUE_ATTACK_INTERVAL {
        return 0;
    }
    // SAFETY: as above.
    let base = if !weapon.is_null() && unsafe { has_text_key(us) } {
        // SAFETY: a non-null weapon the game passes is a live item.
        let record = unsafe { Item::prototype(weapon) }.cast::<WeaponRecord>();
        // SAFETY: its record is a weapon's.
        unsafe { (*record).attack_interval as f32 }
    } else {
        stats.natural_attack_interval
    };
    scaled(base, factor)
}

extern "thiscall" fn attack_value_method(us: *mut UnitServer, kind: i32, weapon: *mut Item) -> i32 {
    attack_value(us, kind, weapon)
}

/// The new `ScaledItemValue(item)`.
extern "system" fn item_value(us: *mut UnitServer, item: *mut ActionItem) -> i32 {
    if item.is_null() {
        return 0;
    }
    // SAFETY: the game calls it with a live controller, on its thread.
    let stats = unsafe { &(*us).stats };
    let factor = rate_factor(stats.rate);
    // SAFETY: as above.
    let base = if unsafe { has_text_key(us) } {
        // SAFETY: a non-null item the game passes is live.
        unsafe { (*item).value as f32 }
    } else {
        stats.natural_attack_interval
    };
    scaled(base, factor)
}

extern "thiscall" fn item_value_method(us: *mut UnitServer, item: *mut ActionItem) -> i32 {
    item_value(us, item)
}

/// `code` (pushes of the arguments) and a call of `to` at `site`, then NOPs over the rest of `original`.
fn inlined_call(
    label: &str,
    site: Site,
    original: &[u8],
    code: &[u8],
    to: usize,
) -> Result<Labelled, BuildError> {
    let mut new = code.to_vec();
    new.extend(patch::call_rel32(site.offset(new.len()), to));
    new.resize(original.len(), 0x90);
    bytes(label, site, original, &new)
}

pub(super) fn build_attack(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![
        Labelled {
            label: "UsePrototypeActionsForMobAttack #0".to_owned(),
            write: SiteWrite::jmp(
                eix_sites::C_UNIT_SERVER_SCALED_ATTACK_VALUE,
                &[0x55, 0x8B, 0xEC, 0x51, 0x8B, 0x81, 0x10, 0x02, 0x00, 0x00],
                attack_value_method as *const () as usize,
            )?,
        },
        // `ExecuteOrder`'s copy, with the weapon in EAX: push eax; push 3; push esi; call.
        inlined_call(
            "UsePrototypeActionsForMobAttack #1",
            eix_sites::C_UNIT_SERVER_EXECUTE_ORDER__1DC,
            &[
                0x8B, 0x86, 0x10, 0x02, 0x00, 0x00, 0x89, 0x45, 0xFC, 0x8B, 0x17, 0xD9, 0x45, 0xFC,
                0xD8, 0x1D, 0x58, 0xB8, 0x73, 0x00, 0xDF, 0xE0, 0xF6, 0xC4, 0x40, 0x75, 0x0C, 0xD9,
                0x05, 0x18, 0xC5, 0x73, 0x00, 0xD8, 0x75, 0xFC, 0xD9, 0x5D, 0xFC, 0x33, 0xC0, 0x83,
                0xFA, 0x03, 0x75, 0x31, 0x85, 0xC9, 0x74, 0x0D, 0xE8, 0x9D, 0x25, 0x15, 0x00, 0x8B,
                0x48, 0x6C, 0x89, 0x4D, 0xF8, 0xEB, 0x0E, 0xD9, 0x86, 0x6C, 0x02, 0x00, 0x00, 0xE8,
                0xB6, 0xA9, 0x19, 0x00, 0x89, 0x45, 0xF8, 0xDB, 0x45, 0xF8, 0xD8, 0x4D, 0xFC, 0xD9,
                0x5D, 0xF8, 0xD9, 0x45, 0xF8, 0xDB, 0x5D, 0xFC, 0x8B, 0x45, 0xFC,
            ],
            &[0x50, 0x68, 0x03, 0x00, 0x00, 0x00, 0x56],
            attack_value as *const () as usize,
        )?,
    ])
}

pub(super) fn build_cast(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![
        Labelled {
            label: "UsePrototypeActionsForMobCast #0".to_owned(),
            write: SiteWrite::jmp(
                eix_sites::C_UNIT_SERVER_SCALED_ITEM_VALUE,
                &[0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x08],
                item_value_method as *const () as usize,
            )?,
        },
        // `PlanAction`'s copy, with the spell in EAX and the controller in EBX: push eax; push ebx; call.
        inlined_call(
            "UsePrototypeActionsForMobCast #1",
            eix_sites::C_UNIT_SERVER_PLAN_ACTION__114,
            &[
                0xD9, 0x83, 0x10, 0x02, 0x00, 0x00, 0xD8, 0x15, 0x58, 0xB8, 0x73, 0x00, 0xDF, 0xE0,
                0xF6, 0xC4, 0x40, 0x75, 0x06, 0xD8, 0x3D, 0x18, 0xC5, 0x73, 0x00, 0x8B, 0x41, 0x4C,
                0xC7, 0x85, 0x74, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x89, 0x85, 0x70, 0xFF,
                0xFF, 0xFF, 0xDF, 0xAD, 0x70, 0xFF, 0xFF, 0xFF, 0xD8, 0xC9, 0xD9, 0x5D, 0x08, 0xDD,
                0xD8, 0xD9, 0x45, 0x08, 0xDB, 0x5D, 0x0C, 0x8B, 0x45, 0x0C,
            ],
            &[0x50, 0x53],
            item_value as *const () as usize,
        )?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rate_scales_only_when_not_about_zero() {
        assert_eq!(rate_factor(15.0), 1.0);
        assert_eq!(rate_factor(7.5), 2.0);
        assert_eq!(rate_factor(0.0), 0.0);
        assert_eq!(rate_factor(1e-6), 1e-6);
        assert!(rate_factor(f32::NAN).is_nan());
    }

    #[test]
    fn the_product_truncates() {
        assert_eq!(scaled(10.0, 1.5), 15);
        assert_eq!(scaled(10.0, 1.59), 15);
        assert_eq!(scaled(f32::NAN, 1.0), i32::MIN);
    }
}
