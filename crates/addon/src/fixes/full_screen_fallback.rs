//! FullScreenFallback: a full-screen mode the display refuses gives way to the smallest larger mode it offers at the
//! same depth, where the game went on without a render surface and crashed (`CTextureDevice::Construct` reads it at
//! 0x4E6183). `ConstructDirectDraw` 0x4EDD70 sets the mode at 0x4EDEF8; ours sets it the same way and looks for a
//! fallback only when that fails, so a mode the display (or a DirectDraw wrapper) accepts is left alone.
#![expect(
    unsafe_code,
    reason = "the stand-in for the game's SetDisplayMode call works on its IDirectDraw7"
)]

use core::ffi::c_void;

use eix_core::engine_fixes::{DisplayMode, fallback_mode};
use eix_engine::ddraw::{self, Mode};
use eix_engine::group::SiteWrite;
use eix_engine::{exec, patch};
use eix_sites::Site;

use crate::groups::{BuildError, Labelled};
use crate::log;
use crate::setup::Setup;

/// `push edx; push eax; call [ecx+0x54]`: the width and the IDirectDraw7 go on the stack last, then
/// `IDirectDraw7::SetDisplayMode` (height, depth, refresh and flags are pushed already).
const SITE: Site = eix_sites::C_TEXTURE_DEVICE_CONSTRUCT_DIRECT_DRAW__186;
const SITE_BYTES: [u8; 5] = [0x52, 0x50, 0xFF, 0x51, 0x54];

/// `pop ecx; push edx; push eax; push ecx`, then a jump to [`set_display_mode`]: the call into the stub left the
/// return address on top, and this puts the two missing arguments under it, as the replaced call had them.
const STUB_HEAD: [u8; 4] = [0x59, 0x52, 0x50, 0x51];

fn to_core(m: Mode) -> DisplayMode {
    DisplayMode {
        width: m.width,
        height: m.height,
        bits: m.bits,
    }
}

/// The game's `SetDisplayMode` call, with the fallback on failure; its HRESULT (the fallback's when one is tried).
extern "system" fn set_display_mode(
    ddraw: *mut c_void,
    width: u32,
    height: u32,
    bits: u32,
    refresh: u32,
    flags: u32,
) -> i32 {
    let wanted = Mode {
        width,
        height,
        bits,
    };
    // SAFETY: game.exe calls here with its live IDirectDraw7 in full-screen cooperative level, as it called
    // SetDisplayMode.
    let hr = unsafe { ddraw::set_display_mode(ddraw, wanted, refresh, flags) };
    if hr >= 0 {
        return hr;
    }
    let what = format!("full screen {width}x{height} at {bits} bits refused (0x{hr:08X})");
    // SAFETY: as above.
    let offered = match unsafe { ddraw::display_modes(ddraw) } {
        Ok(modes) => modes,
        Err(e) => {
            log::warn(
                "fixes",
                &format!("{what}; the modes could not be listed (0x{e:08X})"),
                "FullScreenFallback",
            );
            return hr;
        }
    };
    let offered: Vec<DisplayMode> = offered.into_iter().map(to_core).collect();
    let Some(fallback) = fallback_mode(to_core(wanted), &offered) else {
        log::warn(
            "fixes",
            &format!("{what}; no larger mode at {bits} bits"),
            "FullScreenFallback",
        );
        return hr;
    };
    let mode = Mode {
        width: fallback.width,
        height: fallback.height,
        bits: fallback.bits,
    };
    // Refresh 0: the display's own rate for the new mode, which may not have the one asked for.
    // SAFETY: as above.
    let fallback_hr = unsafe { ddraw::set_display_mode(ddraw, mode, 0, flags) };
    let outcome = if fallback_hr >= 0 {
        "set".to_owned()
    } else {
        format!("refused too (0x{fallback_hr:08X})")
    };
    log::warn(
        "fixes",
        &format!("{what}; {}x{} {outcome}", mode.width, mode.height),
        "FullScreenFallback",
    );
    fallback_hr
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let target = set_display_mode as *const () as usize;
    let stub = exec::place(STUB_HEAD.len() + 5, |at| {
        let mut code = STUB_HEAD.to_vec();
        code.extend(patch::jmp_rel32(Site::of(at + STUB_HEAD.len()), target));
        Ok::<_, String>(code)
    })?;
    Ok(vec![Labelled {
        label: "FullScreenFallback #0".to_owned(),
        write: SiteWrite::call(SITE, &SITE_BYTES, stub)?,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_site_calls_the_stub_that_jumps_to_ours() {
        let w = build(&Setup::default()).unwrap();
        let write = &w[0].write;
        assert_eq!((write.site(), write.expected()), (SITE, &SITE_BYTES[..]));
        let new = write.new_bytes();
        assert_eq!(new[0], 0xE8);
        let stub = SITE
            .offset(5)
            .ru()
            .wrapping_add_signed(i32::from_le_bytes(new[1..5].try_into().unwrap()) as isize);
        // SAFETY: the stub was placed in the pool, readable, nine bytes long.
        let code = unsafe { core::slice::from_raw_parts(stub as *const u8, 9) };
        assert_eq!(&code[..4], &STUB_HEAD);
        let jump = (stub + 9)
            .wrapping_add_signed(i32::from_le_bytes(code[5..9].try_into().unwrap()) as isize);
        assert_eq!(
            (code[4], jump),
            (0xE9, set_display_mode as *const () as usize)
        );
    }
}
