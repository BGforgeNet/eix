//! The interface: `CInterface3D` and its static instance; the 2D layout's windows, regions and sprites.
//!
//! Names follow ei-rebuild.
#![expect(unsafe_code, reason = "calls into game.exe's interface code")]

use core::ffi::{CStr, c_char, c_void};
use core::mem::offset_of;

use super::console::ConsoleWindow;
use super::mfc::{GameArray, GameString};
use super::{Global, code};

/// A rectangle, in layout units or pixels: `CInterface3D::ScaleRectToScreen` 0x5F7CA0 scales the four words, x at +0
/// and +8, y at +4 and +C; the text draws take it in interface coordinates.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    /// Left.
    pub left: i32,
    /// Top.
    pub top: i32,
    /// Right.
    pub right: i32,
    /// Bottom.
    pub bottom: i32,
}

/// A point: `CInterface3D::ScalePointToScreen` 0x5F7D20 scales x at +0 and y at +4.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    /// X.
    pub x: i32,
    /// Y.
    pub y: i32,
}

const _: () = {
    assert!(offset_of!(Rect, bottom) == 0xC);
    assert!(size_of::<Rect>() == 0x10);
    assert!(offset_of!(Point, y) == 0x4);
};

/// `CUiRegion`: a window's clickable area, a rectangle or a polygon.
#[repr(C)]
#[derive(Debug)]
pub struct UiRegion {
    _unknown0: [u8; 4],
    /// The polygon's vertex block, or null: `CUiRegion::SetRect` 0x5FC9E0 frees the allocation whose address the
    /// word in front of the block holds (0x5FC9EA) and clears this.
    pub points: *mut Point,
    /// The end of the vertices in use: `SetRect` clears it (0x5FCA10); `CUiRegion::AddPoint` 0x5FCA70 starts the
    /// bounds when it is null (0x5FCA79).
    pub points_end: *mut Point,
    /// The end of the vertex block: `SetRect` clears it (0x5FCA09).
    pub points_capacity: *mut Point,
    _unknown10: [u8; 4],
    /// The bounds in layout units: `SetRect` stores its four arguments (0x5FCA17..0x5FCA26).
    pub bounds: Rect,
    /// The region is its rectangle: `SetRect` sets it (0x5FCA2C).
    pub rect_only: u8,
    /// The region takes clicks: `SetRect` sets it (0x5FCA29).
    pub enabled: u8,
}

const _: () = {
    assert!(offset_of!(UiRegion, points) == 0x4);
    assert!(offset_of!(UiRegion, points_end) == 0x8);
    assert!(offset_of!(UiRegion, points_capacity) == 0xC);
    assert!(offset_of!(UiRegion, bounds) == 0x14);
    assert!(offset_of!(UiRegion, rect_only) == 0x24);
    assert!(offset_of!(UiRegion, enabled) == 0x25);
};

/// `CUiMesh`: a sprite of a window; never held by value.
#[repr(C)]
#[derive(Debug)]
pub struct UiMesh {
    _unknown0: [u8; 0x1C],
    /// Where the sprite is drawn, in view units, and its depth: `CUiMesh::PlaceBody` 0x5E6FA0 stores them
    /// (0x5E6FE6, 0x5E6FF8, 0x5E6FFB).
    pub place: [f32; 3],
    _unknown28: [u8; 0x220 - 0x28],
    /// Drawn when non-zero: the sprite's draw 0x5E6D90 skips it at 0 (0x5E6D9C).
    pub visible: u32,
    /// The colour factors the draw multiplies by (0x5E6E8C..0x5E6E9B).
    pub tint: [f32; 3],
}

const _: () = {
    assert!(offset_of!(UiMesh, place) == 0x1C);
    assert!(offset_of!(UiMesh, visible) == 0x220);
    assert!(offset_of!(UiMesh, tint) == 0x224);
};

impl UiMesh {
    /// Shapes the sprite as the rectangle (`left`, `top`)-(`right`, `bottom`) in layout units at `depth`, showing the
    /// texels (`u0`, `v0`)-(`u1`, `v1`): `CUiMesh::SetRectangle` 0x5E70E0 (thiscall, `ret 0x28`).
    ///
    /// # Safety
    /// The process must be game.exe RU 1.06, on the game's thread; `this` must be a live sprite.
    #[expect(
        clippy::too_many_arguments,
        reason = "the game function's own ten arguments"
    )]
    pub unsafe fn set_rectangle(
        this: *mut Self,
        depth: f32,
        scale: f32,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        texels: [i32; 4],
    ) {
        type SetRectangle = unsafe extern "thiscall" fn(
            *mut UiMesh,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            i32,
            i32,
            i32,
            i32,
        );
        // SAFETY: 0x5E70E0 takes the sprite in ECX and ten stack words, which it cleans (`ret 0x28` at 0x5E7674).
        let f = unsafe { code::<SetRectangle>(eix_sites::C_UI_MESH_SET_RECTANGLE) };
        let [u0, v0, u1, v1] = texels;
        // SAFETY: the caller guarantees the build, the thread and the sprite.
        unsafe { f(this, depth, scale, left, top, right, bottom, u0, v0, u1, v1) }
    }
}

