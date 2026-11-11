//! PointLightOverflow: a script's `CreatePointLight` with every light slot taken creates no light, where the game wrote
//! one slot past the array. `CScript::CreatePointLight` 0x4BD6A0 takes the slot of the light's id, else the first
//! free one, out of the script's 256 (`PointLights::SetSize(0x100)` in `CScript::Construct`, 0x4BDFF4; 0x20 bytes
//! each); with all of them used by other ids its index ends at the count, and the new light is written there
//! (0x4BD704 on). Ours returns before the write. The search stays as it is: a free slot before the id's own still
//! wins, so a script can hold two lights of one id, as in the game.
#![expect(unsafe_code, reason = "reads the script's light count")]

use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use super::Warnings;
use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// `mov ecx, [esi+4]; xor eax, eax; shl edi, 5`: ESI the script, EDI the slot's index, made a byte offset.
const SLOT: Site = eix_sites::C_SCRIPT_CREATE_POINT_LIGHT__64;
const SLOT_BYTES: [u8; 8] = [0x8B, 0x4E, 0x04, 0x33, 0xC0, 0xC1, 0xE7, 0x05];
/// The function's epilogue (`pop edi`), after the write.
const RETURN: Site = eix_sites::C_SCRIPT_CREATE_POINT_LIGHT__174;
/// The script's light count, after the array's data pointer (+0x04).
const COUNT: usize = 0x08;
/// A slot's size, the `shl edi, 5`.
const SLOT_SHIFT: u32 = 5;

static WARNINGS: Warnings = Warnings::new("PointLightOverflow");

extern "C" fn slot(_: usize, ctx: &mut Context) {
    let script = core::ptr::with_exposed_provenance::<i32>(ctx.esi as usize + COUNT);
    // SAFETY: ESI is the script the game is running, whose count the search just read.
    let count = unsafe { script.read() };
    let index = ctx.edi >> SLOT_SHIFT;
    if i64::from(index) >= i64::from(count) {
        WARNINGS.warn(|| format!("point light in slot {index} of {count}: not created"));
        ctx.resume = RETURN;
    }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "PointLightOverflow #0".to_owned(),
        write: hook::context_hook(SLOT, &SLOT_BYTES, Mode::Before, slot, 0)?,
    }])
}
