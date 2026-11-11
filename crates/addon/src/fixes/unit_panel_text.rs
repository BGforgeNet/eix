//! UnitPanelText: the battle screen's unit panel draws its text every frame through the device context of its
//! DirectDraw surface. `CHudUnitPanel::DrawVanilla` 0x6462A0 gets the surface's DC (`UiSurfaceGetDC` 0x5FDC00), clears
//! the whole surface and draws the unit's name and values with GDI, and gives it back (`UiSurfaceReleaseDC` 0x5FDC60)
//! after the unit's model; each GetDC and ReleaseDC of a DirectDraw surface waits for the device. The fix lets the
//! panel draw into a 32-bit memory DC of its own and copies the result to the surface, through the game's own GetDC
//! and ReleaseDC, only when it differs from what the surface already holds.
#![expect(
    unsafe_code,
    reason = "game-specific calls: the panel surface's DC and the DirectDraw lock"
)]

use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use eix_engine::game::mfc;
use eix_engine::game::surface::{enter_directdraw_lock, leave_directdraw_lock};
use eix_engine::game::ui::Surface;
use eix_engine::gdi::{self, Dib, MemoryDc};
use eix_engine::hook;
use eix_sites::Site;

use crate::groups::{BuildError, Labelled};
use crate::log;
use crate::setup::Setup;

/// `CHudUnitPanel::DrawVanilla` gets the panel surface's DC: `call UiSurfaceGetDC`.
const GET_DC: Site = eix_sites::C_HUD_UNIT_PANEL_DRAW_VANILLA__145;
const GET_DC_BYTES: [u8; 5] = [0xE8, 0x16, 0x78, 0xFB, 0xFF];
/// ... and gives it back after the text: `call UiSurfaceReleaseDC`.
const RELEASE_DC: Site = eix_sites::C_HUD_UNIT_PANEL_DRAW_VANILLA__878;
const RELEASE_DC_BYTES: [u8; 5] = [0xE8, 0x43, 0x71, 0xFB, 0xFF];

/// The panel's drawing between the two calls, and what its surface holds.
struct Panel {
    /// The memory DC lent out by [`get_dc`], dropped by [`release_dc`]; declared before `shadow`, which it selects.
    dc: Option<MemoryDc>,
    /// The panel's own drawing, for one surface.
    shadow: Option<Shadow>,
    /// This draw goes the game's way (no holder, not 32 bits, no DIB or DC).
    game_way: bool,
}

/// The off-surface drawing of one panel surface.
struct Shadow {
    /// The surface it stands for: its holder, its DirectDraw surface, its size.
    key: (usize, usize, u32, u32),
    dib: Dib,
    /// What the surface holds, as last copied there; none before the first copy.
    copied: Option<Vec<u32>>,
}

static PANEL: Mutex<Panel> = Mutex::new(Panel {
    dc: None,
    shadow: None,
    game_way: true,
});
/// The first drawing is logged.
static LOGGED: AtomicBool = AtomicBool::new(false);

/// Stands in for DrawVanilla's `UiSurfaceGetDC`: takes the game's DirectDraw lock as the game's does, and lends the
/// panel a memory DC over its own 32-bit drawing, in GetDC's place in the surface; the game's way when that cannot be.
extern "thiscall" fn get_dc(this: *mut Surface) -> *mut c_void {
    let Ok(mut panel) = PANEL.lock() else {
        // SAFETY: the game's own call.
        return unsafe { Surface::get_dc(this) };
    };
    // SAFETY: DrawVanilla passes its live panel surface, on the game's thread.
    let dc = unsafe { lend_dc(&mut panel, this) };
    panel.game_way = dc.is_null();
    if panel.game_way {
        // SAFETY: the game's own call.
        return unsafe { Surface::get_dc(this) };
    }
    if !LOGGED.swap(true, Ordering::Relaxed) {
        log::info(
            "fixes",
            "UnitPanelText: the unit panel draws its text off the surface",
            "",
        );
    }
    // SAFETY: the game's DirectDraw lock, held from GetDC to ReleaseDC as the game's GetDC holds it (0x5FDC17).
    unsafe { enter_directdraw_lock() };
    // SAFETY: the game's thread; `dc` is live until `release_dc`.
    unsafe { mfc::cdc_from_handle(dc) }
}

