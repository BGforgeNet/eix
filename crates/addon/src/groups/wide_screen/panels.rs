//! The battle screen's panels: each builder sets the panel offset on entry (`-extra` for the panels at the left
//! edge, `extra` at the right) and clears it at every return, and its layout constants move by `extra`. A builder
//! called inside another leaves 0 behind, as the original's does.
#![expect(
    unsafe_code,
    reason = "writing the panels' fields the original's handlers set"
)]

use eix_core::widescreen::Layout;
use eix_engine::game::ui::{HudUnitPanel, Rect, UiWindow};
use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use super::{Numbered, bytes, layout, set_panel_offset, word};
use crate::groups::BuildError;

/// Where a builder's panel sits.
#[derive(Clone, Copy)]
enum Edge {
    Left,
    Right,
}

/// A builder's entry: its first whole instructions (the jump covers 5 bytes of them).
const ENTRIES: [(usize, Site, Edge, &[u8]); 10] = [
    // CHudMap::Setup, Layout.
    (
        21,
        eix_sites::C_HUD_MAP_SETUP_VANILLA,
        Edge::Right,
        &[0x55, 0x8B, 0xEC, 0x6A, 0xFF],
    ),
    (
        23,
        eix_sites::C_HUD_MAP_LAYOUT,
        Edge::Right,
        &[0x56, 0x8B, 0xF1, 0x68, 0x00, 0x00, 0x90, 0x41],
    ),
    // CHudUnitPanel::Layout, Draw.
    (
        37,
        eix_sites::C_HUD_UNIT_PANEL_LAYOUT,
        Edge::Left,
        &[0x51, 0x53, 0x56, 0x8B, 0xF1],
    ),
    (
        39,
        eix_sites::C_HUD_UNIT_PANEL_DRAW_VANILLA,
        Edge::Left,
        &[0x81, 0xEC, 0x00, 0x01, 0x00, 0x00],
    ),
    // CHudQuickItems::Construct.
    (
        47,
        eix_sites::C_HUD_QUICK_ITEMS_CONSTRUCT,
        Edge::Right,
        &[0x6A, 0xFF, 0x68, 0x2C, 0xDF, 0x72, 0x00],
    ),
    // CHudSpellColumn::AddSpell.
    (
        58,
        eix_sites::C_HUD_SPELL_COLUMN_ADD_SPELL,
        Edge::Right,
        &[0x83, 0xEC, 0x20, 0x53, 0x56],
    ),
    // CHudWeaponBar::Highlight.
    (
        65,
        eix_sites::C_HUD_WEAPON_BAR_HIGHLIGHT,
        Edge::Left,
        &[0x53, 0x56, 0x8B, 0x74, 0x24, 0x0C],
    ),
    // CHudMovePanel::Setup, ShowMode.
    (
        68,
        eix_sites::C_HUD_MOVE_PANEL_SETUP_VANILLA,
        Edge::Left,
        &[0x83, 0xEC, 0x1C, 0x53, 0x55],
    ),
    (
        72,
        eix_sites::C_HUD_MOVE_PANEL_SHOW_MODE_VANILLA,
        Edge::Left,
        &[0x8B, 0x44, 0x24, 0x04, 0x83, 0xEC, 0x34],
    ),
    // CHudClock::Setup.
    (
        74,
        eix_sites::C_HUD_CLOCK_SETUP,
        Edge::Right,
        &[0x83, 0xEC, 0x08, 0x53, 0x55],
    ),
];