/// `CUiWindow`, the head every interface window shares; never held by value.
#[repr(C)]
#[derive(Debug)]
pub struct UiWindow {
    _unknown0: [u8; 0xD8],
    /// The window's rectangle in layout units: `CHudClock::Setup` 0x648520 sets it (0x64852B..0x648545).
    pub rect: Rect,
    _unknowne8: [u8; 0x108 - 0xE8],
    /// The sprites: `CUiWindow::AddSprite` 0x5FF3C0 appends the new one to this block (0x5FF3EB).
    pub sprites: *mut *mut UiMesh,
    /// How many sprites there are (0x5FF3DF).
    pub sprite_count: i32,
}

const _: () = {
    assert!(offset_of!(UiWindow, rect) == 0xD8);
    assert!(offset_of!(UiWindow, sprites) == 0x108);
    assert!(offset_of!(UiWindow, sprite_count) == 0x10C);
};

impl UiWindow {
    /// The sprite at `index`, or null when there is none.
    ///
    /// # Safety
    /// `this` must be a live window on the game's thread.
    pub unsafe fn sprite(this: *const Self, index: usize) -> *mut UiMesh {
        // SAFETY: the caller guarantees a live window.
        let w = unsafe { &*this };
        match usize::try_from(w.sprite_count) {
            Ok(n) if index < n && !w.sprites.is_null() => {
                // SAFETY: a live window holds `sprite_count` sprite pointers at `sprites`; `index` is below it.
                let at = unsafe { w.sprites.add(index) };
                // SAFETY: as above.
                unsafe { *at }
            }
            _ => core::ptr::null_mut(),
        }
    }

    /// Adds a sprite of the model `name` (null: a blank one) and returns it, null when none is made:
    /// `CUiWindow::AddSprite` 0x5FF3C0 (thiscall, `ret 4`).
    ///
    /// # Safety
    /// The process must be game.exe RU 1.06, on the game's thread; `this` must be a live window.
    pub unsafe fn add_sprite(this: *mut Self, name: *const c_char) -> *mut UiMesh {
        // SAFETY: 0x5FF3C0 takes the window in ECX and one stack word, which it cleans (`ret 4` at 0x5FF3DC).
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut UiWindow, *const c_char) -> *mut UiMesh>(
                eix_sites::C_UI_WINDOW_ADD_SPRITE,
            )
        };
        // SAFETY: the caller guarantees the build, the thread and the window.
        unsafe { f(this, name) }
    }
}

/// `CHudUnitPanel`: the battle screen's unit panel, which slides away; never held by value.
#[repr(C)]
#[derive(Debug)]
pub struct HudUnitPanel {
    /// The window.
    pub window: UiWindow,
    _unknown110: [u8; 0x16C - 0x110],
    /// The panel slides: `CHudUnitPanel::Setup` 0x6456E0 sets 1 (0x64575D).
    pub sliding: u32,
    /// The slide offset in pixels: Setup stores 0, or `travel` for a panel that starts slid (0x64571D).
    pub offset: i32,
    /// The full slide distance: Setup stores 0xB4 (0x645767).
    pub travel: i32,
    /// How far the panel has slid, 0..1: Setup stores the start flag (0x645734).
    pub slide: f32,
    /// The slide's direction, 1 or -1 by the start flag (0x645771).
    pub slide_direction: f32,
}

const _: () = {
    assert!(offset_of!(HudUnitPanel, sliding) == 0x16C);
    assert!(offset_of!(HudUnitPanel, offset) == 0x170);
    assert!(offset_of!(HudUnitPanel, travel) == 0x174);
    assert!(offset_of!(HudUnitPanel, slide) == 0x178);
    assert!(offset_of!(HudUnitPanel, slide_direction) == 0x17C);
};

