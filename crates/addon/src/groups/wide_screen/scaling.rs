//! Layout to screen and back: `CInterface3D`'s scaling entries keep the layout in the 4:3 part, the margin added
//! when ECX is non-zero (an interface pointer) and left out when it is 0, which the places that scale sizes and
//! offsets now pass; the text extent; the global map's pick picture.
#![expect(
    unsafe_code,
    reason = "scaling the game's rectangles and points, and writing the text extent it asked for"
)]

use std::sync::atomic::{AtomicUsize, Ordering};

use eix_core::widescreen::Layout;
use eix_engine::game::ui::{Point, Rect};
use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, Mode};
use eix_engine::patch;

use super::{Numbered, bytes, layout, word};
use crate::groups::BuildError;

/// `CInterface3D::ScaleRectToScreen` 0x5F7CA0, `ScalePointToScreen` 0x5F7D20 and its unused copy 0x5F7D60: each
/// starts `push esi; mov esi, [esp + 8]`.
const TO_SCREEN: [(usize, usize, bool); 3] = [
    (7, eix_sites::C_INTERFACE_3D_SCALE_RECT_TO_SCREEN, true),
    (8, eix_sites::C_INTERFACE_3D_SCALE_POINT_TO_SCREEN, false),
    (
        9,
        eix_sites::C_INTERFACE_3D_SCALE_POINT_TO_SCREEN__40,
        false,
    ),
];
const TO_SCREEN_START: [u8; 5] = [0x56, 0x8B, 0x74, 0x24, 0x08];

/// The screen-to-layout scalings, from 0x5F7D9E to 0x5F7E60: `ScaleRectFromScreen` 0x5F7DA0, `ScalePointFromScreen`
/// 0x5F7E00 and its copy 0x5F7E30, with the padding in front of each, become the three entries and, two bytes
/// before each, an entry that clears ECX (no margin).
const FROM_SCREEN: usize = eix_sites::C_INTERFACE_3D_SCALE_POINT_TO_SCREEN__7E;
const FROM_SCREEN_ORIGINAL: [u8; 194] = [
    0x90, 0x90, 0x56, 0x8B, 0x74, 0x24, 0x08, 0x8B, 0x06, 0x8D, 0x04, 0x80, 0x8D, 0x04, 0x80, 0xC1,
    0xE0, 0x05, 0x99, 0xF7, 0x39, 0x89, 0x06, 0x8B, 0x46, 0x04, 0x8D, 0x04, 0x40, 0x8D, 0x04, 0x80,
    0x8D, 0x04, 0x80, 0xC1, 0xE0, 0x03, 0x99, 0xF7, 0x79, 0x04, 0x89, 0x46, 0x04, 0x8B, 0x46, 0x08,
    0x8D, 0x04, 0x80, 0x8D, 0x04, 0x80, 0xC1, 0xE0, 0x05, 0x99, 0xF7, 0x39, 0x89, 0x46, 0x08, 0x8B,
    0x46, 0x0C, 0x8D, 0x04, 0x40, 0x8D, 0x04, 0x80, 0x8D, 0x04, 0x80, 0xC1, 0xE0, 0x03, 0x99, 0xF7,
    0x79, 0x04, 0x89, 0x46, 0x0C, 0x5E, 0xC2, 0x04, 0x00, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90, 0x90, 0x56, 0x8B, 0x74, 0x24, 0x08, 0x8B, 0x06, 0x8D, 0x04, 0x80, 0x8D, 0x04, 0x80, 0xC1,
    0xE0, 0x05, 0x99, 0xF7, 0x39, 0x89, 0x06, 0x8B, 0x46, 0x04, 0x8D, 0x04, 0x40, 0x8D, 0x04, 0x80,
    0x8D, 0x04, 0x80, 0xC1, 0xE0, 0x03, 0x99, 0xF7, 0x79, 0x04, 0x89, 0x46, 0x04, 0x5E, 0xC2, 0x04,
    0x00, 0x90, 0x56, 0x8B, 0x74, 0x24, 0x08, 0x8B, 0x06, 0x8D, 0x04, 0x80, 0x8D, 0x04, 0x80, 0xC1,
    0xE0, 0x05, 0x99, 0xF7, 0x39, 0x89, 0x06, 0x8B, 0x46, 0x04, 0x8D, 0x04, 0x40, 0x8D, 0x04, 0x80,
    0x8D, 0x04, 0x80, 0xC1, 0xE0, 0x03, 0x99, 0xF7, 0x79, 0x04, 0x89, 0x46, 0x04, 0x5E, 0xC2, 0x04,
    0x00, 0x90,
];
/// The point entry's offset in that range; the rect entry's is 2, the copy's 0x92.
const POINT_FROM_SCREEN: usize = eix_sites::SCALE_POINT_FROM_SCREEN_ENTRY;
/// `xor ecx, ecx`: the entry in front of each.
const NO_MARGIN: [u8; 2] = [0x31, 0xC9];

