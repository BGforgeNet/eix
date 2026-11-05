//! Regions and sprites made while a panel builder runs move along x by the panel offset: `CUiRegion::SetRect` and
//! `AddPoint`, `CUiMesh::PlaceBody`, and the shape builders, whose x87 code the FPU hooks join in the middle.
#![expect(
    unsafe_code,
    reason = "replacing game functions and reading the regions and sprites they get"
)]

use std::ffi::c_void;

use eix_engine::game::mfc;
use eix_engine::game::ui::{Rect, UiMesh, UiRegion, layout};
use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, FpuContext, Mode};
use eix_engine::x87::Ext;

use super::{Numbered, panel_offset};
use crate::groups::BuildError;
use crate::log;

/// `CUiRegion::SetRect` 0x5FC9E0 (`push esi; mov esi, ecx; mov eax, ...`): replaced whole; its body from 0x5FC9F4
/// holds the weapon bar's two floats (panels).
const SET_RECT: (usize, [u8; 5]) = (
    eix_sites::C_UI_REGION_SET_RECT,
    [0x56, 0x8B, 0xF1, 0x8B, 0x46],
);

/// `CUiRegion::AddPoint` 0x5FCA70: `sub esp, 0xC; push ebx; mov ebx, ecx; push edi; xor edi, edi`.
const ADD_POINT: usize = eix_sites::C_UI_REGION_ADD_POINT;
const ADD_POINT_PROLOGUE: [u8; 9] = [0x83, 0xEC, 0x0C, 0x53, 0x8B, 0xD9, 0x57, 0x33, 0xFF];

/// `CUiMesh::PlaceBody` 0x5E6FA0 (`sub esp, 0xC; fld dword ptr [esp + 0x10]`), replaced whole.
const PLACE_BODY: (usize, [u8; 5]) = (
    eix_sites::C_UI_MESH_PLACE_BODY,
    [0x83, 0xEC, 0x0C, 0xD9, 0x44],
);

/// `fld dword ptr [esp + 0x34]; fmul dword ptr [0x74015C]`: a vertex's x to view units, in `CUiMesh::SetTriangle`
/// (#1) and `SetGrid` (#2).
const LOAD_X: [u8; 10] = [0xD9, 0x44, 0x24, 0x34, 0xD8, 0x0D, 0x5C, 0x01, 0x74, 0x00];
const LOAD_X_SITES: [(usize, usize); 2] = [
    (1, eix_sites::C_UI_MESH_SET_TRIANGLE__4A7),
    (2, eix_sites::C_UI_MESH_SET_GRID_VANILLA__60D),
];
/// Stack word 13, ESP + 0x34: the vertex's layout x.
const LOAD_X_WORD: usize = 13;

/// `fmul dword ptr [0x740164]` on the sum of the left and right x, in `CUiMesh::SetRectangle` (#3) and `SetFrame`
/// (#4).
const SCALE_SUM: [u8; 6] = [0xD8, 0x0D, 0x64, 0x01, 0x74, 0x00];
const SCALE_SUM_SITES: [(usize, usize); 2] = [
    (3, eix_sites::C_UI_MESH_SET_RECTANGLE__4F1),
    (4, eix_sites::C_UI_MESH_SET_FRAME__5FE),
];

/// The region's rectangle with x moved by the panel offset; the vertices go.
extern "thiscall" fn set_rect(this: *mut UiRegion, left: i32, top: i32, right: i32, bottom: i32) {
    let offset = panel_offset();
    // SAFETY: the game calls SetRect on a live region, on its thread.
    let region = unsafe { &mut *this };
    if !region.points.is_null() {
        // SAFETY: the word in front of a vertex block holds the address of the allocation the block lies in.
        let header = unsafe { region.points.cast::<*mut c_void>().sub(1) };
        // SAFETY: as above.
        let block = unsafe { header.read() };
        // SAFETY: that allocation is the game's and freed here only, as the original does.
        unsafe { mfc::delete(block) };
        region.points = std::ptr::null_mut();
    }
    region.points_capacity = std::ptr::null_mut();
    region.points_end = std::ptr::null_mut();
    region.bounds = Rect {
        left: left.wrapping_add(offset),
        top,
        right: right.wrapping_add(offset),
        bottom,
    };
    region.enabled = 1;
    region.rect_only = 1;
}

