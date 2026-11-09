//! UnitShadowResolution: a unit's shadow is drawn into a 64 by 64 texture. `CObject3DClientSpecific::Render` rasterises
//! the caster's triangles into one shared scratch surface, blurs it and copies it into the unit's own texture; the
//! side is spelled out where the two textures are created (`CreateFormat25`, `GetOrCreateFormat25`), where the
//! silhouette is scaled into the surface, where the scratch is cleared and in the two blur calls. With the setting at
//! 128 or 256 every one of those takes the new side, or the fix applies none of them: the two textures must match.
//! Rendering only; nothing a multiplayer peer computes reads a shadow.
//!
//! The silhouette scale divides 63.0 (a constant the game shares) by the caster's extent: both divisions read our
//! own float, the side less one, instead. The caster map's source stride in the draw descriptor does not depend on the
//! side and stays.

use std::sync::atomic::{AtomicU32, Ordering};

use eix_core::engine_fixes::unit_shadow_size;
use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, Mode};

use crate::groups::{BuildError, Labelled};
use crate::log;
use crate::setup::Setup;

/// `mov eax, 0x40`: the side, in the four texture creators.
const SIDE: [u8; 5] = [0xB8, 0x40, 0x00, 0x00, 0x00];
const SIDE_SITES: [usize; 4] = [
    eix_sites::TEXTURE_MANAGER_CREATE_FORMAT25__21,
    eix_sites::TEXTURE_MANAGER_CREATE_FORMAT25__71,
    eix_sites::TEXTURE_MANAGER_GET_OR_CREATE_FORMAT25__2F,
    eix_sites::TEXTURE_MANAGER_GET_OR_CREATE_FORMAT25__7F,
];
/// `fdivr dword [0x73D044]`: 63.0 over the extent, in the two places that scale the silhouette.
const SCALE: [u8; 6] = [0xD8, 0x3D, 0x44, 0xD0, 0x73, 0x00];
const SCALE_SITES: [usize; 2] = [
    eix_sites::C_OBJECT_3D_CLIENT_SPECIFIC_RENDER__A18,
    eix_sites::C_OBJECT_3D_CLIENT_SPECIFIC_RENDER__A2D,
];
/// `mov ecx, 0x800`: the scratch's clear, in dwords (the side squared, 16 bits a pixel).
const CLEAR: usize = eix_sites::C_OBJECT_3D_CLIENT_SPECIFIC_RENDER__C3C;
const CLEAR_BYTES: [u8; 5] = [0xB9, 0x00, 0x08, 0x00, 0x00];
/// `push 0x40; push 0x40; mov edx, [esi]`: the blur's width and height, then the rest of the call's setup.
const BLUR_A: usize = eix_sites::C_OBJECT_3D_CLIENT_SPECIFIC_RENDER__D9B;
const BLUR_A_BYTES: [u8; 6] = [0x6A, 0x40, 0x6A, 0x40, 0x8B, 0x16];
/// `push 0x40; push 0x40; push eax`.
const BLUR_B: usize = eix_sites::C_OBJECT_3D_CLIENT_SPECIFIC_RENDER__DAF;
const BLUR_B_BYTES: [u8; 5] = [0x6A, 0x40, 0x6A, 0x40, 0x50];

/// The side less one as `f32` bits, which the scale operands address.
static EXTENT: AtomicU32 = AtomicU32::new(0);

/// `push imm8` holds no 128: the calls run as the game has them, then the pushed words become the side.
extern "C" fn blur_a(side: usize, ctx: &mut Context) {
    // The words are the two pushes; the load after them pushes nothing.
    set_side(side, ctx, [0, 1]);
}

extern "C" fn blur_b(side: usize, ctx: &mut Context) {
    // The word at the stack's top is `push eax`, which follows the two sizes.
    set_side(side, ctx, [1, 2]);
}

fn set_side(side: usize, ctx: &mut Context, words: [usize; 2]) {
    let side = u32::try_from(side).unwrap_or(0);
    for word in words {
        if !ctx.set_stack_word(word, side) {
            log::error(
                "fixes",
                "the blur's arguments are off the stack",
                "UnitShadowResolution",
            );
        }
    }
}

/// The largest pixel index of a texture `side` wide: what the silhouette scale maps the caster's extent to.
fn extent(side: u32) -> f32 {
    side as f32 - 1.0
}

/// Where the scale operands point: our float, which holds the side less one. The game is a 32-bit image, so the
/// address fits.
fn extent_address() -> u32 {
    EXTENT.as_ptr().expose_provenance() as u32
}

fn with_word(bytes: &[u8], at: usize, value: u32) -> Vec<u8> {
    let mut new = bytes.to_vec();
    new[at..at + 4].copy_from_slice(&value.to_le_bytes());
    new
}