/// `mov ecx, [reg + 0x98]` (the window's interface) before a call that scales a size or an offset: the edit field
/// (#10, #11), the connection list (#12, #13), the save / load list (#14, #15), the chat line (#16, #17), the
/// tooltip (#18) with ESI, the unit panel's depth clears (#32 .. #34) with EBX. Four NOPs and `xor ecx, ecx` pass 0.
const NO_INTERFACE: [(usize, usize, u8); 12] = [
    (
        10,
        eix_sites::C_UI_EDIT_FIELD_DRAW_CONTENTS_VANILLA__23F,
        0x8E,
    ),
    (
        11,
        eix_sites::C_UI_EDIT_FIELD_DRAW_CONTENTS_VANILLA__264,
        0x8E,
    ),
    (
        12,
        eix_sites::C_MP_CONNECT_SCREEN_DRAW_LIST_VANILLA__38E,
        0x8E,
    ),
    (
        13,
        eix_sites::C_MP_CONNECT_SCREEN_DRAW_LIST_VANILLA__3BE,
        0x8E,
    ),
    (
        14,
        eix_sites::C_SAVE_LOAD_WINDOW_REFRESH_LIST_VANILLA__257,
        0x8E,
    ),
    (
        15,
        eix_sites::C_SAVE_LOAD_WINDOW_REFRESH_LIST_VANILLA__268,
        0x8E,
    ),
    (16, eix_sites::C_CHAT_OVERLAY_DRAW_TYPED__139, 0x8E),
    (17, eix_sites::C_CHAT_OVERLAY_DRAW_TYPED__16C, 0x8E),
    (18, eix_sites::C_UI_TOOLTIP_ON_MOUSE_MOVE__1FE, 0x8E),
    (32, eix_sites::C_HUD_UNIT_PANEL_DRAW_VANILLA__1AC, 0x8B),
    (33, eix_sites::C_HUD_UNIT_PANEL_DRAW_VANILLA__1EE, 0x8B),
    (34, eix_sites::C_HUD_UNIT_PANEL_DRAW_VANILLA__1F9, 0x8B),
];
const NO_INTERFACE_NEW: [u8; 6] = [0x90, 0x90, 0x90, 0x90, 0x31, 0xC9];

/// `mov ecx, [edi + 4]` before `CUiSurface::DrawTextIn` 0x5FDE20's call of ScaleRectToScreen (#92).
const DRAW_TEXT: (usize, [u8; 3]) = (eix_sites::C_UI_SURFACE_DRAW_TEXT_IN__74, [0x8B, 0x4F, 0x04]);

/// The camera's and the global map's mouse moves call ScalePointFromScreen with a drag offset (#94, #95): they go
/// to the entry without the margin.
const DRAGS: [(usize, usize); 2] = [
    (94, eix_sites::C_CAMERA_INPUT_ON_MOUSE_MOVE__39),
    (95, eix_sites::GLOBAL_MAP_MOUSE_MOVE__7C),
];

