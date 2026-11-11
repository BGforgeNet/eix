//! ExpLimit: a hero's experience stops at `[AddonExp] Limit` times its intelligence factor. `GainExperience`'s
//! inlined copy of `AddScaledExperience` becomes a call of the function, whose start then calls [`add_scaled`].
//!
//! LuaGetExpModifiers, later in table order, replaces the same function by a jump over the first 5 bytes of site #1;
//! it expects this group's bytes there (`applied_write`), as the original writes over them.
#![expect(
    unsafe_code,
    reason = "the replacement works on the game's statistics block"
)]

use std::sync::atomic::{AtomicU32, Ordering};

use eix_engine::game::unit::UnitStats;
use eix_engine::patch;
use eix_engine::x87::Ext;
use eix_sites::Site;

use super::{BuildError, Labelled, Setup, bytes};

/// `Limit`, as `f32` bits.
static LIMIT: AtomicU32 = AtomicU32::new(0);

/// `UnitStats::AddScaledExperience` 0x523960: the function the sites route to.
const ADD_SCALED: Site = eix_sites::UNIT_STATS_ADD_SCALED_EXPERIENCE;
/// `UnitStats::GainExperience`'s inlined copy of it.
const GAIN_COPY: Site = eix_sites::UNIT_STATS_GAIN_EXPERIENCE__27;

/// The new `AddScaledExperience(stats, amount)`: the game's sum with the pending experience, then the cap.
extern "system" fn add_scaled(stats: *mut UnitStats, amount: f32) {
    let limit = f32::from_bits(LIMIT.load(Ordering::Relaxed));
    // SAFETY: the game calls AddScaledExperience with its statistics block in ECX, which site #1 passes here.
    let factor = unsafe { UnitStats::experience_factor(stats) };
    // SAFETY: as above; the block is the game's, on its thread, and nothing else holds a reference to it.
    let stats = unsafe { &mut *stats };
    let cap = Ext::new(limit.into()).fmul(factor.into()).to_f32();
    let total = Ext::new(factor.into())
        .fmul(amount.into())
        .fadd(stats.pending_experience.into())
        .to_f32();
    stats.pending_experience = total;
    if total > 0.0 {
        stats.pending_experience = 0.0;
        stats.experience = Ext::new(stats.experience.into())
            .fadd(total.into())
            .to_f32();
    }
    if stats.experience > cap {
        stats.experience = cap;
    }
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    LIMIT.store(setup.options.ai.exp.limit.to_bits(), Ordering::Relaxed);

    // #0: push dword [ebp+0xc]; mov ecx, esi; call AddScaledExperience; then a jump past the inlined copy's tail,
    // whose `fstp st(0)` at 0x523A19 the original runs on an empty x87 stack.
    let call_at = GAIN_COPY.offset(5);
    let mut gain = vec![0xFF, 0x75, 0x0C, 0x89, 0xF1];
    gain.extend(patch::call_rel32(call_at, ADD_SCALED.ru()));
    let after = GAIN_COPY.offset(gain.len()).offset(2);
    gain.extend([
        0xEB,
        (eix_sites::UNIT_STATS_GAIN_EXPERIENCE__6B.since(after)) as u8,
    ]);
    gain.resize(66, 0x90);

    // #1: pushad; push dword [esp+0x24] (the amount); push ecx (stats); call add_scaled; popad; ret 4.
    let mut start = vec![0x60, 0xFF, 0x74, 0x24, 0x24, 0x51];
    start.extend(patch::call_rel32(
        ADD_SCALED.offset(start.len()),
        add_scaled as *const () as usize,
    ));
    start.extend([0x61, 0xC2, 0x04, 0x00]);

    Ok(vec![
        bytes(
            "ExpLimit #0",
            GAIN_COPY,
            &[
                0xDD, 0x05, 0x28, 0xE1, 0x73, 0x00, 0xD9, 0x46, 0x20, 0xD8, 0x25, 0x24, 0xE1, 0x73,
                0x00, 0xE8, 0xB5, 0x47, 0x1C, 0x00, 0xD8, 0x4D, 0x0C, 0xD8, 0x86, 0x00, 0x07, 0x00,
                0x00, 0xD8, 0x15, 0x58, 0xB8, 0x73, 0x00, 0xD9, 0x96, 0x00, 0x07, 0x00, 0x00, 0xDF,
                0xE0, 0xF6, 0xC4, 0x41, 0x75, 0x12, 0xD8, 0x46, 0x04, 0xC7, 0x86, 0x00, 0x07, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0xD9, 0x5E, 0x04, 0xEB, 0x02,
            ],
            &gain,
        )?,
        bytes(
            "ExpLimit #1",
            ADD_SCALED,
            &[
                0xDD, 0x05, 0x28, 0xE1, 0x73, 0x00, 0x56, 0x8B, 0xF1, 0xD9, 0x46, 0x20, 0xD8, 0x25,
                0x24,
            ],
            &start,
        )?,
    ])
}
