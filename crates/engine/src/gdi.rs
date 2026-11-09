//! GDI drawing: a 32-bit DIB section and a memory device context drawing into it, and the few calls a hook makes on
//! the game's own device contexts.
#![expect(
    unsafe_code,
    reason = "CreateDIBSection, the memory DC calls, BitBlt, StretchDIBits, the DC state calls and DeleteObject"
)]

use core::ffi::c_void;

use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, HALFTONE, HBITMAP, HDC, HGDIOBJ, IntersectClipRect,
    RestoreDC, SRCCOPY, SaveDC, SelectObject, SetBrushOrgEx, SetStretchBltMode, StretchDIBits,
};

/// The header of a `width` x `height` 32-bit `BI_RGB` bitmap whose rows run top-down, as a surface's memory runs.
fn top_down(width: i32, height: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative: rows top-down.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [windows_sys::Win32::Graphics::Gdi::RGBQUAD {
            rgbBlue: 0,
            rgbGreen: 0,
            rgbRed: 0,
            rgbReserved: 0,
        }],
    }
}

/// A top-down 32-bit DIB section (`BI_RGB`: blue in the low byte, the top byte unused), as GDI draws into a
/// 32-bit DirectDraw surface's device context.
#[derive(Debug)]
pub struct Dib {
    bitmap: HBITMAP,
    bits: *mut u32,
    width: u32,
    height: u32,
}

// SAFETY: a GDI bitmap is a process-wide handle and its bits are plain memory; the holder decides which thread draws.
unsafe impl Send for Dib {}

impl Dib {
    /// A `width` x `height` DIB section, all zero; None when GDI refuses it or the size is 0 or too large.
    pub fn new(width: u32, height: u32) -> Option<Self> {
        let (Ok(w), Ok(h)) = (i32::try_from(width), i32::try_from(height)) else {
            return None;
        };
        if w == 0 || h == 0 {
            return None;
        }
        let info = top_down(w, h);
        let mut bits: *mut c_void = core::ptr::null_mut();
        // SAFETY: a valid BITMAPINFO; no section handle, so GDI allocates the (zeroed) bits.
        let bitmap = unsafe {
            CreateDIBSection(
                core::ptr::null_mut(),
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                core::ptr::null_mut(),
                0,
            )
        };
        if bitmap.is_null() || bits.is_null() {
            if !bitmap.is_null() {
                // SAFETY: the bitmap made above, selected nowhere.
                unsafe { DeleteObject(bitmap) };
            }
            return None;
        }
        Some(Self {
            bitmap,
            bits: bits.cast(),
            width,
            height,
        })
    }

    /// Its width.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Its height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Its pixels, row after row; GDI calls on a [`MemoryDc`] may change them.
    pub fn pixels(&self) -> &[u32] {
        // SAFETY: a 32-bit DIB section's rows have no padding: width x height words, alive while the bitmap is.
        unsafe {
            core::slice::from_raw_parts(self.bits, self.width as usize * self.height as usize)
        }
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        // SAFETY: the bitmap this owns; a memory DC that still selects it keeps it (DeleteObject then fails).
        unsafe { DeleteObject(self.bitmap) };
    }
}

/// A memory device context drawing into a [`Dib`]; drop it before the DIB.
#[derive(Debug)]
pub struct MemoryDc {
    dc: HDC,
    old: HGDIOBJ,
}

// SAFETY: a DC is a process-wide handle; one thread at a time must use it, which its holder sees to.
unsafe impl Send for MemoryDc {}

impl MemoryDc {
    /// A fresh memory DC (GDI's default state) with `dib` selected; None when GDI refuses one.
    pub fn new(dib: &Dib) -> Option<Self> {
        // SAFETY: no preconditions: a memory DC compatible with the screen.
        let dc = unsafe { CreateCompatibleDC(core::ptr::null_mut()) };
        if dc.is_null() {
            return None;
        }
        // SAFETY: a fresh DC and a live bitmap selected into no other DC.
        let old = unsafe { SelectObject(dc, dib.bitmap) };
        if old.is_null() {
            // SAFETY: the DC made above.
            unsafe { DeleteDC(dc) };
            return None;
        }
        Some(Self { dc, old })
    }