/// A builder's returns, and the points inside one where the original clears the offset: the whole instructions
/// there, which run after the offset is cleared.
const EXITS: [(usize, Site, &[u8]); 13] = [
    // CHudMap::Setup's `ret 8`; Layout before the regions' offsets (`mov ecx, [esi + 0x12C]`).
    (
        22,
        eix_sites::C_HUD_MAP_SETUP_VANILLA__9FF,
        &[0xC2, 0x08, 0x00, 0x90, 0x90],
    ),
    (
        24,
        eix_sites::C_HUD_MAP_LAYOUT__1D9,
        &[0x8B, 0x8E, 0x2C, 0x01, 0x00, 0x00],
    ),
    // CHudUnitPanel::Setup's `ret 8`, Layout's `ret`; Draw before the window's base draw (`mov eax, [esp + 0x110]`).
    (
        36,
        eix_sites::C_HUD_UNIT_PANEL_SETUP__896,
        &[0xC2, 0x08, 0x00, 0x90, 0x90],
    ),
    (
        38,
        eix_sites::C_HUD_UNIT_PANEL_LAYOUT__319,
        &[0xC3, 0x90, 0x90, 0x90, 0x90],
    ),
    (
        40,
        eix_sites::C_HUD_UNIT_PANEL_DRAW_VANILLA__87D,
        &[0x8B, 0x84, 0x24, 0x10, 0x01, 0x00, 0x00],
    ),
    // CHudQuickItems::Construct's and CHudActionColumn::Construct's epilogues (`mov eax, esi; pop edi; ...`).
    (
        48,
        eix_sites::C_HUD_QUICK_ITEMS_CONSTRUCT__22F,
        &[0x8B, 0xC6, 0x5F, 0x5E, 0x5D],
    ),
    (
        53,
        eix_sites::C_HUD_ACTION_COLUMN_CONSTRUCT__152,
        &[0x8B, 0xC6, 0x5F, 0x5E, 0x5D],
    ),
    // CHudSpellColumn::AddSpell's (`pop edi; pop esi; pop ebx; add esp, 0x20`).
    (
        59,
        eix_sites::C_HUD_SPELL_COLUMN_ADD_SPELL__247,
        &[0x5F, 0x5E, 0x5B, 0x83, 0xC4, 0x20],
    ),
    // CHudWeaponBar::Highlight's two (`pop edi; pop esi; pop ebx; ret 8`).
    (
        66,
        eix_sites::C_HUD_WEAPON_BAR_HIGHLIGHT__93,
        &[0x5F, 0x5E, 0x5B, 0xC2, 0x08, 0x00],
    ),
    (
        67,
        eix_sites::C_HUD_WEAPON_BAR_HIGHLIGHT__11C,
        &[0x5F, 0x5E, 0x5B, 0xC2, 0x08, 0x00],
    ),
    // CHudMovePanel::Setup's last store before its return; ShowMode's and CHudClock::Setup's `ret 4`.
    (
        69,
        eix_sites::C_HUD_MOVE_PANEL_SETUP_VANILLA__3B1,
        &[0xC7, 0x80, 0xCC, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF6, 0x3E],
    ),
    (
        73,
        eix_sites::C_HUD_MOVE_PANEL_SHOW_MODE_VANILLA__175,
        &[0xC2, 0x04, 0x00, 0x90, 0x90],
    ),
    (
        75,
        eix_sites::C_HUD_CLOCK_SETUP__3B7,
        &[0xC2, 0x04, 0x00, 0x90, 0x90],
    ),
];

/// `CHudUnitPanel::Setup` 0x6456E0 from 0x645719, after the SEH frame: the slide fields from its flag argument and
/// the window's rectangle, up to `call 0x5FF830` at 0x645777.
const UNIT_PANEL_SETUP: (Site, [u8; 5]) = (
    eix_sites::C_HUD_UNIT_PANEL_SETUP__39,
    [0x89, 0x54, 0x24, 0x4C, 0x89],
);
const UNIT_PANEL_SETUP_RESUME: Site = eix_sites::C_HUD_UNIT_PANEL_SETUP__97;
/// Stack word 0x13, ESP + 0x4C: the local the slide's floats are loaded from.
const UNIT_PANEL_LOCAL: usize = 0x13;
/// The rectangle the original's handler gives the panel whatever the margin (Setup's is (0, 0)-(185, 225)), and
/// the panel's slide distance.
const UNIT_PANEL_RECT: Rect = Rect {
    left: -133,
    top: 0,
    right: 52,
    bottom: 225,
};
const UNIT_PANEL_TRAVEL: i32 = 0xB4;

/// `CHudActionColumn::Construct` 0x63F300 at 0x63F378, after the vtables: `mov [esi + 0xD8], ebx`, the window's left.
const ACTION_COLUMN: (Site, [u8; 6]) = (
    eix_sites::C_HUD_ACTION_COLUMN_CONSTRUCT__78,
    [0x89, 0x9E, 0xD8, 0x00, 0x00, 0x00],
);

