//! What the nine `Lua*` groups share: the Lua host, the hook kinds, the effect maxima and
//! RefreshMaxima'.
#![expect(
    unsafe_code,
    reason = "the groups' replacements read and call game.exe's unit code"
)]

use eix_core::stats::{EFFECT_TYPES, effect_maxima};
use eix_engine::game::unit::{Unit, UnitStats, UnitVitals};
use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Handler, Mode};
use eix_lua::Host;

use super::{BuildError, Labelled};
use crate::log;
use crate::lua::{addr, host};

/// The Lua host for a group's preparation: a group whose callbacks have no state fails.
pub(super) fn require_host() -> Result<(), BuildError> {
    host()
        .map(|_| ())
        .ok_or_else(|| BuildError::from("there is no Lua state".to_owned()))
}

/// Runs `f` with the Lua host. Only an applied group's hooks call this, and a group applies only with a state, so
/// `default` is for a call from another thread, which the game's single-threaded unit code never makes.
pub(super) fn with_host<R>(default: R, f: impl FnOnce(&Host) -> R) -> R {
    match host() {
        Some(h) => f(h),
        None => {
            log::error(
                "lua",
                "a Lua hook ran without the Lua state on its thread",
                "",
            );
            default
        }
    }
}

/// A function replacement: a `jmp` at the function's entry (`original` its first bytes) to `replacement`, a function
/// of the game function's signature and convention. The original is never called, so no trampoline is made.
pub(super) fn replace(
    label: &str,
    site: usize,
    original: &[u8],
    replacement: usize,
) -> Result<Labelled, BuildError> {
    Ok(Labelled {
        label: label.to_owned(),
        write: SiteWrite::jmp(site, original, replacement)?,
    })
}

/// A context hook at `site`.
pub(super) fn context(
    label: &str,
    site: usize,
    original: &[u8],
    mode: Mode,
    handler: Handler,
) -> Result<Labelled, BuildError> {
    Ok(Labelled {
        label: label.to_owned(),
        write: hook::context_hook(site, original, mode, handler, 0)?,
    })
}

/// The largest value per effect type of `unit`'s map effects; zeros for a null unit.
///
/// # Safety
/// `unit` must be null or a live unit, on the game's thread.
pub(super) unsafe fn unit_effect_maxima(unit: *mut Unit) -> [f32; EFFECT_TYPES] {
    if unit.is_null() {
        return [0.0; EFFECT_TYPES];
    }
    // SAFETY: the caller guarantees a live unit.
    let m = unsafe { &(*unit).map_object };
    let count = usize::try_from(m.effects.len).unwrap_or(0);
    let effects = if count == 0 || m.effects.data.is_null() {
        &[][..]
    } else {
        // SAFETY: a live map object's effect array holds `len` records at `data`.
        unsafe { core::slice::from_raw_parts(m.effects.data, count) }
    };
    effect_maxima(effects.iter().map(|e| (e.kind, e.value)))
}

/// Whether `unit` has a text key.
///
/// # Safety
/// `unit` must be a live unit.
pub(super) unsafe fn has_text_key(unit: *mut Unit) -> bool {
    // SAFETY: the caller guarantees a live unit.
    eix_core::stats::is_text_key(unsafe { (*unit).map_object.transform.base.id })
}

/// RefreshMaxima': the maxima from the three Lua formulas, then the slots and the hit points.
///
/// # Safety
/// `stats` and `vitals` must be `unit`'s live statistics and server vitals, on the game's thread.
pub(super) unsafe fn refresh_maxima(
    stats: *mut UnitStats,
    vitals: *mut UnitVitals,
    unit: *mut Unit,
) {
    let (s, u) = (addr(stats), addr(unit));
    let max_hp = with_host(0.0, |h| {
        let base = h.calc_base_health(s);
        h.get_modified_health(s, u, base)
    });
    // SAFETY: the caller guarantees the vitals.
    unsafe { (*vitals).max_hit_points = max_hp };
    let max_mana = with_host(0.0, |h| h.calc_base_mana(s));
    // SAFETY: as above.
    unsafe { (*vitals).max_mana = max_mana };
    // SAFETY: the maximum is read back, as the original does; a script may have changed it meanwhile.
    let max_hp = unsafe { (*vitals).max_hit_points };
    // SAFETY: the caller guarantees the statistics and the thread.
    unsafe { UnitStats::scale_slot_maxima(stats, max_hp) };
    // SAFETY: as above.
    let max_hp = unsafe { (*vitals).max_hit_points };
    // SAFETY: as above.
    let hp = unsafe { UnitStats::reduce(stats, max_hp) };
    // SAFETY: as above.
    unsafe { (*vitals).hit_points = hp };
}