    /// The DC's handle.
    pub fn handle(&self) -> *mut c_void {
        self.dc
    }
}

impl Drop for MemoryDc {
    fn drop(&mut self) {
        // SAFETY: the DC this owns; the bitmap it had first goes back in, which releases the DIB.
        unsafe { SelectObject(self.dc, self.old) };
        // SAFETY: as above.
        unsafe { DeleteDC(self.dc) };
    }
}

/// Copies the `width` x `height` pixels at the origin of `src` to the origin of the device context `dst`
/// (`BitBlt`, `SRCCOPY`); false when GDI fails it.
///
/// # Safety
/// `dst` must be a live device context.
pub unsafe fn copy_to(dst: *mut c_void, src: &MemoryDc, width: u32, height: u32) -> bool {
    let (Ok(w), Ok(h)) = (i32::try_from(width), i32::try_from(height)) else {
        return false;
    };
    // SAFETY: the caller guarantees `dst`; `src` is live.
    unsafe { BitBlt(dst, 0, 0, w, h, src.dc, 0, 0, SRCCOPY) != 0 }
}

/// Saves the state of the device context `dc` (`SaveDC`) and returns its level for [`restore`]; 0 when GDI fails.
///
/// # Safety
/// `dc` must be a live device context.
pub unsafe fn save(dc: *mut c_void) -> i32 {
    // SAFETY: the caller guarantees `dc`.
    unsafe { SaveDC(dc) }
}

/// Restores the state of `dc` that [`save`] returned `level` for (`RestoreDC`).
///
/// # Safety
/// `dc` must be a live device context that saved `level`.
pub unsafe fn restore(dc: *mut c_void, level: i32) {
    // SAFETY: the caller guarantees `dc` and the level.
    unsafe { RestoreDC(dc, level) };
}

/// Narrows the clip region of `dc` to the rectangle `[left, top, right, bottom]` (`IntersectClipRect`).
///
/// # Safety
/// `dc` must be a live device context.
pub unsafe fn clip(dc: *mut c_void, rect: [i32; 4]) {
    let [left, top, right, bottom] = rect;
    // SAFETY: the caller guarantees `dc`.
    unsafe { IntersectClipRect(dc, left, top, right, bottom) };
}

/// Draws the `width` x `height` pixels (top row first, blue in the low byte) into the rectangle
/// `[left, top, right, bottom]` of `dc`, scaled with `HALFTONE` (`StretchDIBits`); false when GDI fails it or the
/// pixels do not match the size.
///
/// # Safety
/// `dc` must be a live device context.
pub unsafe fn draw_scaled(
    dc: *mut c_void,
    rect: [i32; 4],
    width: u32,
    height: u32,
    pixels: &[u32],
) -> bool {
    let (Ok(w), Ok(h)) = (i32::try_from(width), i32::try_from(height)) else {
        return false;
    };
    if pixels.len() != width as usize * height as usize {
        return false;
    }
    let info = top_down(w, h);
    let [left, top, right, bottom] = rect;
    // SAFETY: the caller guarantees `dc`.
    let mode = unsafe { SetStretchBltMode(dc, HALFTONE) };
    // SAFETY: as above; HALFTONE asks for the brush origin to be set after the mode.
    unsafe { SetBrushOrgEx(dc, 0, 0, core::ptr::null_mut()) };
    // SAFETY: as above; `info` describes `pixels`, which hold width x height words.
    let lines = unsafe {
        StretchDIBits(
            dc,
            left,
            top,
            right - left,
            bottom - top,
            0,
            0,
            w,
            h,
            pixels.as_ptr().cast(),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        )
    };
    if mode != 0 {
        // SAFETY: as above: the mode the DC had.
        unsafe { SetStretchBltMode(dc, mode as _) };
    }
    lines != 0
}
