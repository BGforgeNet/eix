//! Encumbrance, from eix.ini's `[Encumbrance]` in the mod folder: the load a hero carries slows running, and above
//! the maximum load walking too, and makes running cost more mana (core's [`eix_core::encumbrance`]).
//!
//! - Speed: movement is the packed base speed byte x the mode's byte (`CUnit::GetMoveSpeeds`, 0x52F9CF..0x52F9ED;
//!   Run reads params +0x10, Walk and Stop +0x11). Both stat writers (`UnitStats::Recompute`, `ResetToBase`) end in
//!   `UnitStats::Pack` 0x522CE0, so one hook there, after the base speed byte's store, rewrites the base byte for the
//!   walking factor and the run byte for the running factor, from the load and maximum load of the stats it packs.
//! - Gate: `UnitStats::MovementState` 0x5241C0 skips its load test (state 2), as SpellAddon's `heroalwayscanrun`
//!   does; wounded legs (state 1) still stop running.
//! - Drain: `UpdateMotion`'s running drain is rescaled right after its subtraction, whatever run system computed it;
//!   the Lua running-cost group rescales the script's drain the same way ([`drain_multiplier`]).
#![expect(
    unsafe_code,
    reason = "hooks the stat packing and reads the stats and packed bytes it writes"
)]

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use eix_core::encumbrance::{Settings, encumbrance, rescaled};
use eix_engine::game::unit::{UnitServer, UnitStats, UnitVitals};
use eix_engine::hook::{self, Context, FpuContext, Mode};
use eix_engine::x87::Ext;
use eix_sites::Site;

use crate::game::BeforeWinMain;
use crate::groups::{self, BuildError, Labelled, Tally, apply_built};
use crate::log;

const GATE_LABEL: &str = "Encumbrance gate";
/// `MovementState`'s `je` past the load test becomes `jmp`.
const GATE: Site = eix_sites::UNIT_STATS_MOVEMENT_STATE__70;
/// In `Pack`, after the base speed byte's store at 0x522E1A: `mov eax, [ebx+0x30]; mov [ebp-0x10], eax`. EBX is
/// the `UnitStats`, EDI the packed params.
const PACK_SITE: Site = eix_sites::UNIT_STATS_PACK__13D;
const PACK_BYTES: [u8; 6] = [0x8B, 0x43, 0x30, 0x89, 0x45, 0xF0];
/// The packed params' run byte (`modeSpeeds[0]`, Pack 0x522D78) and base speed byte (0x522E1A).
const RUN_BYTE: usize = 0x10;
const BASE_BYTE: usize = 0x14;
const DRAIN_LABEL: &str = "Encumbrance drain";
/// In `UpdateMotion`, just after the running drain's subtraction (`fst [edi+0x14]; fcomp dword [0x73B858]`): ST0 is
/// the new mana, [EDI+0x14] still the old, ESI the `UnitServer`, EDI its vitals. SpellAddon's `newrunsystem` patches
/// end before it, so every run system's drain gets rescaled.
const DRAIN_SITE: Site = eix_sites::UPDATE_MOTION__A93;
const DRAIN_BYTES: [u8; 9] = [0xD9, 0x57, 0x14, 0xD8, 0x1D, 0x58, 0xB8, 0x73, 0x00];
/// The first byte of `Recompute`'s `fld [ebx+0x24]`, the instruction SpellAddon's `encumbrancemovespeed` hooks.
const RECOMPUTE_SPEED_FIRST: u8 = 0xD9;

static SETTINGS: OnceLock<Settings> = OnceLock::new();
/// Whether the first pack looked at SpellAddon's speed hook yet.
static CHECKED: AtomicBool = AtomicBool::new(false);

/// SpellAddon applies when the sound system starts, before any unit is packed; its `encumbrancemovespeed` changes
/// the speed `Pack` reads, so both would apply.
fn check_spelladdon() {
    // SAFETY: game.exe's code section is mapped for the process's lifetime.
    let first = unsafe { (eix_sites::UNIT_STATS_RECOMPUTE__67A.here() as *const u8).read() };
    if first != RECOMPUTE_SPEED_FIRST {
        log::warn(
            "encumbrance",
            "SpellAddon's encumbrancemovespeed also slows movement and stacks with [Encumbrance]; set it to 0 \
             (with speedslowremaster=2 it is also what carries the Speed and Slow spells into movement)",
            "",
        );
    }
}

extern "C" fn pack(_: usize, ctx: &mut Context) {
    let Some(&settings) = SETTINGS.get() else {
        return;
    };
    if !CHECKED.swap(true, Ordering::Relaxed) {
        check_spelladdon();
    }
    // SAFETY: at this site EBX is the `UnitStats` Pack packs, live for the call.
    let stats = unsafe { &*(ctx.ebx as usize as *const UnitStats) };
    let e = encumbrance(stats.load, stats.capacity);
    if e == 0.0 {
        return;
    }
    let params = ctx.edi as usize as *mut u8;
    let (base_at, run_at) = (
        params.wrapping_add(BASE_BYTE),
        params.wrapping_add(RUN_BYTE),
    );
    // SAFETY: EDI is the params Pack writes; it stored the base speed byte just before this site.
    let base = unsafe { base_at.read() };
    // SAFETY: the run byte of the same params, stored earlier in Pack.
    let run = unsafe { run_at.read() };
    let (base, run) = settings.packed(e, stats.speed, base, run);
    // SAFETY: as above; the game's thread, inside Pack, which owns the params.
    unsafe { base_at.write(base) };
    // SAFETY: as above.
    unsafe { run_at.write(run) };
}