/// A memory DC over the panel's 32-bit drawing for `this`, stored in its DC field; null when there is none.
///
/// # Safety
/// `this` must be the live panel surface, on the game's thread.
unsafe fn lend_dc(panel: &mut Panel, this: *mut Surface) -> *mut c_void {
    // SAFETY: the caller guarantees the surface.
    let surface = unsafe { &mut *this };
    // SAFETY: a holder the surface owns, or null.
    let Some(holder) = (unsafe { surface.holder.as_ref() }) else {
        return core::ptr::null_mut();
    };
    if holder.bit_count != 32 || holder.surface.is_null() {
        return core::ptr::null_mut();
    }
    let key = (
        surface.holder as usize,
        holder.surface as usize,
        holder.width,
        holder.height,
    );
    if panel.shadow.as_ref().is_none_or(|s| s.key != key) {
        panel.shadow = Dib::new(holder.width, holder.height).map(|dib| Shadow {
            key,
            dib,
            copied: None,
        });
    }
    let Some(shadow) = &panel.shadow else {
        return core::ptr::null_mut();
    };
    let Some(dc) = MemoryDc::new(&shadow.dib) else {
        return core::ptr::null_mut();
    };
    surface.dc = dc.handle();
    let handle = dc.handle();
    panel.dc = Some(dc);
    handle
}

/// Stands in for DrawVanilla's `UiSurfaceReleaseDC`: copies the panel's drawing to its surface through the game's
/// GetDC and ReleaseDC when it differs from what the surface holds, takes the memory DC back and leaves the lock.
extern "thiscall" fn release_dc(this: *mut Surface) {
    let Ok(mut panel) = PANEL.lock() else {
        // SAFETY: the game's own call.
        return unsafe { Surface::release_dc(this) };
    };
    if panel.game_way {
        drop(panel);
        // SAFETY: the game's own call, for the DC its GetDC got.
        return unsafe { Surface::release_dc(this) };
    }
    let dc = panel.dc.take();
    if let (Some(dc), Some(shadow)) = (&dc, panel.shadow.as_mut()) {
        let drawn = shadow.dib.pixels();
        if shadow.copied.as_deref() != Some(drawn) {
            // SAFETY: the live panel surface; the game's GetDC puts the surface's DC in its DC field.
            unsafe { Surface::get_dc(this) };
            // SAFETY: as above.
            let target = unsafe { (*this).dc };
            // SAFETY: a live DC of the surface, which is the drawing's size.
            let copied = !target.is_null()
                && unsafe { gdi::copy_to(target, dc, shadow.dib.width(), shadow.dib.height()) };
            // SAFETY: gives back the DC the game's GetDC got.
            unsafe { Surface::release_dc(this) };
            // A failed copy leaves the surface's content unknown: copy again next time.
            shadow.copied = copied.then(|| drawn.to_vec());
        }
    }
    drop(dc);
    // SAFETY: the live panel surface; no DC is out any more.
    unsafe { (*this).dc = core::ptr::null_mut() };
    // SAFETY: entered by `get_dc`, on this thread.
    unsafe { leave_directdraw_lock() };
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let (get, _) = hook::redirect_call(GET_DC, GET_DC_BYTES, get_dc as *const () as usize)?;
    let (release, _) = hook::redirect_call(
        RELEASE_DC,
        RELEASE_DC_BYTES,
        release_dc as *const () as usize,
    )?;
    Ok(vec![
        Labelled {
            label: "UnitPanelText #0".to_owned(),
            write: get,
        },
        Labelled {
            label: "UnitPanelText #1".to_owned(),
            write: release,
        },
    ])
}

#[cfg(test)]
mod tests {
    use eix_engine::game::ui;

    use super::*;

    #[test]
    fn the_calls_are_the_surface_dc_calls() {
        for (site, bytes, old) in [
            (GET_DC, GET_DC_BYTES, ui::SURFACE_GET_DC),
            (RELEASE_DC, RELEASE_DC_BYTES, ui::SURFACE_RELEASE_DC),
        ] {
            let rel = i32::from_le_bytes(bytes[1..5].try_into().unwrap());
            assert_eq!(bytes[0], 0xE8);
            assert_eq!(
                Site::new((site.offset(5)).ru().wrapping_add_signed(rel as isize)),
                old
            );
        }
    }
}
