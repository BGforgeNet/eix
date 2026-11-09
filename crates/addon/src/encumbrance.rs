//! Encumbrance, from eix.ini's `[Encumbrance]` in the mod folder: the load a hero carries slows running, and above
//! the maximum load walking too, and makes running cost more mana (core's [`eix_core::encumbrance`]).
//!
//! - Speed: movement is the packed base speed byte x the mode's byte (`CUnit::GetMoveSpeeds`, 0x52F9CF..0x52F9ED;
//!   Run reads params +0x10, Walk and Stop +0x11). Both stat writers (`UnitStats::Recompute`, `ResetToBase`) end in
//!   `UnitStats::Pack` 0x522CE0, so one hook there, after the base speed byte's store, rewrites the base byte for the
//!   walking factor and the run byte for the running factor, from the load and maximum load of the stats it packs.
//! - Gate: `UnitStats::MovementState` 0x5241C0 skips its load test (state 2), as SpellAddon's `heroalwayscanrun`
//!   does; wounded legs (state 1) still stop running.
#![expect(
    unsafe_code,
    reason = "hooks the stat packing and reads the stats and packed bytes it writes"
)]

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use eix_core::encumbrance::{Settings, encumbrance};
use eix_engine::game::unit::UnitStats;
use eix_engine::hook::{self, Context, Mode};

use crate::game::BeforeWinMain;
use crate::groups::{self, BuildError, Labelled, Tally, apply_built};
use crate::log;

const GATE_LABEL: &str = "Encumbrance gate";
/// `MovementState`'s `je` past the load test becomes `jmp`.
const GATE: usize = eix_sites::UNIT_STATS_MOVEMENT_STATE__70;
/// In `Pack`, after the base speed byte's store at 0x522E1A: `mov eax, [ebx+0x30]; mov [ebp-0x10], eax`. EBX is
/// the `UnitStats`, EDI the packed params.
const PACK_SITE: usize = eix_sites::UNIT_STATS_PACK__13D;
const PACK_BYTES: [u8; 6] = [0x8B, 0x43, 0x30, 0x89, 0x45, 0xF0];
/// The packed params' run byte (`modeSpeeds[0]`, Pack 0x522D78) and base speed byte (0x522E1A).
const RUN_BYTE: usize = 0x10;
const BASE_BYTE: usize = 0x14;
/// The first byte of `Recompute`'s `fld [ebx+0x24]`, the instruction SpellAddon's `encumbrancemovespeed` hooks.
const RECOMPUTE_SPEED_FIRST: u8 = 0xD9;

static SETTINGS: OnceLock<Settings> = OnceLock::new();
/// Whether the first pack looked at SpellAddon's speed hook yet.
static CHECKED: AtomicBool = AtomicBool::new(false);

/// SpellAddon applies when the sound system starts, before any unit is packed; its `encumbrancemovespeed` changes
/// the speed `Pack` reads, so both would apply.
fn check_spelladdon() {
    // SAFETY: game.exe's code section is mapped for the process's lifetime.
    let first = unsafe { (eix_sites::UNIT_STATS_RECOMPUTE__67A as *const u8).read() };
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
    ])
}

/// Installs the writes `[Encumbrance]` asks for; nothing when it is off.
pub(crate) fn install(token: &BeforeWinMain, settings: Settings) {
    if !settings.enabled {
        log::info("encumbrance", "off: [Encumbrance] Enabled is 0", "");
        return;
    }
    let _ = SETTINGS.set(settings);
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
            eix_sites::UNIT_STATS_PACK__13D..eix_sites::UNIT_STATS_PACK__13D + PACK_BYTES.len(),
            eix_sites::UPDATE_MOTION__A93..eix_sites::UPDATE_MOTION__A93 + 9,
        ] {
            for (name, w) in spelladdon::writes() {
                assert!(
                    w.site() + w.size() <= ours.start || ours.end <= w.site(),
                    "SpellAddon's {name} at {:#X}",
                    w.site()
                );
            }
        }
    }

    #[test]
    fn nothing_is_built_when_off() {
        assert!(writes(Settings::default()).unwrap().is_empty());
        assert_eq!(writes(on()).unwrap().len(), 2);
    }
}