/// `CInterface3D::TextExtent` 0x5F8340 from 0x5F83CF: the drawn text's extent (EBX, EBP) divided by the screen's
/// size into the SIZE at ESP + 0x30, then `call 0x700970` at 0x5F8403 (the string's destructor) and the epilogue.
const TEXT_EXTENT: usize = eix_sites::C_INTERFACE_3D_TEXT_EXTENT__8F;
const TEXT_EXTENT_ORIGINAL: [u8; 80] = [
    0x8D, 0x04, 0x9B, 0x8B, 0x74, 0x24, 0x30, 0xC7, 0x44, 0x24, 0x28, 0xFF, 0xFF, 0xFF, 0xFF, 0x8D,
    0x04, 0x80, 0xC1, 0xE0, 0x05, 0x99, 0xF7, 0x3F, 0x8B, 0xC8, 0x8D, 0x44, 0x6D, 0x00, 0x8D, 0x04,
    0x80, 0x8D, 0x04, 0x80, 0xC1, 0xE0, 0x03, 0x99, 0xF7, 0x7F, 0x04, 0x89, 0x0E, 0x8D, 0x4C, 0x24,
    0x34, 0x89, 0x46, 0x04, 0xE8, 0x68, 0x85, 0x10, 0x00, 0x8B, 0x4C, 0x24, 0x20, 0x8B, 0xC6, 0x5F,
    0x5E, 0x5D, 0x5B, 0x64, 0x89, 0x0D, 0x00, 0x00, 0x00, 0x00, 0x83, 0xC4, 0x1C, 0xC2, 0x0C, 0x00,
];
/// Where the destructor's call starts in it; the hook runs it and the epilogue from a copy.
const TEXT_EXTENT_TAIL: usize = 52;
/// Stack words at the site: the SIZE's address (ESP + 0x30), the SEH state (ESP + 0x28); ECX for the destructor is
/// ESP + 0x34.
const TEXT_EXTENT_OUT: usize = 12;
const TEXT_EXTENT_STATE: usize = 10;
const TEXT_EXTENT_STRING: usize = 0x34;

/// The global map's pick picture `CUiSurface::Pixel` 0x5FD240 is read at (x / 2, y / 2); its callers in the map's
/// mouse move (#80, the call's operand at 0x63023C) and click (#81, 0x62FB20) push x / 2 last, with it in EAX. They
/// call 9 bytes early instead (#79, the padding in front of it), where half the margin comes off.
const PICK_ENTRY: usize = eix_sites::C_UI_SURFACE_RELEASE_CONTENTS__117;
const PICK: usize = eix_sites::C_UI_SURFACE_PIXEL;
const PICK_CALLERS: [(usize, usize); 2] = [
    (80, eix_sites::GLOBAL_MAP_MOUSE_MOVE__22C),
    (81, eix_sites::GLOBAL_MAP_LEFT_DOWN__1D0),
];

/// The copy of TextExtent's tail.
static TEXT_EXTENT_RESUME: AtomicUsize = AtomicUsize::new(0);

fn scale_rect(margin: bool, rect: *mut Rect, to_screen: bool) {
    let l = layout();
    // SAFETY: the game passes the rectangle it scales, live on its thread.
    let r = unsafe { &mut *rect };
    if to_screen {
        r.left = l.x_to_screen(r.left, margin);
        r.top = l.y_to_screen(r.top);
        r.right = l.x_to_screen(r.right, margin);
        r.bottom = l.y_to_screen(r.bottom);
    } else {
        r.left = l.x_from_screen(r.left, margin);
        r.right = l.x_from_screen(r.right, margin);
        r.top = l.y_from_screen(r.top);
        r.bottom = l.y_from_screen(r.bottom);
    }
}