/// A layout constant: an immediate (`Int`) or a float (`Float`), the original's value and the direction `extra`
/// moves it.
#[derive(Clone, Copy)]
enum Constant {
    Int(i32),
    Float(i32),
}

/// Each constant's site (its 4 bytes), what it holds, and +1 or -1: the value becomes `value + sign * extra`.
const CONSTANTS: [(usize, Site, Constant, i32); 29] = [
    // CUiTooltip::OnMouseMove: the layout's right edge (`cmp eax, 800`, `add eax, -800`).
    (
        19,
        eix_sites::C_UI_TOOLTIP_ON_MOUSE_MOVE__243,
        Constant::Int(800),
        1,
    ),
    (
        20,
        eix_sites::C_UI_TOOLTIP_ON_MOUSE_MOVE__24A,
        Constant::Int(-800),
        -1,
    ),
    // CHudMap::Setup's rectangle left and right, Layout's x, Update's two slide floats.
    (
        25,
        eix_sites::C_HUD_MAP_SETUP_VANILLA__31,
        Constant::Int(0x267),
        1,
    ),
    (
        26,
        eix_sites::C_HUD_MAP_SETUP_VANILLA__41,
        Constant::Int(800),
        1,
    ),
    (27, eix_sites::C_HUD_MAP_LAYOUT__1CC, Constant::Int(700), 1),
    (
        28,
        eix_sites::C_HUD_MAP_UPDATE_VANILLA__1E3,
        Constant::Float(713),
        1,
    ),
    (
        29,
        eix_sites::C_HUD_MAP_UPDATE_VANILLA__23F,
        Constant::Float(850),
        1,
    ),
    // The map's click: its centre x (`add eax, -700`) twice.
    (
        30,
        eix_sites::MAP_MOUSE_UP_VERIFY__DD,
        Constant::Int(-700),
        -1,
    ),
    (
        31,
        eix_sites::MAP_MOUSE_UP_VERIFY__222,
        Constant::Int(-700),
        -1,
    ),
    // CHudUnitPanel::Draw's model x, Update's two slide floats, Layout's x.
    (
        41,
        eix_sites::C_HUD_UNIT_PANEL_DRAW_VANILLA__368,
        Constant::Int(100),
        -1,
    ),
    (
        42,
        eix_sites::C_HUD_UNIT_PANEL_UPDATE_VANILLA__13B,
        Constant::Float(-50),
        -1,
    ),
    (
        43,
        eix_sites::C_HUD_UNIT_PANEL_UPDATE_VANILLA__197,
        Constant::Float(87),
        -1,
    ),
    (
        44,
        eix_sites::C_HUD_UNIT_PANEL_LAYOUT__2D1,
        Constant::Int(20),
        -1,
    ),
    // CHudQuickItems::Construct's rectangle left and right.
    (
        45,
        eix_sites::C_HUD_QUICK_ITEMS_CONSTRUCT__80,
        Constant::Int(520),
        1,
    ),
    (
        46,
        eix_sites::C_HUD_QUICK_ITEMS_CONSTRUCT__94,
        Constant::Int(720),
        1,
    ),
    // The bottom panels' right column, game.exe's float 695.0 (HudQuickItems::SetUnit reads it).
    (49, eix_sites::G_PANEL_RIGHT, Constant::Float(695), 1),
    // CHudQuickItems::AddItem's two x floats.
    (
        50,
        eix_sites::C_HUD_QUICK_ITEMS_ADD_ITEM__133,
        Constant::Float(680),
        1,
    ),
    (
        51,
        eix_sites::C_HUD_QUICK_ITEMS_ADD_ITEM__127,
        Constant::Float(720),
        1,
    ),
    // CHudActionColumn::Construct's right, AddAction's x float.
    (
        54,
        eix_sites::C_HUD_ACTION_COLUMN_CONSTRUCT__8E,
        Constant::Int(40),
        -1,
    ),
    (
        55,
        eix_sites::C_HUD_ACTION_COLUMN_ADD_ACTION__133,
        Constant::Float(20),
        -1,
    ),
    // CHudSpellColumn::Construct's rectangle left and right.
    (
        56,
        eix_sites::C_HUD_SPELL_COLUMN_CONSTRUCT__66,
        Constant::Int(0x2F8),
        1,
    ),
    (
        57,
        eix_sites::C_HUD_SPELL_COLUMN_CONSTRUCT__7A,
        Constant::Int(800),
        1,
    ),
    // CHudWeaponBar::Construct's right and its regions' first x.
    (
        60,
        eix_sites::C_HUD_WEAPON_BAR_CONSTRUCT__7B,
        Constant::Int(240),
        -1,
    ),
    (
        61,
        eix_sites::C_HUD_WEAPON_BAR_CONSTRUCT__4C,
        Constant::Int(80),
        -1,
    ),
    // CHudMovePanel::Setup's rectangle left and right.
    (
        70,
        eix_sites::C_HUD_MOVE_PANEL_SETUP_VANILLA__F,
        Constant::Int(0),
        -1,
    ),
    (
        71,
        eix_sites::C_HUD_MOVE_PANEL_SETUP_VANILLA__23,
        Constant::Int(80),
        -1,
    ),
    // CHudClock::Setup's rectangle left and right.
    (76, eix_sites::C_HUD_CLOCK_SETUP__11, Constant::Int(720), 1),
    (77, eix_sites::C_HUD_CLOCK_SETUP__25, Constant::Int(800), 1),
    // The layout's right edge, game.exe's float 800.0 (HudClock::Update's centre).
    (78, eix_sites::G_LAYOUT_WIDTH, Constant::Float(800), 1),
];

