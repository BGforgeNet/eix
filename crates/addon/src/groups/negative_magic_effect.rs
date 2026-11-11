//! NegativeMagicEffect: negative effect values count. `UnitStats::Recompute` keeps the larger magnitude per effect
//! type instead of the larger value (#0-#3), and `CSpellContainer::Build` stores four built values without its clamp
//! at 0 (#4-#7).

use eix_core::rules::larger_magnitude;
use eix_engine::patch;
use eix_sites::Site;

use super::{BuildError, Labelled, Setup, bytes};

/// Recompute's select, called with the two values on the stack; the game stores the result from EAX.
extern "system" fn select(a: f32, b: f32) -> u32 {
    larger_magnitude(a, b).to_bits()
}

/// The 20-byte branch-free "larger of" at `site` becomes `sub esp, 8; fstp [esp]; fstp [esp+4]; call select`,
/// NOPs: ST0 (loaded by the `fld` that replaces the compare) is the first argument, ST1 the second.
fn select_call(label: &str, site: Site, original: &[u8]) -> Result<Labelled, BuildError> {
    let mut new = vec![0x83, 0xEC, 0x08, 0xD9, 0x1C, 0x24, 0xD9, 0x5C, 0x24, 0x04];
    new.extend(patch::call_rel32(
        site.offset(new.len()),
        select as *const () as usize,
    ));
    new.resize(original.len(), 0x90);
    bytes(label, site, original, &new)
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    // `fcomp dword [ebp-x]` becomes `fld dword [ebp-x]`: the stored value joins the stack instead of being compared.
    const FCOMP: [u8; 2] = [0xD8, 0x5D];
    const FLD: [u8; 2] = [0xD9, 0x45];
    // The clamp's `fstp dword [ebp-0x14]` becomes a jump to the field's own `fstp`, 0x21 bytes on.
    const FSTP: [u8; 2] = [0xD9, 0x5D];
    const JMP: [u8; 2] = [0xEB, 0x21];
    Ok(vec![
        bytes(
            "NegativeMagicEffect #0",
            eix_sites::UNIT_STATS_RECOMPUTE__195,
            &FCOMP,
            &FLD,
        )?,
        select_call(
            "NegativeMagicEffect #1",
            eix_sites::UNIT_STATS_RECOMPUTE__198,
            &[
                0xDF, 0xE0, 0x8B, 0x4D, 0xF8, 0xC1, 0xE0, 0x17, 0xC1, 0xF8, 0x1F, 0x23, 0xC8, 0xF7,
                0xD0, 0x23, 0x45, 0xFC, 0x0B, 0xC1,
            ],
        )?,
        bytes(
            "NegativeMagicEffect #2",
            eix_sites::UNIT_STATS_RECOMPUTE__1E0,
            &FCOMP,
            &FLD,
        )?,
        select_call(
            "NegativeMagicEffect #3",
            eix_sites::UNIT_STATS_RECOMPUTE__1E3,
            &[
                0xDF, 0xE0, 0x8B, 0x4D, 0xF4, 0xC1, 0xE0, 0x17, 0xC1, 0xF8, 0x1F, 0x23, 0xC8, 0xF7,
                0xD0, 0x23, 0x45, 0xFC, 0x0B, 0xC1,
            ],
        )?,
        bytes(
            "NegativeMagicEffect #4",
            eix_sites::C_SPELL_CONTAINER_BUILD__11A,
            &FSTP,
            &JMP,
        )?,
        bytes(
            "NegativeMagicEffect #5",
            eix_sites::C_SPELL_CONTAINER_BUILD__17E,
            &FSTP,
            &JMP,
        )?,
        bytes(
            "NegativeMagicEffect #6",
            eix_sites::C_SPELL_CONTAINER_BUILD__1BF,
            &FSTP,
            &JMP,
        )?,
        bytes(
            "NegativeMagicEffect #7",
            eix_sites::C_SPELL_CONTAINER_BUILD__237,
            &FSTP,
            &JMP,
        )?,
    ])
}
