//! The screens around the layout: the curtains over the camp's and the multiplayer hero screen's margins, the
//! tutorial frames that point at the moved panels, the interface fonts' size and the 3D view's proportions.
#![expect(
    unsafe_code,
    reason = "calling the game's screen builders and reading and changing their windows"
)]

use std::ffi::CStr;
use std::sync::atomic::{AtomicUsize, Ordering};

use eix_core::widescreen::{Layout, tutorial_frame_shift};
use eix_engine::game::ui::{HelpWindow, UiMesh, UiWindow};
use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use super::{Numbered, bytes, layout, word};
use crate::groups::BuildError;
use crate::log;

/// `CCampPages::Create` 0x60C870 (`push ebp; mov ebp, esp; push -1`, thiscall, no arguments).
const CAMP_CREATE: (Site, [u8; 5]) = (
    eix_sites::C_CAMP_PAGES_CREATE,
    [0x55, 0x8B, 0xEC, 0x6A, 0xFF],
);
/// `CMpHeroScreen::Create` 0x613EA0 (`push -1; push 0x72BCBC`, thiscall, no arguments).
const HERO_CREATE: (Site, [u8; 7]) = (
    eix_sites::C_MP_HERO_SCREEN_CREATE,
    [0x6A, 0xFF, 0x68, 0xBC, 0xBC, 0x72, 0x00],
);
/// `CMpHeroScreen::SetMode` 0x615D30 (`push ebx; push ebp; push esi; mov esi, ecx`, thiscall, the mode, `ret 4`).
const HERO_SET_MODE: (Site, [u8; 5]) = (
    eix_sites::C_MP_HERO_SCREEN_SET_MODE,
    [0x53, 0x55, 0x56, 0x8B, 0xF1],
);
/// The hero screen's sprites with the two curtains, and the mode that lists the heroes.
const HERO_SPRITES: i32 = 21;
const HERO_LIST_MODE: i32 = 9;

/// The trampolines to the original functions.
static CAMP_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static HERO_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static SET_MODE_ORIGINAL: AtomicUsize = AtomicUsize::new(0);

/// `CHelpWindow::ShowPage` 0x659420 at 0x659994: `push 0; call AddSprite`, the first sprite of a `#rectangle` line,
/// with ESI the window and the line's left and right x in stack words 25 (ESP + 0x64) and 22 (ESP + 0x58).
const HELP_FRAME: Site = eix_sites::C_HELP_WINDOW_SHOW_PAGE__574;
const HELP_FRAME_ORIGINAL: [u8; 7] = [0x6A, 0x00, 0xE8, 0x25, 0x5A, 0xFA, 0xFF];
const HELP_LEFT_WORD: usize = 25;
const HELP_RIGHT_WORD: usize = 22;

/// `CInterface3D::Create` 0x5F4990 at 0x5F4A4F: `fmul dword ptr [0x73D09C]`, a 75-point font's height times 0.01;
/// the operand points at the font scale instead, written into the padding at 0x5F4F81 after the function.
const FONT_SCALE: Site = eix_sites::C_INTERFACE_3D_CREATE__BF;
const FONT_SCALE_AT: Site = eix_sites::C_INTERFACE_3D_CREATE__5F1;
const FONT_SCALE_DEFAULT: Site = eix_sites::RDATA_73D09C;

/// The axis swap 0x4F1330 from 0x4F135A: `mov [eax], ecx` (m[0] = 1.0), then `mov dword [eax + 0x14], -1.0` and the
/// epilogue (`mov esp, ebp; pop ebp; ret 4`) and four NOPs of padding.
const AXIS_SWAP: Site = eix_sites::AXIS_SWAP_MATRIX__2A;
const AXIS_SWAP_ORIGINAL: [u8; 19] = [
    0x89, 0x08, 0xC7, 0x40, 0x14, 0x00, 0x00, 0x80, 0xBF, 0x8B, 0xE5, 0x5D, 0xC2, 0x04, 0x00, 0x90,
    0x90, 0x90, 0x90,
];
/// The store of m[0] the hook replaces; the copy of the rest runs from here.
const AXIS_SWAP_REST: usize = 2;
const AXIS_SWAP_END: usize = 15;