/// `CInterface3D::DrawOverlays` 0x5F5320 constructs the full-size backdrop twice with `push 1; push 600; push 800`:
/// it gets flag 0 and the mode's size. The flag's byte, and the height's and the width's immediates.
const OVERLAYS: [[(usize, Site); 3]; 2] = [
    [
        (82, eix_sites::C_INTERFACE_3D_DRAW_OVERLAYS__14A),
        (83, eix_sites::C_INTERFACE_3D_DRAW_OVERLAYS__14E),
        (84, eix_sites::C_INTERFACE_3D_DRAW_OVERLAYS__153),
    ],
    [
        (85, eix_sites::C_INTERFACE_3D_DRAW_OVERLAYS__248),
        (86, eix_sites::C_INTERFACE_3D_DRAW_OVERLAYS__24C),
        (87, eix_sites::C_INTERFACE_3D_DRAW_OVERLAYS__251),
    ],
];

/// CHudWeaponBar::AddWeapon's two `fadd` operands at 0x63BF80 (80.0 at 0x741F1C) and 0x63BF71 (120.0 at 0x740094)
/// read the floats `80 - extra` and `120 - extra`, which go into SetRect's replaced body at 0x5FC9F4.
const WEAPON_FLOATS: Site = eix_sites::C_UI_REGION_SET_RECT__14;
const WEAPON_FLOATS_ORIGINAL: [u8; 8] = [0xC4, 0x04, 0xC7, 0x46, 0x04, 0x00, 0x00, 0x00];
const WEAPON_OPERANDS: [(usize, Site, Site, Site); 2] = [
    (
        63,
        eix_sites::C_HUD_WEAPON_BAR_ADD_WEAPON__2C0,
        eix_sites::RDATA_741F1C,
        WEAPON_FLOATS,
    ),
    (
        64,
        eix_sites::C_HUD_WEAPON_BAR_ADD_WEAPON__2B1,
        eix_sites::RDATA_740094,
        WEAPON_FLOATS.offset(4),
    ),
];

extern "C" fn enter(edge: usize, _: &mut Context) {
    let extra = layout().extra;
    set_panel_offset(if edge == Edge::Left as usize {
        -extra
    } else {
        extra
    });
}

extern "C" fn leave(_: usize, _: &mut Context) {
    set_panel_offset(0);
}

