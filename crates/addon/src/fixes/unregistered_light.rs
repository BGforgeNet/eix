//! UnregisteredLight: removing or moving a light the terrain does not hold leaves the terrain alone, where the game
//! used the missing registry entry. Both look the light up in the terrain's registry (+0x25C) and use the node
//! untested: `Terrain::RemoveLight` 0x6CA720 then walks its areas from stack garbage (0x6CA7BE on) and releases the
//! light, a reference the registry never took; `Terrain::MoveLight` 0x6CA830 lets `LightAreaOf` 0x6CAA70 write the
//! area at address 4. A light removed twice, or moved before it is added (a torch's `HideEffects` then `SetLightAt`,
//! 0x52D45C), gets there. Ours returns from RemoveLight with the light untouched, and from MoveLight once it has moved
//! the light itself, the last thing the function does to it (0x6CA92F on).
#![expect(unsafe_code, reason = "moves the light the game passes")]

use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use super::Warnings;
use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// RemoveLight: `jne 0x6CA767; mov [esp+0x20], eax` after the lookup, EAX the node or 0.
const REMOVE_FOUND: Site = eix_sites::TERRAIN_REMOVE_LIGHT__3F;
const REMOVE_FOUND_BYTES: [u8; 6] = [0x75, 0x06, 0x89, 0x44, 0x24, 0x20];
const REMOVE_GO_ON: Site = eix_sites::TERRAIN_REMOVE_LIGHT__47;
/// Its epilogue, past the release.
const REMOVE_RETURN: Site = eix_sites::TERRAIN_REMOVE_LIGHT__103;
/// MoveLight: `cmp eax, ecx; jne 0x6CA885; xor edi, edi; jmp 0x6CA888` after the lookup, EAX the found position and
/// ECX the registry's end.
const MOVE_FOUND: Site = eix_sites::TERRAIN_MOVE_LIGHT__4D;
const MOVE_FOUND_BYTES: [u8; 8] = [0x3B, 0xC1, 0x75, 0x04, 0x33, 0xFF, 0xEB, 0x03];
const MOVE_GO_ON: Site = eix_sites::TERRAIN_MOVE_LIGHT__55;
/// Its epilogue (`pop edi`).
const MOVE_RETURN: Site = eix_sites::TERRAIN_MOVE_LIGHT__177;
/// MoveLight's arguments as the site's stack holds them: the light, then the offset (three floats).
const MOVE_LIGHT_ARG: usize = 0x34;
const MOVE_OFFSET_ARG: usize = 0x38;
/// The light's position.
const POSITION: usize = 0x1C;

static WARNINGS: Warnings = Warnings::new("UnregisteredLight");

extern "C" fn remove_found(_: usize, ctx: &mut Context) {
    if ctx.eax != 0 {
        ctx.resume = REMOVE_GO_ON;
        return;
    }
    WARNINGS.warn(|| "a light the terrain does not hold removed: nothing to do".to_owned());
    ctx.resume = REMOVE_RETURN;
}

extern "C" fn move_found(_: usize, ctx: &mut Context) {
    if ctx.eax != ctx.ecx {
        ctx.resume = MOVE_GO_ON;
        return;
    }
    WARNINGS.warn(|| "a light the terrain does not hold moved: the light alone".to_owned());
    let light = ctx.stack_at(ctx.stack() + MOVE_LIGHT_ARG).unwrap_or(0) as usize;
    let offset = ctx.stack_at(ctx.stack() + MOVE_OFFSET_ARG).unwrap_or(0) as usize;
    if light != 0 && offset != 0 {
        let position = core::ptr::with_exposed_provenance_mut::<[f32; 3]>(light + POSITION);
        let offset = core::ptr::with_exposed_provenance::<[f32; 3]>(offset);
        // SAFETY: the light the game passed.
        let p = unsafe { position.read_unaligned() };
        // SAFETY: the offset the game passed.
        let d = unsafe { offset.read_unaligned() };
        // The game's `fadd` and `fstp` give these sums: a sum of two floats rounds to the same float whether the x87
        // rounds it to 24, 53 or 64 bits first.
        let moved = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
        // SAFETY: as the read.
        unsafe { position.write_unaligned(moved) };
    }
    ctx.resume = MOVE_RETURN;
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![
        Labelled {
            label: "UnregisteredLight #0".to_owned(),
            write: hook::context_hook(
                REMOVE_FOUND,
                &REMOVE_FOUND_BYTES,
                Mode::Drop,
                remove_found,
                0,
            )?,
        },
        Labelled {
            label: "UnregisteredLight #1".to_owned(),
            write: hook::context_hook(MOVE_FOUND, &MOVE_FOUND_BYTES, Mode::Drop, move_found, 0)?,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_found_branches_go_where_the_jumps_went() {
        assert_eq!(
            REMOVE_GO_ON,
            REMOVE_FOUND
                .offset(2)
                .offset(usize::from(REMOVE_FOUND_BYTES[1]))
        );
        assert_eq!(MOVE_GO_ON, MOVE_FOUND.offset(MOVE_FOUND_BYTES.len()));
    }
}