/// The copy of the axis swap's rest.
static AXIS_SWAP_RESUME: AtomicUsize = AtomicUsize::new(0);

type Create = unsafe extern "thiscall" fn(*mut UiWindow);
type SetMode = unsafe extern "thiscall" fn(*mut UiWindow, i32);

/// A curtain's look: shown or not, tint 0.3.
fn look(curtain: *mut UiMesh, visible: u32) {
    // SAFETY: a sprite the window just made, on the game's thread.
    let c = unsafe { &mut *curtain };
    c.visible = visible;
    c.tint = [0.3; 3];
}

/// Two sprites over the window's margins, the left one (-margin, 0)-(0, 600), the right one (800, 0)-(800 + margin,
/// 600) with the texels mirrored, at depth 2 and scale 100. The margin's pixels are taken as layout units, as the
/// original does: wider than the margins, which the screen's edge cuts.
fn add_curtains(window: *mut UiWindow, visible: u32) {
    let margin = layout().margin;
    // Both are whole numbers far below 2^24, exact as floats; the right edge is summed as a double.
    let left_edge = -(margin as f32);
    let right_edge = (f64::from(margin) + 800.0) as f32;
    let curtains = [
        (left_edge, 0.0, [0xE3, 0x3E, 0x100, 0xB0]),
        (800.0, right_edge, [0x100, 0xB0, 0xE3, 0x3E]),
    ];
    for (left, right, texels) in curtains {
        // SAFETY: the window the game's Create just built, on its thread; a null name makes a blank sprite.
        let curtain = unsafe { UiWindow::add_sprite(window, std::ptr::null()) };
        if curtain.is_null() {
            log::error(
                "WideScreen",
                "the window made no sprite for a curtain",
                &format!("{window:p}"),
            );
            return;
        }
        // SAFETY: as above; the sprite is the window's new one.
        unsafe { UiMesh::set_rectangle(curtain, 2.0, 100.0, left, 0.0, right, 600.0, texels) };
        look(curtain, visible);
    }
}

/// The camp's Create, then the curtains, shown.
extern "thiscall" fn camp_create(this: *mut UiWindow) {
    // SAFETY: the trampoline runs the original Create, of this type.
    let original: Create = unsafe { hook::as_fn_at(CAMP_ORIGINAL.load(Ordering::SeqCst)) };
    // SAFETY: the game called Create on this window, on its thread.
    unsafe { original(this) };
    add_curtains(this, 1);
}

/// The hero screen's Create, then the curtains, hidden.
extern "thiscall" fn hero_create(this: *mut UiWindow) {
    // SAFETY: as in `camp_create`.
    let original: Create = unsafe { hook::as_fn_at(HERO_ORIGINAL.load(Ordering::SeqCst)) };
    // SAFETY: as in `camp_create`.
    unsafe { original(this) };
    add_curtains(this, 0);
}

/// The hero screen's SetMode, then, when the window has its 19 sprites and the curtains, the curtains shown in every
/// mode but the hero list.
extern "thiscall" fn hero_set_mode(this: *mut UiWindow, mode: i32) {
    // SAFETY: the trampoline runs the original SetMode, of this type.
    let original: SetMode = unsafe { hook::as_fn_at(SET_MODE_ORIGINAL.load(Ordering::SeqCst)) };
    // SAFETY: the game called SetMode on this window, on its thread.
    unsafe { original(this, mode) };
    // SAFETY: as above.
    if unsafe { (*this).sprite_count } != HERO_SPRITES {
        return;
    }
    let shown = u32::from(mode != HERO_LIST_MODE);
    for index in [19, 20] {
        // SAFETY: as above; the window has 21 sprites.
        let curtain = unsafe { UiWindow::sprite(this, index) };
        if !curtain.is_null() {
            // SAFETY: a live sprite of the window.
            unsafe { (*curtain).visible = shown };
        }
    }
}