fn scale_point(margin: bool, point: *mut Point, to_screen: bool) {
    let l = layout();
    // SAFETY: the game passes the point it scales, live on its thread.
    let p = unsafe { &mut *point };
    if to_screen {
        p.x = l.x_to_screen(p.x, margin);
        p.y = l.y_to_screen(p.y);
    } else {
        p.x = l.x_from_screen(p.x, margin);
        p.y = l.y_from_screen(p.y);
    }
}

// The entries are thiscall in the game, the rectangle or point the one stack argument (`ret 4`); fastcall takes ECX
// as the first argument and EDX, which thiscall leaves undefined, as the second.
extern "fastcall" fn rect_to_screen(ecx: usize, _edx: usize, rect: *mut Rect) {
    scale_rect(ecx != 0, rect, true);
}

extern "fastcall" fn point_to_screen(ecx: usize, _edx: usize, point: *mut Point) {
    scale_point(ecx != 0, point, true);
}

extern "fastcall" fn rect_from_screen(ecx: usize, _edx: usize, rect: *mut Rect) {
    scale_rect(ecx != 0, rect, false);
}

extern "fastcall" fn point_from_screen(ecx: usize, _edx: usize, point: *mut Point) {
    scale_point(ecx != 0, point, false);
}

/// The extent in layout units, by the 4:3 part's width and the mode's height, then the original's tail.
extern "C" fn text_extent(_: usize, ctx: &mut Context) {
    let (cx, cy) = layout().text_extent(ctx.ebx as i32, ctx.ebp as i32);
    let out = ctx.stack_word(TEXT_EXTENT_OUT).unwrap_or(0);
    if out != 0 {
        let size = std::ptr::with_exposed_provenance_mut::<[i32; 2]>(out as usize);
        // SAFETY: the caller's SIZE, which TextExtent fills, on the game's thread.
        unsafe { size.write([cx, cy]) };
    }
    ctx.set_stack_word(TEXT_EXTENT_STATE, u32::MAX);
    ctx.esi = out;
    ctx.eax = cy as u32;
    ctx.ecx = (ctx.stack() + TEXT_EXTENT_STRING) as u32;
    ctx.resume = TEXT_EXTENT_RESUME.load(Ordering::SeqCst);
}

/// Half the margin off the picture's x (EAX and the argument just pushed).
extern "C" fn pick(shift: usize, ctx: &mut Context) {
    let x = ctx.eax.wrapping_sub(shift as u32);
    ctx.eax = x;
    ctx.set_stack_word(1, x);
}

/// The 194 bytes from 0x5F7D9E: an ECX-clearing entry, then a jump to each from-screen scaling, `int3` between.
fn from_screen_code() -> Vec<u8> {
    let mut code = vec![0xCC; FROM_SCREEN_ORIGINAL.len()];
    let entries = [
        (2, rect_from_screen as *const () as usize),
        (
            POINT_FROM_SCREEN - FROM_SCREEN,
            point_from_screen as *const () as usize,
        ),
        (0x92, point_from_screen as *const () as usize),
    ];
    for (at, target) in entries {
        code[at - 2..at].copy_from_slice(&NO_MARGIN);
        code[at..at + 5].copy_from_slice(&patch::jmp_rel32(FROM_SCREEN + at, target));
    }
    code
}