/// The setting's reason to leave the fix out, with a warning for a value the renderer cannot take.
pub(super) fn off(setup: &Setup) -> Option<String> {
    let value = setup.options.settings.unit_shadow_resolution;
    match unit_shadow_size(value) {
        Some(64) => Some("UnitShadowResolution is 64".to_owned()),
        Some(_) => None,
        None => {
            log::warn(
                "fixes",
                &format!("UnitShadowResolution {value} is not 64, 128 or 256: the game's 64 stays"),
                "UnitShadowResolution",
            );
            Some(format!(
                "UnitShadowResolution {value} is not 64, 128 or 256"
            ))
        }
    }
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let value = setup.options.settings.unit_shadow_resolution;
    let side = match unit_shadow_size(value) {
        Some(side) if side > 64 => side,
        _ => return Err(format!("UnitShadowResolution {value} is not 128 or 256").into()),
    };
    EXTENT.store(extent(side).to_bits(), Ordering::Relaxed);
    let mut writes = Vec::new();
    let mut add = |label: String, write: SiteWrite| writes.push(Labelled { label, write });
    for (i, site) in SIDE_SITES.into_iter().enumerate() {
        let write = SiteWrite::bytes(site, &SIDE, &with_word(&SIDE, 1, side))?;
        add(format!("UnitShadowResolution side #{i}"), write);
    }
    for (i, site) in SCALE_SITES.into_iter().enumerate() {
        let write = SiteWrite::bytes(site, &SCALE, &with_word(&SCALE, 2, extent_address()))?;
        add(format!("UnitShadowResolution scale #{i}"), write);
    }
    let clear = SiteWrite::bytes(
        CLEAR,
        &CLEAR_BYTES,
        &with_word(&CLEAR_BYTES, 1, side * side / 2),
    )?;
    add("UnitShadowResolution clear".to_owned(), clear);
    let arg = side as usize;
    let a = hook::context_hook(BLUR_A, &BLUR_A_BYTES, Mode::Before, blur_a, arg)?;
    add("UnitShadowResolution blur #0".to_owned(), a);
    let b = hook::context_hook(BLUR_B, &BLUR_B_BYTES, Mode::Before, blur_b, arg)?;
    add("UnitShadowResolution blur #1".to_owned(), b);
    Ok(writes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(value: u32) -> Setup {
        let mut setup = Setup::default();
        setup.options.settings.unit_shadow_resolution = value;
        setup
    }

    #[test]
    fn the_group_covers_every_site_with_the_games_bytes() {
        let writes = build(&setup(128)).unwrap();
        let sites: Vec<(usize, usize)> = writes
            .iter()
            .map(|l| (l.write.site(), l.write.expected().len()))
            .collect();
        let want: Vec<(usize, usize)> = SIDE_SITES
            .iter()
            .map(|&s| (s, SIDE.len()))
            .chain(SCALE_SITES.iter().map(|&s| (s, SCALE.len())))
            .chain([
                (CLEAR, CLEAR_BYTES.len()),
                (BLUR_A, BLUR_A_BYTES.len()),
                (BLUR_B, BLUR_B_BYTES.len()),
            ])
            .collect();
        assert_eq!(sites, want);
        let expected: Vec<&[u8]> = writes.iter().map(|l| l.write.expected()).collect();
        assert_eq!(expected[0], SIDE);
        assert_eq!(expected[4], SCALE);
        assert_eq!(expected[6], CLEAR_BYTES);
        assert_eq!(expected[7], BLUR_A_BYTES);
        assert_eq!(expected[8], BLUR_B_BYTES);
    }

    #[test]
    fn the_sizes_follow_the_side() {
        for (side, clear) in [(128u32, 0x2000u32), (256, 0x8000)] {
            let writes = build(&setup(side)).unwrap();
            for l in &writes[..4] {
                assert_eq!(
                    l.write.new_bytes(),
                    [0xB8, side as u8, (side >> 8) as u8, 0, 0]
                );
            }
            assert_eq!(
                writes[6].write.new_bytes(),
                [&[0xB9][..], &clear.to_le_bytes()].concat()
            );
            assert_eq!(extent(side), if side == 128 { 127.0 } else { 255.0 });
            // The scale operands name our float, not the game's shared 63.0.
            for l in &writes[4..6] {
                assert_eq!(l.write.new_bytes()[..2], SCALE[..2]);
                assert_eq!(l.write.new_bytes()[2..], extent_address().to_le_bytes());
            }
        }
    }

    #[test]
    fn only_128_and_256_build_and_only_they_stay_on() {
        for value in [0, 64, 100, 512] {
            assert!(build(&setup(value)).is_err(), "{value}");
        }
        assert_eq!(
            off(&setup(64)).as_deref(),
            Some("UnitShadowResolution is 64")
        );
        assert!(off(&setup(128)).is_none());
        assert!(off(&setup(256)).is_none());
        assert!(off(&setup(100)).is_some());
    }
}
