//! DirectDraw 7, as far as eix calls it on the game's behalf: the display modes of an `IDirectDraw7` and setting one.
#![expect(
    unsafe_code,
    reason = "calls through an IDirectDraw7's vtable and its EnumDisplayModes callback"
)]

use alloc::vec::Vec;
use core::ffi::c_void;
use core::mem::offset_of;

/// `IDirectDraw7::EnumDisplayModes`'s vtable slot (byte offset 0x20).
const ENUM_DISPLAY_MODES: usize = 8;
/// `IDirectDraw7::SetDisplayMode`'s vtable slot (byte offset 0x54), the one game.exe calls at 0x4EDEF8.
const SET_DISPLAY_MODE: usize = 21;
/// `DDENUMRET_OK`: go on with the next mode.
const DDENUMRET_OK: i32 = 1;

/// A display mode: its size and bits per pixel. The same shape as core's `engine_fixes::DisplayMode`, which the
/// engine cannot use: it builds without std, and core with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bits per pixel.
    pub bits: u32,
}

/// `DDSURFACEDESC2`, with the fields a display mode fills named.
#[repr(C)]
struct SurfaceDesc2 {
    size: u32,
    flags: u32,
    height: u32,
    width: u32,
    _unknown10: [u8; 0x48 - 0x10],
    /// `DDPIXELFORMAT`: `dwSize`, `dwFlags`, `dwFourCC`, then `dwRGBBitCount`.
    _pixel_format_head: [u32; 3],
    rgb_bit_count: u32,
    _unknown58: [u8; 0x7C - 0x58],
}

const _: () = {
    assert!(offset_of!(SurfaceDesc2, height) == 0x8);
    assert!(offset_of!(SurfaceDesc2, width) == 0xC);
    assert!(offset_of!(SurfaceDesc2, rgb_bit_count) == 0x54);
    assert!(size_of::<SurfaceDesc2>() == 0x7C);
};

type EnumCallback = unsafe extern "system" fn(*mut SurfaceDesc2, *mut c_void) -> i32;
type EnumDisplayModes = unsafe extern "system" fn(
    *mut c_void,
    u32,
    *mut SurfaceDesc2,
    *mut c_void,
    EnumCallback,
) -> i32;
type SetDisplayMode = unsafe extern "system" fn(*mut c_void, u32, u32, u32, u32, u32) -> i32;

/// The function in `slot` of the vtable of the COM object `this`.
///
/// # Safety
/// `this` must be a live `IDirectDraw7`.
unsafe fn method(this: *mut c_void, slot: usize) -> usize {
    // SAFETY: a COM object's first word is its vtable (caller's guarantee).
    let vtable = unsafe { *this.cast::<*const usize>() };
    // SAFETY: IDirectDraw7's vtable has more than `slot` entries; both callers pass a slot of it.
    let entry = unsafe { vtable.add(slot) };
    // SAFETY: a vtable entry is a readable function pointer.
    unsafe { *entry }
}

/// Collects each mode `EnumDisplayModes` reports into the `Vec<Mode>` at `context`.
unsafe extern "system" fn collect(desc: *mut SurfaceDesc2, context: *mut c_void) -> i32 {
    // SAFETY: DirectDraw passes a valid descriptor for the call.
    let desc = unsafe { &*desc };
    // SAFETY: `context` is the Vec `display_modes` handed over, alive and not otherwise borrowed while it enumerates.
    let modes = unsafe { &mut *context.cast::<Vec<Mode>>() };
    modes.push(Mode {
        width: desc.width,
        height: desc.height,
        bits: desc.rgb_bit_count,
    });
    DDENUMRET_OK
}

/// Every display mode `ddraw` offers, or the failing HRESULT of `EnumDisplayModes`.
///
/// # Safety
/// `ddraw` must be a live `IDirectDraw7`.
pub unsafe fn display_modes(ddraw: *mut c_void) -> Result<Vec<Mode>, i32> {
    let mut modes: Vec<Mode> = Vec::new();
    // SAFETY: the caller's guarantee.
    let address = unsafe { method(ddraw, ENUM_DISPLAY_MODES) };
    // SAFETY: slot 8 of IDirectDraw7 is EnumDisplayModes, of this type.
    let enumerate = unsafe { crate::hook::as_fn_at::<EnumDisplayModes>(address) };
    // SAFETY: no surface description filters the modes; the callback writes only into `modes` while this runs.
    let hr = unsafe {
        enumerate(
            ddraw,
            0,
            core::ptr::null_mut(),
            (&raw mut modes).cast(),
            collect,
        )
    };
    if hr < 0 { Err(hr) } else { Ok(modes) }
}

/// `IDirectDraw7::SetDisplayMode` with the game's arguments; its HRESULT.
///
/// # Safety
/// `ddraw` must be a live `IDirectDraw7` whose cooperative level allows a mode change.
pub unsafe fn set_display_mode(ddraw: *mut c_void, mode: Mode, refresh: u32, flags: u32) -> i32 {
    // SAFETY: the caller's guarantee.
    let address = unsafe { method(ddraw, SET_DISPLAY_MODE) };
    // SAFETY: slot 21 of IDirectDraw7 is SetDisplayMode, of this type.
    let set = unsafe { crate::hook::as_fn_at::<SetDisplayMode>(address) };
    // SAFETY: as above.
    unsafe { set(ddraw, mode.width, mode.height, mode.bits, refresh, flags) }
}