/// The vertex's x (stack word 1, before the prologue) moved by the panel offset.
extern "C" fn add_point(_: usize, ctx: &mut Context) {
    if let Some(x) = ctx.stack_word(1) {
        ctx.set_stack_word(1, x.wrapping_add_signed(panel_offset()));
    }
}

/// The view's place for a sprite at layout (`x`, `y`) and `depth`, x moved by the panel offset; on the x87 under the
/// game's control word, as the original computes it.
extern "thiscall" fn place_body(this: *mut UiMesh, x: f32, y: f32, depth: f32) {
    // SAFETY: game.exe's `.rdata` constants, read on the game's thread.
    let to_view = unsafe { layout::LAYOUT_TO_VIEW.read() };
    // SAFETY: as above.
    let scale = unsafe { layout::VIEW_SCALE.read() };
    // SAFETY: as above.
    let centre_x = unsafe { layout::CENTRE_X.read() };
    // SAFETY: as above.
    let centre_y = unsafe { layout::CENTRE_Y.read() };
    // Each float and int operand widens to a double exactly, so `fmul qword` rounds as the original's `fmul dword`
    // and `fadd qword` as its `fiadd`.
    let vx = Ext::new(x.into())
        .fadd(panel_offset().into())
        .fmul(to_view.into())
        .fsub(centre_x.into())
        .fmul(scale.into())
        .fmul(depth.into())
        .to_f32();
    let vy = Ext::new(y.into())
        .fmul(to_view.into())
        .fsub(centre_y.into())
        .fmul(scale.into())
        .fmul(depth.into())
        .to_f32();
    // SAFETY: the game calls PlaceBody on a live sprite, on its thread.
    unsafe { (*this).place = [vx, vy, depth] };
}

/// In place of `fld [esp + 0x34]; fmul [LAYOUT_TO_VIEW]`: pushes `(x + offset) * LAYOUT_TO_VIEW`.
extern "C" fn load_x(_: usize, ctx: &mut FpuContext) {
    // SAFETY: a game.exe `.rdata` constant, read on the game's thread.
    let to_view = unsafe { layout::LAYOUT_TO_VIEW.read() };
    let x = ctx.regs.stack_word(LOAD_X_WORD).map(f32::from_bits);
    let pushed = x.is_some_and(|x| {
        ctx.fpu.push(
            Ext::new(x.into())
                .fadd(panel_offset().into())
                .fmul(to_view.into()),
        )
    });
    if !pushed {
        log::error(
            "WideScreen",
            "the x87 stack is full or the local is off the stack",
            &format!("{:#x}", ctx.regs.resume),
        );
    }
}

/// In place of `fmul [HALF_LAYOUT_TO_VIEW]` on ST(0) = left + right: the sum moved by twice the offset first.
extern "C" fn scale_sum(_: usize, ctx: &mut FpuContext) {
    // SAFETY: as in `load_x`.
    let half = unsafe { layout::HALF_LAYOUT_TO_VIEW.read() };
    let offset = panel_offset();
    let done = ctx.fpu.st(0).is_some_and(|sum| {
        let offset = f64::from(offset);
        ctx.fpu
            .set_st(0, sum.fadd(offset).fadd(offset).fmul(half.into()))
    });
    if !done {
        log::error(
            "WideScreen",
            "the x87 stack is empty",
            &format!("{:#x}", ctx.regs.resume),
        );
    }
}

pub(super) fn build() -> Result<Vec<Numbered>, BuildError> {
    let mut sites = vec![
        (
            0,
            SiteWrite::jmp(SET_RECT.0, &SET_RECT.1, set_rect as *const () as usize)?,
        ),
        (
            5,
            hook::context_hook_covering(
                ADD_POINT,
                &ADD_POINT_PROLOGUE[..5],
                &ADD_POINT_PROLOGUE,
                Mode::After,
                add_point,
                0,
            )?,
        ),
        (
            6,
            SiteWrite::jmp(
                PLACE_BODY.0,
                &PLACE_BODY.1,
                place_body as *const () as usize,
            )?,
        ),
    ];
    for (n, site) in LOAD_X_SITES {
        sites.push((
            n,
            hook::fpu_hook(site, &LOAD_X[..5], &LOAD_X, Mode::Drop, load_x, 0)?,
        ));
    }
    for (n, site) in SCALE_SUM_SITES {
        sites.push((
            n,
            hook::fpu_hook(site, &SCALE_SUM[..5], &SCALE_SUM, Mode::Drop, scale_sum, 0)?,
        ));
    }
    Ok(sites)
}