pub(super) fn build(l: &Layout) -> Result<Vec<Numbered>, BuildError> {
    let mut sites = Vec::new();
    for (n, site, rect) in TO_SCREEN {
        let to = if rect {
            rect_to_screen as *const () as usize
        } else {
            point_to_screen as *const () as usize
        };
        sites.push((n, SiteWrite::jmp(site, &TO_SCREEN_START, to)?));
    }
    sites.push((
        93,
        bytes(FROM_SCREEN, &FROM_SCREEN_ORIGINAL, &from_screen_code())?,
    ));
    for (n, site, base) in NO_INTERFACE {
        sites.push((
            n,
            bytes(
                site,
                &[0x8B, base, 0x98, 0x00, 0x00, 0x00],
                &NO_INTERFACE_NEW,
            )?,
        ));
    }
    sites.push((92, bytes(DRAW_TEXT.0, &DRAW_TEXT.1, &[0x31, 0xC9, 0x90])?));
    for (n, site) in DRAGS {
        sites.push((
            n,
            bytes(
                site,
                &patch::call_rel32(site, POINT_FROM_SCREEN),
                &patch::call_rel32(site, POINT_FROM_SCREEN - NO_MARGIN.len()),
            )?,
        ));
    }

    let tail = TEXT_EXTENT + TEXT_EXTENT_TAIL;
    TEXT_EXTENT_RESUME.store(
        hook::trampoline(tail, &TEXT_EXTENT_ORIGINAL[TEXT_EXTENT_TAIL..])?,
        Ordering::SeqCst,
    );
    sites.push((
        88,
        hook::context_hook(
            TEXT_EXTENT,
            &TEXT_EXTENT_ORIGINAL,
            Mode::Drop,
            text_extent,
            0,
        )?
        .padded(0xCC),
    ));

    let shift = usize::try_from(l.pick_shift()).map_err(|e| BuildError::from(e.to_string()))?;
    sites.push((
        79,
        hook::context_hook(PICK_ENTRY, &[0x90; 9], Mode::Drop, pick, shift)?.padded(0xCC),
    ));
    for (n, operand) in PICK_CALLERS {
        let end = operand + 4;
        let rel = |to: usize| (to as u32).wrapping_sub(end as u32).to_le_bytes();
        sites.push((n, word(operand, rel(PICK), rel(PICK_ENTRY))?));
    }
    Ok(sites)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A jump's target.
    fn target(code: &[u8], at: usize) -> usize {
        let mut rel = [0u8; 4];
        rel.copy_from_slice(&code[at + 1..at + 5]);
        (FROM_SCREEN as u32 + at as u32 + 5).wrapping_add(u32::from_le_bytes(rel)) as usize
    }

    #[test]
    fn the_from_screen_entries_sit_where_the_callers_enter() {
        let code = from_screen_code();
        assert_eq!(code.len(), 194);
        // 0x5F7DA0, 0x5F7E00 (the camera's and the map's drags enter at 0x5F7DFE) and 0x5F7E30.
        for (at, to) in [
            (2, rect_from_screen as *const () as usize),
            (0x62, point_from_screen as *const () as usize),
            (0x92, point_from_screen as *const () as usize),
        ] {
            assert_eq!(code[at - 2..at], NO_MARGIN, "{at:#x}");
            assert_eq!(code[at], 0xE9, "{at:#x}");
            assert_eq!(target(&code, at), to, "{at:#x}");
        }
        assert_eq!(code[193], 0xCC);
    }

    #[test]
    fn the_pick_callers_are_redirected_nine_bytes_early() {
        // The operands: call 0x5FD240 becomes call 0x5FD237.
        let l = crate::groups::wide_screen::tests::layout_1056x768();
        let sites = build(&l).unwrap_or_default();
        let find = |n: usize| sites.iter().find(|(m, _)| *m == n).map(|(_, w)| w.clone());
        let expected = [
            (
                80,
                SiteWrite::bytes(
                    eix_sites::GLOBAL_MAP_MOUSE_MOVE__22C,
                    &[0x00, 0xD0, 0xFC, 0xFF],
                    &[0xF7, 0xCF, 0xFC, 0xFF],
                ),
            ),
            (
                81,
                SiteWrite::bytes(
                    eix_sites::GLOBAL_MAP_LEFT_DOWN__1D0,
                    &[0x1C, 0xD7, 0xFC, 0xFF],
                    &[0x13, 0xD7, 0xFC, 0xFF],
                ),
            ),
        ];
        for (n, want) in expected {
            assert_eq!(find(n), want.ok(), "#{n}");
        }
    }
}
