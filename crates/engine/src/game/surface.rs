//! The DirectDraw surfaces the interface and the cursor draw on: the device, the surface holder `CDXSurface`, and the
//! lock game.exe takes around its DirectDraw calls on them.
//!
//! Names follow ei-rebuild.
#![expect(unsafe_code, reason = "the game's DirectDraw lock")]

use core::ffi::c_void;
use core::mem::offset_of;

use windows_sys::Win32::System::Threading::{
    CRITICAL_SECTION, EnterCriticalSection, LeaveCriticalSection,
};

use super::Global;

/// `CTextureDevice`, the DirectDraw device; never held by value.
#[repr(C)]
#[derive(Debug)]
pub struct TextureDevice {
    _unknown0: [u8; 0xC],
    /// The `IDirectDraw7`: `CDXSurface::Construct` 0x4EF7B0 creates its surface through it (0x4EF8DB).
    pub ddraw: *mut c_void,
    _unknown10: [u8; 4],
    /// The primary surface's holder: `CDXSurface::Construct` takes the pixel format from it (0x4EF846), and the
    /// cursor thread draws on it (0x5E56FF).
    pub primary: *mut DxSurface,
    /// The render target's holder (the back buffer): `CursorDrawFrame` 0x5E54A0 saves the background from it and
    /// draws the cursor on it (0x5E5512, 0x5E5589).
    pub render_target: *mut DxSurface,
    _unknown1c: [u8; 4],
    /// Bit 0, [`DEVICE_FULL_SCREEN`]: the device owns the display, so `DevicePresent` 0x4EF610 flips (0x4EF627) and
    /// focus loss gives the display back (`CTextureDevice::RestoreDisplayMode` 0x4EEC60, called at 0x457E62);
    /// windowed it blits.
    pub flags: u32,
}

const _: () = {
    assert!(offset_of!(TextureDevice, _unknown0) == 0x0);
    assert!(offset_of!(TextureDevice, ddraw) == 0xC);
    assert!(offset_of!(TextureDevice, _unknown10) == 0x10);
    assert!(offset_of!(TextureDevice, primary) == 0x14);
    assert!(offset_of!(TextureDevice, render_target) == 0x18);
    assert!(offset_of!(TextureDevice, _unknown1c) == 0x1C);
    assert!(offset_of!(TextureDevice, flags) == 0x20);
};

/// [`TextureDevice::flags`]: full screen.
pub const DEVICE_FULL_SCREEN: u32 = 1;

/// The device the game draws with: `WindowProc` 0x457D40 reads it at 0x457E53 to give the display back on focus
/// loss; null before the renderer exists.
pub const DEVICE: Global<*mut TextureDevice> = Global::at(eix_sites::DATA_7AF080);

/// Whether the game runs in a window: its device exists and does not own the display. False before the renderer
/// exists, as nothing tells the modes apart then.
///
/// # Safety
///
/// Reads the game's global: the caller runs on the game's thread, or none writes it meanwhile.
pub unsafe fn is_windowed() -> bool {
    // SAFETY: the caller's guarantee covers the global; it holds the device or null.
    let device = unsafe { DEVICE.read() };
    // SAFETY: a non-null device pointer is the game's live CTextureDevice.
    !device.is_null() && unsafe { (*device).flags } & DEVICE_FULL_SCREEN == 0
}

/// `CDXSurface`, a reference-counted holder of one DirectDraw surface (0x20 bytes: `push 0x20; call operator new` at
/// 0x5FCC90); never held by value.
#[repr(C)]
#[derive(Debug)]
pub struct DxSurface {
    _vtable: *const usize,
    _references: u32,
    /// The last DirectDraw call's HRESULT.
    pub result: i32,
    /// The device: `CDXSurface::Construct` 0x4EF7B0 stores its first argument (0x4EF810).
    pub device: *mut TextureDevice,
    /// The `IDirectDrawSurface7`: Construct stores what `IDirectDraw7::CreateSurface` made (0x4EF8FD).
    pub surface: *mut c_void,
    /// Its width, from `GetSurfaceDesc` (0x4EF962).
    pub width: u32,
    /// Its height (0x4EF95F).
    pub height: u32,
    /// Its bits per pixel (0x4EF965).
    pub bit_count: u32,
}

const _: () = {
    assert!(offset_of!(DxSurface, _vtable) == 0x0);
    assert!(offset_of!(DxSurface, _references) == 0x4);
    assert!(offset_of!(DxSurface, result) == 0x8);
    assert!(offset_of!(DxSurface, device) == 0xC);
    assert!(offset_of!(DxSurface, surface) == 0x10);
    assert!(offset_of!(DxSurface, width) == 0x14);
    assert!(offset_of!(DxSurface, height) == 0x18);
    assert!(offset_of!(DxSurface, bit_count) == 0x1C);
    assert!(size_of::<DxSurface>() == 0x20);
};

/// The lock game.exe takes around its DirectDraw calls on the interface's and the cursor's surfaces, which the cursor
/// thread draws on too: the cursor's blit helper 0x5E5643, `UiSurfaceGetDC` 0x5FDC0C (left in `UiSurfaceReleaseDC`
/// 0x5FDC6F), `CDXSurface::Construct` 0x4EF8B8.
pub const DIRECTDRAW_LOCK: Global<CRITICAL_SECTION> = Global::at(eix_sites::DATA_7AEE78);

/// Enters [`DIRECTDRAW_LOCK`], which is recursive, as the game's nesting needs.
///
/// # Safety
/// The process must be a supported game.exe after the lock's initialisation (before WinMain's first frame); the thread
/// must leave it as often as it enters it.
pub unsafe fn enter_directdraw_lock() {
    // SAFETY: the caller guarantees the build and an initialised lock.
    unsafe { EnterCriticalSection(DIRECTDRAW_LOCK.as_ptr()) };
}

/// Leaves [`DIRECTDRAW_LOCK`].
///
/// # Safety
/// The thread must have entered it.
pub unsafe fn leave_directdraw_lock() {
    // SAFETY: the caller guarantees the entry.
    unsafe { LeaveCriticalSection(DIRECTDRAW_LOCK.as_ptr()) };
}