/// `CHelpWindow`: a help topic's or a tutorial message's pages; never held by value.
#[repr(C)]
#[derive(Debug)]
pub struct HelpWindow {
    _unknown0: [u8; 0x164],
    /// A message window, not help: `CHelpWindow::ShowPage` 0x659420 tests it (0x6596A7).
    pub message: u8,
    _unknown165: [u8; 3],
    /// The message's or topic's text key (a `CString`'s characters): ShowPage opens its section (0x6594E9).
    pub text: *const c_char,
    _unknown16c: [u8; 0x184 - 0x16C],
    /// The page shown: ShowPage seeks its `#screen` line (0x659557).
    pub page: i32,
}

const _: () = {
    assert!(offset_of!(HelpWindow, message) == 0x164);
    assert!(offset_of!(HelpWindow, text) == 0x168);
    assert!(offset_of!(HelpWindow, page) == 0x184);
};

/// `CBriefingScreen`, the base and village screen of a "brief" zone; never held by value.
#[repr(C)]
#[derive(Debug)]
pub struct BriefingScreen {
    _unknown0: [u8; 0x184],
    /// The dialogue's topic: `CBriefingScreen::OnBriefing` 0x6030C0 assigns it (0x6030CE), and
    /// `CBriefingScreen::StartPhrases` 0x608210 opens the text `briefing <topic>` (0x60828B-0x6082AC).
    pub topic: GameString,
}

const _: () = assert!(offset_of!(BriefingScreen, topic) == 0x184);

/// The 2D layout's constants in game.exe's `.rdata`, read by `CUiMesh::PlaceBody` 0x5E6FA0 and the shape builders:
/// layout x and y to view units are `(v * LAYOUT_TO_VIEW - centre) * VIEW_SCALE * depth`.
pub mod layout {
    use super::Global;

    /// 1/400: layout units to view units (0x5E6FA7).
    pub const LAYOUT_TO_VIEW: Global<f32> = Global::at(eix_sites::RDATA_74015C);
    /// 1/800: half of it, for a sum of two x values (`CUiMesh::SetRectangle` 0x5E75D1).
    pub const HALF_LAYOUT_TO_VIEW: Global<f32> = Global::at(eix_sites::RDATA_740164);
    /// 1.0: the view's x centre (0x5E6FB8).
    pub const CENTRE_X: Global<f32> = Global::at(eix_sites::RDATA_73B84C);
    /// 0.75: the view's y centre (0x5E6FDA).
    pub const CENTRE_Y: Global<f32> = Global::at(eix_sites::RDATA_73C524);
    /// The view's scale (0x5E6FBE).
    pub const VIEW_SCALE: Global<f32> = Global::at(eix_sites::RDATA_740158);
}

/// The head of `CInterface3D`; never held by value.
#[repr(C)]
#[derive(Debug)]
pub struct Interface {
    _unknown0: [u8; 0x14],
    /// The console window, or null: `CInterface3D::ConsoleAddText` 0x5F7EB0 passes the text on to it.
    pub console: *mut ConsoleWindow,
}

const _: () = {
    assert!(offset_of!(Interface, console) == 0x14);
};

/// The interface, constructed in place by its static initialiser (0x5F4685 calls `CInterface3D::Construct`
/// 0x5F46A0).
pub const INTERFACE: Global<Interface> = Global::at(eix_sites::DATA_7C7D50);

/// `UiSurface`, a window's drawing surface: its own DirectDraw surface in system memory, which the text draws reach
/// through a device context; never held by value.
#[repr(C)]
#[derive(Debug)]
pub struct Surface {
    _vtable: *const usize,
    _unknown4: [u8; 4],
    /// The device context the draws use while one is out: `UiSurfaceGetDC` 0x5FDC00 has the surface's stored here
    /// (0x5FDC20) and `UiSurfaceReleaseDC` 0x5FDC60 clears it (0x5FDC9A); `CUiSurface::Clear` 0x5FE2F0 draws through
    /// it (0x5FE2FD).
    pub dc: *mut c_void,
    _flags: u32,
    /// The surface's holder, of the surface's size (`CUiSurface::Construct` 0x5FCC00 stores it at 0x5FCCDC); null
    /// while `CUiSurface::SwapContents` 0x5FD7A0 keeps the pixels in memory. Without it GetDC returns null (0x5FDC07)
    /// and ReleaseDC only clears the DC field (0x5FDC69).
    pub holder: *mut super::surface::DxSurface,
}

const _: () = {
    assert!(offset_of!(Surface, dc) == 0x8);
    assert!(offset_of!(Surface, holder) == 0x10);
};

/// `UiSurface::DrawTextIn` 0x5FDE20: draws `text` within `rect` with a font, a colour and DrawText format bits
/// (thiscall, `ret 0x18`); the connect screen's list calls it for each label (0x61DC7F).
pub type DrawTextIn =
    unsafe extern "thiscall" fn(*mut Surface, *const u8, *mut Rect, i32, u32, u32, i32) -> i32;