/// The unit panel's slide fields, as Setup computes them from its flag, and the original handler's rectangle and
/// `sliding` 0; the panel offset `-extra`.
extern "C" fn unit_panel_setup(_: usize, ctx: &mut Context) {
    // From Setup's code before the site: EDX is the flag (0 or 1), ECX 0 or 0xB4 by it, and the carry set when
    // it is non-zero (`neg al`), which gives the slide's direction.
    let flag = ctx.edx;
    let direction = if ctx.eflags & 1 != 0 { 1 } else { -1 };
    let panel = std::ptr::with_exposed_provenance_mut::<HudUnitPanel>(ctx.esi as usize);
    // SAFETY: ESI is the unit panel Setup builds, live on the game's thread.
    let p = unsafe { &mut *panel };
    p.offset = ctx.ecx as i32;
    // The flag and the direction are 0, 1 and -1, which convert exactly.
    p.slide = flag as f32;
    p.slide_direction = direction as f32;
    p.window.rect = UNIT_PANEL_RECT;
    p.sliding = 0;
    p.travel = UNIT_PANEL_TRAVEL;
    ctx.set_stack_word(UNIT_PANEL_LOCAL, direction as u32);
    ctx.eax = direction as u32;
    ctx.ecx = ctx.esi;
    set_panel_offset(-layout().extra);
    ctx.resume = UNIT_PANEL_SETUP_RESUME;
}

/// The action column's left at `-extra`, the panel offset with it (EDI, which the constructor reloads next, too).
extern "C" fn action_column(_: usize, ctx: &mut Context) {
    let left = -layout().extra;
    let window = std::ptr::with_exposed_provenance_mut::<UiWindow>(ctx.esi as usize);
    // SAFETY: ESI is the column the constructor builds, live on the game's thread.
    unsafe { (*window).rect.left = left };
    ctx.edi = left as u32;
    set_panel_offset(left);
}

fn constant_bytes(c: Constant, delta: i32) -> [u8; 4] {
    match c {
        Constant::Int(v) => v.wrapping_add(delta).to_le_bytes(),
        // Whole numbers far below 2^24: exact as floats.
        Constant::Float(v) => (v.wrapping_add(delta) as f32).to_le_bytes(),
    }
}

pub(super) fn build(l: &Layout) -> Result<Vec<Numbered>, BuildError> {
    let mut sites = Vec::new();
    for (n, site, edge, covered) in ENTRIES {
        sites.push((
            n,
            hook::context_hook_covering(
                site,
                &covered[..5],
                covered,
                Mode::Before,
                enter,
                edge as usize,
            )?,
        ));
    }
    for (n, site, covered) in EXITS {
        sites.push((
            n,
            hook::context_hook_covering(site, &covered[..5], covered, Mode::After, leave, 0)?,
        ));
    }
    sites.push((
        35,
        hook::context_hook(
            UNIT_PANEL_SETUP.0,
            &UNIT_PANEL_SETUP.1,
            Mode::Drop,
            unit_panel_setup,
            0,
        )?,
    ));
    sites.push((
        52,
        hook::context_hook_covering(
            ACTION_COLUMN.0,
            &ACTION_COLUMN.1[..5],
            &ACTION_COLUMN.1,
            Mode::Drop,
            action_column,
            0,
        )?,
    ));
    for (n, site, c, sign) in CONSTANTS {
        let moved = constant_bytes(c, sign.wrapping_mul(l.extra));
        sites.push((n, word(site, constant_bytes(c, 0), moved)?));
    }
    for [(nf, flag), (nh, height), (nw, width)] in OVERLAYS {
        sites.push((nf, bytes(flag, &[1], &[0])?));
        sites.push((
            nh,
            word(height, 600i32.to_le_bytes(), l.height.to_le_bytes())?,
        ));
        sites.push((
            nw,
            word(width, 800i32.to_le_bytes(), l.width.to_le_bytes())?,
        ));
    }
    let mut floats = Vec::with_capacity(8);
    floats.extend_from_slice(&constant_bytes(Constant::Float(80), -l.extra));
    floats.extend_from_slice(&constant_bytes(Constant::Float(120), -l.extra));
    sites.push((62, bytes(WEAPON_FLOATS, &WEAPON_FLOATS_ORIGINAL, &floats)?));
    for (n, site, old, new) in WEAPON_OPERANDS {
        sites.push((n, SiteWrite::pointer(site, old.ru(), new.ru())));
    }
    Ok(sites)
}