/// A tutorial message's frames on the pages that point at a moved panel move with it.
extern "C" fn help_frame(_: usize, ctx: &mut Context) {
    let window = std::ptr::with_exposed_provenance::<HelpWindow>(ctx.esi as usize);
    // SAFETY: ESI is the help window ShowPage builds, live on the game's thread.
    let w = unsafe { &*window };
    if w.message == 0 || w.text.is_null() {
        return;
    }
    // SAFETY: the text is the window's string's NUL-terminated characters.
    let text = unsafe { CStr::from_ptr(w.text) }.to_bytes();
    let page = [text, b":", w.page.to_string().as_bytes()].concat();
    let shift = tutorial_frame_shift(&page, layout().extra);
    if shift == 0 {
        return;
    }
    for word in [HELP_LEFT_WORD, HELP_RIGHT_WORD] {
        if let Some(x) = ctx.stack_word(word) {
            ctx.set_stack_word(word, x.wrapping_add_signed(shift));
        }
    }
}

/// m[0] = the projection's x scale, then the original's rest.
extern "C" fn axis_swap(scale: usize, ctx: &mut Context) {
    let m = std::ptr::with_exposed_provenance_mut::<u32>(ctx.eax as usize);
    // SAFETY: EAX is the matrix the axis swap fills, live on the game's thread.
    unsafe { m.write(scale as u32) };
    ctx.resume = Site::of(AXIS_SWAP_RESUME.load(Ordering::SeqCst));
}

pub(super) fn build(l: &Layout) -> Result<Vec<Numbered>, BuildError> {
    let (camp, camp_original) = hook::replace_function(
        CAMP_CREATE.0,
        &CAMP_CREATE.1,
        camp_create as *const () as usize,
    )?;
    CAMP_ORIGINAL.store(camp_original, Ordering::SeqCst);
    let (hero, hero_original) = hook::replace_function(
        HERO_CREATE.0,
        &HERO_CREATE.1,
        hero_create as *const () as usize,
    )?;
    HERO_ORIGINAL.store(hero_original, Ordering::SeqCst);
    let (set_mode, set_mode_original) = hook::replace_function(
        HERO_SET_MODE.0,
        &HERO_SET_MODE.1,
        hero_set_mode as *const () as usize,
    )?;
    SET_MODE_ORIGINAL.store(set_mode_original, Ordering::SeqCst);

    let (check, resume) = hook::trampoline(
        AXIS_SWAP.offset(AXIS_SWAP_REST),
        &AXIS_SWAP_ORIGINAL[AXIS_SWAP_REST..AXIS_SWAP_END],
    )?;
    AXIS_SWAP_RESUME.store(resume, Ordering::SeqCst);
    let axis = l.axis_scale().to_bits() as usize;

    let mut font = vec![0xD8, 0x0D];
    font.extend_from_slice(&FONT_SCALE_AT.ru32().to_le_bytes());
    let mut font_original = vec![0xD8, 0x0D];
    font_original.extend_from_slice(&FONT_SCALE_DEFAULT.ru32().to_le_bytes());

    Ok(vec![
        (96, camp),
        (97, hero),
        (98, set_mode),
        (
            99,
            hook::context_hook(HELP_FRAME, &HELP_FRAME_ORIGINAL, Mode::After, help_frame, 0)?,
        ),
        (89, bytes(FONT_SCALE, &font_original, &font)?),
        (
            90,
            word(FONT_SCALE_AT, [0x90; 4], l.font_scale().to_le_bytes())?,
        ),
        (
            91,
            hook::context_hook(AXIS_SWAP, &AXIS_SWAP_ORIGINAL, Mode::Drop, axis_swap, axis)?
                .padded(0xCC)
                .requiring(check),
        ),
    ])
}