/// The address of [`DrawTextIn`].
pub const DRAW_TEXT_IN: usize = eix_sites::C_UI_SURFACE_DRAW_TEXT_IN;

/// `UiSurfaceGetDC` 0x5FDC00: takes [`super::surface::DIRECTDRAW_LOCK`], gets the surface's device context into
/// [`Surface::dc`] (retrying while DirectDraw answers DDERR_WASSTILLDRAWING) and returns its MFC `CDC` (thiscall,
/// returns the `CDC*` or null; without a holder it takes nothing and returns null).
pub const SURFACE_GET_DC: usize = eix_sites::UI_SURFACE_GET_DC;
/// `UiSurfaceReleaseDC` 0x5FDC60: gives the device context back, clears [`Surface::dc`] and leaves the lock
/// (thiscall).
pub const SURFACE_RELEASE_DC: usize = eix_sites::UI_SURFACE_RELEASE_DC;

impl Surface {
    /// Calls the game's [`SURFACE_GET_DC`].
    ///
    /// # Safety
    /// The process must be game.exe RU 1.06, on the game's thread; `this` must be a live surface without a device
    /// context out.
    pub unsafe fn get_dc(this: *mut Self) -> *mut c_void {
        // SAFETY: 0x5FDC00 is `CDC* __thiscall ()`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self) -> *mut c_void>(SURFACE_GET_DC)
        };
        // SAFETY: the caller guarantees the build, the thread and the surface.
        unsafe { f(this) }
    }

    /// Calls the game's [`SURFACE_RELEASE_DC`].
    ///
    /// # Safety
    /// As for [`Self::get_dc`], with the device context [`Self::get_dc`] got out.
    pub unsafe fn release_dc(this: *mut Self) {
        // SAFETY: 0x5FDC60 is `void __thiscall ()`.
        let f = unsafe { code::<unsafe extern "thiscall" fn(*mut Self)>(SURFACE_RELEASE_DC) };
        // SAFETY: the caller guarantees the build, the thread and the surface.
        unsafe { f(this) }
    }

    /// Draws `text` within `rect` over at most `max_lines` wrapped lines and returns the lines drawn:
    /// `UiSurface::DrawWrappedText` 0x5FDFA0 (thiscall, `ret 0x1C`).
    ///
    /// # Safety
    /// The process must be game.exe RU 1.06, on the game's thread; `this` must be a live surface and `text` a
    /// NUL-terminated string.
    #[expect(
        clippy::too_many_arguments,
        reason = "the arguments of the game's function, passed on as they are"
    )]
    pub unsafe fn draw_wrapped_text(
        this: *mut Self,
        text: *const u8,
        rect: *mut Rect,
        max_lines: i32,
        font: i32,
        colour: u32,
        format: u32,
        flag: i32,
    ) -> i32 {
        type Draw = unsafe extern "thiscall" fn(
            *mut Surface,
            *const u8,
            *mut Rect,
            i32,
            i32,
            u32,
            u32,
            i32,
        ) -> i32;
        // SAFETY: 0x5FDFA0 takes the text, the rectangle, the line limit, then DrawTextIn's last four arguments.
        let f = unsafe { code::<Draw>(eix_sites::UI_SURFACE_DRAW_WRAPPED_TEXT) };
        // SAFETY: the caller guarantees the build, the thread, the surface and the text.
        unsafe { f(this, text, rect, max_lines, font, colour, format, flag) }
    }
}