/// What a running unit's drain is multiplied by at `load` of `capacity`: 1 while `[Encumbrance]` is off.
pub(crate) fn drain_multiplier(load: f32, capacity: f32) -> f64 {
    SETTINGS
        .get()
        .map_or(1.0, |s| s.drain_multiplier(encumbrance(load, capacity)))
}

extern "C" fn drain(_: usize, ctx: &mut FpuContext) {
    // SAFETY: at this site ESI is the running unit's live `UnitServer`, on the game's thread.
    let server = unsafe { &*(ctx.regs.esi as usize as *const UnitServer) };
    let m = drain_multiplier(server.stats.load, server.stats.capacity);
    if m == 1.0 {
        return;
    }
    // SAFETY: EDI is that server's vitals; the store after this hook writes the new mana there.
    let old = unsafe { (*(ctx.regs.edi as usize as *const UnitVitals)).mana };
    let Some(new) = ctx.fpu.st(0) else {
        return;
    };
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the game stores the mana as an f32"
    )]
    let new = new.to_f64() as f32;
    ctx.fpu
        .set_st(0, Ext::new(f64::from(rescaled(old, new, m))));
}

fn writes(settings: Settings) -> Result<Vec<Labelled>, BuildError> {
    if !settings.enabled {
        return Ok(Vec::new());
    }
    Ok(vec![
        groups::bytes(GATE_LABEL, GATE, &[0x74], &[0xEB])?,
        Labelled {
            label: "Encumbrance speed".to_owned(),
            write: hook::context_hook(PACK_SITE, &PACK_BYTES, Mode::Before, pack, 0)?,
        },
        Labelled {
            label: DRAIN_LABEL.to_owned(),
            // The handler runs before the store, which then writes the rescaled ST0.
            write: hook::fpu_hook(
                DRAIN_SITE,
                &DRAIN_BYTES,
                &DRAIN_BYTES,
                Mode::After,
                drain,
                0,
            )?,
        },
    ])
}

/// Installs the writes `[Encumbrance]` asks for; nothing when it is off.
pub(crate) fn install(token: &BeforeWinMain, settings: Settings) {
    if !settings.enabled {
        log::info("encumbrance", "off: [Encumbrance] Enabled is 0", "");
        return;
    }
    log::info(
        "encumbrance",
        &format!(
            "RunSlope {}, WalkSlope {}, MinSpeed {}, RunDrain {}",
            settings.run_slope, settings.walk_slope, settings.min_speed, settings.run_drain
        ),
        "",
    );
    let mut tally = Tally::default();
    apply_built(
        token,
        "encumbrance",
        "Encumbrance",
        writes(settings),
        &mut tally,
    );
    // Only an applied group sets the rules: the Lua running-cost group reads them too, and without the hooks it must
    // leave the drain alone. The hooks first run after WinMain, well after this.
    if tally.applied == 1 {
        let _ = SETTINGS.set(settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> Settings {
        Settings {
            enabled: true,
            ..Settings::default()
        }
    }

    #[test]
    fn the_gate_write_is_heroalwayscanrun() {
        let ours = writes(on()).unwrap();
        let gate = &ours.iter().find(|l| l.label == GATE_LABEL).unwrap().write;
        let (_, theirs) = spelladdon::writes()
            .into_iter()
            .find(|(name, _)| name == "[gameplay] heroalwayscanrun=1")
            .unwrap();
        assert_eq!(gate.site(), eix_sites::UNIT_STATS_MOVEMENT_STATE__70);
        assert_eq!(
            (gate.site(), gate.expected(), gate.new_bytes()),
            (theirs.site(), theirs.expected(), theirs.new_bytes())
        );
    }

    #[test]
    fn spelladdon_writes_nothing_at_the_hooks() {
        for ours in [
            eix_sites::UNIT_STATS_PACK__13D
                ..eix_sites::UNIT_STATS_PACK__13D.offset(PACK_BYTES.len()),
            DRAIN_SITE..DRAIN_SITE.offset(DRAIN_BYTES.len()),
        ] {
            for (name, w) in spelladdon::writes() {
                assert!(
                    w.site().offset(w.size()) <= ours.start || ours.end <= w.site(),
                    "SpellAddon's {name} at {:#X}",
                    w.site()
                );
            }
        }
    }

    #[test]
    fn nothing_is_built_when_off() {
        assert!(writes(Settings::default()).unwrap().is_empty());
        assert_eq!(writes(on()).unwrap().len(), 3);
    }

    #[test]
    fn the_drain_hook_covers_the_store_and_compare() {
        let ours = writes(on()).unwrap();
        let drain = &ours.iter().find(|l| l.label == DRAIN_LABEL).unwrap().write;
        assert_eq!(drain.site(), eix_sites::UPDATE_MOTION__A93);
        // fst dword [edi+0x14]; fcomp dword [0x73B858]
        assert_eq!(
            drain.expected(),
            [0xD9, 0x57, 0x14, 0xD8, 0x1D, 0x58, 0xB8, 0x73, 0x00]
        );
    }

    #[test]
    fn no_multiplier_until_installed() {
        assert_eq!(drain_multiplier(500.0, 312.0), 1.0);
    }
}