impl Interface {
    /// Adds `text` to the console window, when there is one: `CInterface3D::ConsoleAddText` 0x5F7EB0 (thiscall,
    /// `ret 4`).
    ///
    /// # Safety
    /// The process must be game.exe RU 1.06, on the game's thread; `this` must be [`INTERFACE`].
    pub unsafe fn console_add_text(this: *mut Self, text: &CStr) {
        // SAFETY: 0x5F7EB0 is `void __thiscall (const char*)`; it skips a null window.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *const u8)>(
                eix_sites::C_INTERFACE_3D_CONSOLE_ADD_TEXT,
            )
        };
        // SAFETY: the caller guarantees the build, the thread and the interface; `text` is NUL-terminated.
        unsafe { f(this, text.as_ptr().cast()) }
    }

    /// Plays an interface sound: `CInterface3D::PlaySound` 0x5F7E60 (thiscall, the file and 0 on the stack, `ret 8`).
    ///
    /// # Safety
    /// As for [`console_add_text`](Self::console_add_text).
    pub unsafe fn play_sound(this: *mut Self, file: &CStr) {
        // SAFETY: 0x5F7E60 is `void __thiscall (const char*, int)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *const u8, i32)>(
                eix_sites::C_INTERFACE_3D_PLAY_SOUND,
            )
        };
        // SAFETY: as for `console_add_text`; `file` is NUL-terminated.
        unsafe { f(this, file.as_ptr().cast(), 0) }
    }

    /// Opens `window` on top: `CInterface3D::PushWindow` 0x5F7350 (thiscall, the window, 1 and 1 on the stack,
    /// `ret 0xC`, as the addon calls it).
    ///
    /// # Safety
    /// As for [`console_add_text`](Self::console_add_text); `window` a constructed window the interface then owns.
    pub unsafe fn push_window(this: *mut Self, window: *mut c_void) {
        // SAFETY: 0x5F7350 is `void __thiscall (CUiWindow*, bool, bool)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *mut c_void, u32, u32)>(
                eix_sites::C_INTERFACE_3D_PUSH_WINDOW,
            )
        };
        // SAFETY: as for `console_add_text`.
        unsafe { f(this, window, 1, 1) }
    }
}

/// The size of a `CMessageBox`, as the addon allocates one for the game over window.
pub const MESSAGE_BOX_SIZE: usize = 0x180;

/// Constructs a message window in `block` (of [`MESSAGE_BOX_SIZE`] bytes from the game's heap) with the text keys
/// `title` and `text`, the style the game over window uses: `CMessageBox::Construct` 0x634650 (thiscall, the
/// interface, title, text, 0x20000, 3 and 1 on the stack, `ret 0x18`).
///
/// # Safety
/// The process must be game.exe RU 1.06, on the game's thread; `block` must be such a block holding no object.
pub unsafe fn construct_message_box(
    block: *mut c_void,
    interface: *mut Interface,
    title: &CStr,
    text: &CStr,
) {
    // SAFETY: 0x634650 is `CMessageBox* __thiscall (CInterface3D*, const char*, const char*, int, int, int)`.
    let f = unsafe {
        code::<
            unsafe extern "thiscall" fn(
                *mut c_void,
                *mut Interface,
                *const u8,
                *const u8,
                u32,
                u32,
                u32,
            ) -> *mut c_void,
        >(eix_sites::C_MESSAGE_BOX_CONSTRUCT)
    };
    // SAFETY: the caller guarantees the block and the thread; both texts are NUL-terminated.
    unsafe {
        f(
            block,
            interface,
            title.as_ptr().cast(),
            text.as_ptr().cast(),
            0x2_0000,
            3,
            1,
        )
    };
}

/// The head of a `CUiContainer` up to its children (the battle screen is one): `CUiContainer::AddChild` 0x600D4D
/// grows the array's count at +0x14C.
#[repr(C)]
#[derive(Debug)]
pub struct UiContainer {
    _unknown0: [u8; 0x144],
    /// The child windows (the battle screen's panels).
    pub children: GameArray<*mut c_void>,
}

const _: () = assert!(offset_of!(UiContainer, children) == 0x144);

/// Shows (`true`) or hides a window: `CUiWindow::SetEnabled` 0x6002A0 (thiscall, the flag and 0 on the stack,
/// `ret 8`).
///
/// # Safety
/// The process must be game.exe RU 1.06, on the game's thread; `window` must be a live window.
pub unsafe fn set_window_enabled(window: *mut c_void, enabled: bool) {
    // SAFETY: 0x6002A0 is `void __thiscall (bool, int)`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void, u32, u32)>(
            eix_sites::C_UI_WINDOW_SET_ENABLED,
        )
    };
    // SAFETY: the caller guarantees the window and the thread.
    unsafe { f(window, u32::from(enabled), 0) }
}

/// The mouse cursor (`CCursor`), constructed by its static initialiser.
pub const CURSOR: Global<c_void> = Global::at(eix_sites::DATA_7C7BD8);

/// Shows or hides the cursor's image: `CCursor::ShowImage` 0x5E5270 (thiscall, `ret 4`).
///
/// # Safety
/// The process must be game.exe RU 1.06, on the game's thread.
pub unsafe fn show_cursor(shown: bool) {
    // SAFETY: 0x5E5270 is `void __thiscall (bool)`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void, u32)>(eix_sites::C_CURSOR_SHOW_IMAGE)
    };
    // SAFETY: the caller guarantees the build and the thread; the cursor is a constructed static.
    unsafe { f(CURSOR.as_ptr(), u32::from(shown)) }
}
