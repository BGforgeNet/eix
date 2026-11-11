//! UnitPanelSpells: the unit panel's values page (`CHudUnitPanel::DrawUnitInfo` 0x647040) lists, for a unit outside
//! the player's party, its spells below the values: each spell's icon, then its effect as the spell windows word it
//! (the label `infoitem_8`, "Effect:", and the spell's damage, truncated). With them the page can outgrow the panel:
//! then the game's own scroll bar (`CUiScrollbar`, as the quest journal has one) shows at the page's right edge, and
//! its arrows, its thumb and the mouse wheel over the page scroll it by whole lines.
//!
//! The bar is the battle screen's last child, added once the screen has made all of its own: the game reads some of
//! them by their place (`CHudTopPanel::ShowPage` takes the screen's tenth child as its message list, 0x64A3D5). The
//! screen hands input to its children in their order and the panel's own region covers the whole panel, so the panel
//! leaves the bar's clicks and the wheel over the page unhandled, and the screen passes them on to the bar. The bar is
//! enabled only while the panel shows the values page in place, so elsewhere the wheel stays the camera's.
//!
//! Hooks: the end of the battle screen's making (the bar is made), where `DrawUnitInfo` has set its first line's
//! rectangle (the lines move up by the bar's position and the page is clipped below its top), where it is done (the
//! spells are drawn, the bar learns how far the page scrolls, the clip goes), where `DrawVanilla` has drawn any page
//! (the bar goes off unless the values page was drawn), and the panel's mouse-down and wheel handlers once they found
//! the panel under the cursor (they return unhandled for the bar). A unit outside the party holds its spells only in the
//! server's world: the panel's unit (the client's) reads them from the party records, which only party units have. So
//! they come from the server's unit of the same id, which a multiplayer client does not have: there the page stays as
//! the game draws it.
#![expect(
    unsafe_code,
    reason = "making the scroll bar, reading the panel's frame, units and spells, and drawing into the panel's surface"
)]

use std::ffi::{CStr, CString, c_void};
use std::sync::Mutex;

use eix_engine::float::ftol;
use eix_engine::game::mfc::{self, GameString};
use eix_engine::game::net::is_multiplayer_client;
use eix_engine::game::spell::Spell;
use eix_engine::game::text::{self, TEXTS};
use eix_engine::game::ui::{
    self, BattleScreen, DrawTextIn, HudUnitPanel, Interface, Point, Rect, SCROLLBAR_SIZE, Surface,
    UiContainer, UiScrollbar,
};
use eix_engine::game::unit::Unit;
use eix_engine::game::world::{SERVER_WORLD, World};
use eix_engine::gdi;
use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// `CBattleScreen::Create` has made all its windows and frees its locals, the screen still in ESI:
/// `lea ecx, [esp+0x18]; mov [esp+0x5C], edi`.
const CREATE: Site = eix_sites::C_BATTLE_SCREEN_CREATE__A04;
const CREATE_BYTES: [u8; 8] = [0x8D, 0x4C, 0x24, 0x18, 0x89, 0x7C, 0x24, 0x5C];
/// `CHudUnitPanel_OnMouseDownVerify` found a region of the panel under the cursor and marks the click handled next:
/// `lea eax, [esi-0x94]`; its unhandled way out, `pop esi; ret 0xC`, is [`MOUSE_DOWN_UNHANDLED`].
const MOUSE_DOWN: Site = eix_sites::C_HUD_UNIT_PANEL_ON_MOUSE_DOWN_VERIFY__1C;
const MOUSE_DOWN_BYTES: [u8; 6] = [0x8D, 0x86, 0x6C, 0xFF, 0xFF, 0xFF];
const MOUSE_DOWN_UNHANDLED: Site = eix_sites::C_HUD_UNIT_PANEL_ON_MOUSE_DOWN_VERIFY__32;
/// `CHudUnitPanel_OnWheelVerify` has the panel's region under the cursor in EAX (negative: none): `test eax, eax;
/// mov eax, 1`; its unhandled way out, `pop esi; ret 0x10`, is [`WHEEL_UNHANDLED`].
const WHEEL: Site = eix_sites::C_HUD_UNIT_PANEL_ON_WHEEL_VERIFY__18;
const WHEEL_BYTES: [u8; 7] = [0x85, 0xC0, 0xB8, 0x01, 0x00, 0x00, 0x00];
const WHEEL_UNHANDLED: Site = eix_sites::C_HUD_UNIT_PANEL_ON_WHEEL_VERIFY__24;
/// The handlers' input part is 0x94 into the panel (`lea ecx, [esi-0x94]`, 0x64833D).
const INPUT_PART: u32 = 0x94;
/// `DrawUnitInfo` has stored its first line's rectangle: `mov [ebp-0x30], 0x9B; mov [ebp-0x2C], 0x32`.
const START: Site = eix_sites::C_HUD_UNIT_PANEL_DRAW_UNIT_INFO__6B;
const START_BYTES: [u8; 14] = [
    0xC7, 0x45, 0xD0, 0x9B, 0x00, 0x00, 0x00, 0xC7, 0x45, 0xD4, 0x32, 0x00, 0x00, 0x00,
];
/// `DrawUnitInfo`'s last line is drawn; every path meets here before the frame's strings go: `lea ecx, [ebp-0x4C];
/// mov byte [ebp-4], 1`.
const END: Site = eix_sites::C_HUD_UNIT_PANEL_DRAW_UNIT_INFO__74C;
const END_BYTES: [u8; 7] = [0x8D, 0x4D, 0xB4, 0xC6, 0x45, 0xFC, 0x01];
/// `CHudUnitPanel::DrawVanilla` has drawn its page, or found no unit to draw, with the panel in EBX:
/// `mov edx, [ebx+0x118]; mov ecx, [edx]`.
const FRAME: Site = eix_sites::C_HUD_UNIT_PANEL_DRAW_VANILLA__870;
const FRAME_BYTES: [u8; 8] = [0x8B, 0x93, 0x18, 0x01, 0x00, 0x00, 0x8B, 0x0A];

/// `DrawUnitInfo`'s line rectangle in its frame: left, top, right, bottom at EBP-0x38 .. EBP-0x2C.
const RECT_LOCAL: u32 = 0x38;
/// Its flag that the unit is outside the player's party, the byte at EBP-0xD (set at 0x647119, cleared at 0x647147).
const OUTSIDE_PARTY_LOCAL: u32 = 0xD;
/// Its unit, the first argument (EBP+8), which the page later reuses for other values (0x6475E8).
const UNIT_ARGUMENT: u32 = 8;

/// The step from one line to the next: `DrawUnitInfo` offsets its rectangle by 15 after each (0x647105).
const LINE: i32 = 15;
/// The page in the panel's layout units: `CHudUnitPanel::Setup` puts the panel's surface at x 20, 220 high
/// (0x645D6D..0x645D7C), and the first line's top is 35 (`DrawUnitInfo` 0x6470A4).
const PAGE_LEFT: i32 = 20;
const PAGE_TOP: i32 = 35;
const PAGE_BOTTOM: i32 = 220;
/// The bar's right and bottom edges: 5 units inside the inner edge of the panel's frame (x 182, y 220 on screen), the
/// gap the quest journal's bar keeps from its own frame.
const BAR_MARGIN: i32 = 5;
const BAR_RIGHT: i32 = 182 - BAR_MARGIN;
const BAR_BOTTOM: i32 = PAGE_BOTTOM - BAR_MARGIN;
/// The bar's own strip at the right of its rectangle (arrows and thumb): `CUiScrollbar::SetRect` 0x65B490 places
/// them in the last 10 units (0x65B4C7); the rest is the area its wheel scrolls (0x65B6C2).
const BAR_WIDTH: i32 = 10;
/// The bar's sprite depth and its arrows' speed, as the quest journal makes its bar (0x64BFAF..0x64BFB6).
const BAR_DEPTH: f32 = 3.0;
const BAR_SPEED: f32 = 1.0;
/// The values page: [`HudUnitPanel::page`].
const VALUES_PAGE: i32 = 2;
/// A spell's icon: its side, and the gap before its text.
const ICON: i32 = 14;
const ICON_GAP: i32 = 3;
/// The font, colour, format and flag of `DrawUnitInfo`'s lines (0x6470ED..0x6470F6).
const FONT: i32 = 0;
const COLOUR: u32 = 0x00A7_D7E4;
/// A unit's spell slots: `CUnit::GetSpell` 0x5319A0 answers for slots below 8 (0x5319FA).
const SPELL_SLOTS: u32 = 8;
/// The text of the effect label: `string infoitem_8`, the line the spell windows show the damage on.
const LABEL_KEY: &CStr = c"infoitem_8";

/// The scroll bar and the page it scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct State {
    /// The panel the bar belongs to, and the bar; 0 before a battle screen is made.
    panel: u32,
    bar: u32,
    /// The page top the bar is placed for.
    bar_top: i32,
    /// The bar is enabled (it draws and takes input).
    enabled: bool,
    /// The unit the page showed last.
    unit: u32,
    /// The device context's saved state while the page is clipped, else 0.
    saved: i32,
    /// The values page was drawn since the last frame.
    drawn: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    panel: 0,
    bar: 0,
    bar_top: PAGE_TOP,
    enabled: false,
    unit: 0,
    saved: 0,
    drawn: false,
});

/// The spell icons decoded so far, by icon number; `None` for one that has no picture.
static ICONS: Mutex<Vec<(u32, Option<ui::Picture>)>> = Mutex::new(Vec::new());
/// The effect label, read once.
static LABEL: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// The lines a page whose last line ends at `bottom`, scrolled by `offset`, reaches past the panel's bottom, a part
/// line counting whole, so the top line is never cut.
fn lines_below(bottom: i32, offset: i32) -> i32 {
    let over = bottom
        .saturating_add(offset)
        .saturating_sub(PAGE_BOTTOM)
        .max(0);
    over.saturating_add(LINE - 1) / LINE
}

/// The bar's rectangle for a page whose first line's top is `top`.
fn bar_rect(top: i32) -> Rect {
    Rect {
        left: PAGE_LEFT,
        top,
        right: BAR_RIGHT,
        bottom: BAR_BOTTOM,
    }
}

/// Where an event at (`x`, `y`) belongs while the bar for a page starting at `top` is on: its own strip (`Strip`),
/// the area its wheel scrolls (`Page`), or neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spot {
    Strip,
    Page,
    Outside,
}

fn spot(top: i32, x: i32, y: i32) -> Spot {
    let r = bar_rect(top);
    if y < r.top || y >= r.bottom || x < r.left || x >= r.right {
        Spot::Outside
    } else if x >= r.right - BAR_WIDTH {
        Spot::Strip
    } else {
        Spot::Page
    }
}

/// A decoded icon's pixels for drawing over the panel: those under half covered drop to 0, which the surface's
/// clear also writes, and the rest lose their alpha, kept off 0.
fn opaque(pixels: &[u32]) -> Vec<u32> {
    pixels
        .iter()
        .map(|&p| {
            if p >> 24 < 0x80 {
                0
            } else {
                (p & 0x00FF_FFFF).max(1)
            }
        })
        .collect()
}

/// The effect line: the label, a space, the value.
fn effect_line(label: &[u8], value: i32) -> CString {
    let mut text = label.to_vec();
    text.push(b' ');
    text.extend_from_slice(value.to_string().as_bytes());
    // A game text holds no NUL; one that did would end the line there.
    let end = text.iter().position(|&b| b == 0).unwrap_or(text.len());
    text.truncate(end);
    CString::new(text).unwrap_or_default()
}

/// The word at `address` on the hooked code's stack, 0 when it is not there.
fn local(ctx: &Context, address: u32) -> u32 {
    ctx.stack_at(address as usize).unwrap_or(0)
}

/// The bar of `state`, when it belongs to the panel at `panel`.
fn bar_of(state: &State, panel: u32) -> Option<*mut UiScrollbar> {
    (state.bar != 0 && state.panel == panel)
        .then(|| core::ptr::with_exposed_provenance_mut::<UiScrollbar>(state.bar as usize))
}

/// Turns the bar on or off when that changes.
///
/// # Safety
/// `bar` must be the live bar of `state`, on the game's thread.
unsafe fn enable(state: &mut State, bar: *mut UiScrollbar, on: bool) {
    if state.enabled != on {
        // SAFETY: the caller guarantees the bar, a window of the battle screen.
        unsafe { ui::set_window_enabled(bar.cast(), on) };
        state.enabled = on;
    }
}

/// At [`CREATE`]: makes the bar, the last child of the battle screen in ESI, off.
extern "C" fn create(_: usize, ctx: &mut Context) {
    let Ok(mut state) = STATE.lock() else {
        return;
    };
    let screen = core::ptr::with_exposed_provenance_mut::<BattleScreen>(ctx.esi as usize);
    // SAFETY: ESI is the battle screen being made, whose panel is set up.
    let panel = unsafe { (*screen).unit_panel };
    if panel.is_null() {
        return;
    }
    // SAFETY: the live panel; its interface is the screen's.
    let interface = unsafe { (*panel).window.interface };
    // SAFETY: the game's heap, as the journal allocates its bar.
    let block = unsafe { mfc::new(SCROLLBAR_SIZE) };
    if block.is_null() {
        return;
    }
    // SAFETY: a fresh block of the bar's size and the live interface.
    let bar = unsafe {
        UiScrollbar::construct(
            block,
            interface,
            bar_rect(PAGE_TOP),
            BAR_DEPTH,
            0,
            BAR_SPEED,
        )
    };
    // The constructor leaves the four sprites shown with a maximum of 0; only setting a maximum hides them
    // (`CUiScrollbar::SetMaximum` 0x65B006), and `end` sets it only when it changes.
    // SAFETY: the bar just made.
    unsafe { UiScrollbar::set_maximum(bar, 0) };
    // SAFETY: the battle screen is a container; it owns the bar from here on.
    unsafe { UiContainer::add_child(screen.cast(), bar.cast()) };
    *state = State {
        panel: panel.expose_provenance() as u32,
        bar: bar.expose_provenance() as u32,
        bar_top: PAGE_TOP,
        enabled: true,
        unit: 0,
        saved: 0,
        drawn: false,
    };
    // SAFETY: the bar just made.
    unsafe { enable(&mut state, bar, false) };
}

/// At [`START`]: moves the first line up by the bar's position and clips the page below its top.
extern "C" fn start(_: usize, ctx: &mut Context) {
    let Ok(mut state) = STATE.lock() else {
        return;
    };
    let Some(bar) = bar_of(&state, ctx.esi) else {
        return;
    };
    state.saved = 0;
    let rect = ctx.ebp.wrapping_sub(RECT_LOCAL);
    let top = local(ctx, rect + 4) as i32;
    if state.bar_top != top {
        // SAFETY: the panel's live bar.
        unsafe { UiScrollbar::set_rect(bar, &bar_rect(top)) };
        state.bar_top = top;
    }
    let unit = local(ctx, ctx.ebp.wrapping_add(UNIT_ARGUMENT));
    if state.unit != unit {
        // SAFETY: as above; another unit's page starts at its top.
        unsafe { UiScrollbar::set_position(bar, 0) };
        state.unit = unit;
    }
    // SAFETY: as above.
    let offset = unsafe { (*bar).position }.saturating_mul(LINE);
    if offset <= 0 {
        return;
    }
    let bottom = local(ctx, rect + 12) as i32;
    ctx.set_stack_at((rect + 4) as usize, top.wrapping_sub(offset) as u32);
    ctx.set_stack_at((rect + 12) as usize, bottom.wrapping_sub(offset) as u32);
    let surface = core::ptr::with_exposed_provenance_mut::<Surface>(ctx.edi as usize);
    // SAFETY: EDI is the page's surface, whose device context DrawVanilla has out while it draws.
    let s = unsafe { &*surface };
    let (dc, interface) = (s.dc, s.interface);
    if dc.is_null() || interface.is_null() {
        return;
    }
    let mut clip = Rect {
        left: 0,
        top,
        right: 800,
        bottom: 600,
    };
    // SAFETY: the surface's live interface, on the game's thread.
    unsafe { Interface::scale_rect_to_screen(interface, &mut clip) };
    // SAFETY: the surface's device context, live until DrawVanilla gives it back after this page.
    state.saved = unsafe { gdi::save(dc) };
    if state.saved != 0 {
        // SAFETY: as above.
        unsafe { gdi::clip(dc, [clip.left, clip.top, clip.right, clip.bottom]) };
    }
}

/// The spells of the unit at `unit` as the server's world holds them, none on a multiplayer client.
///
/// # Safety
/// `unit` must be the live unit the panel shows, on the game's thread.
unsafe fn spells(unit: *mut Unit) -> Vec<*mut Spell> {
    // SAFETY: the game's thread.
    if unsafe { is_multiplayer_client() } {
        return Vec::new();
    }
    // SAFETY: the caller guarantees the unit; its id is the server unit's too.
    let id = unsafe { (*unit).map_object.transform.base.id };
    // SAFETY: the server's world, constructed, with a zone loaded while the panel shows a unit.
    let server = unsafe { World::find_unit(SERVER_WORLD.as_ptr(), id) };
    if server.is_null() {
        return Vec::new();
    }
    (0..SPELL_SLOTS)
        // SAFETY: the server's live unit, with its server controller.
        .map(|slot| unsafe { Unit::spell(server, slot) })
        .filter(|s| !s.is_null())
        .collect()
}

/// The effect label, read from the texts the first time.
///
/// # Safety
/// The game's thread, where the panel draws.
unsafe fn label() -> Vec<u8> {
    let Ok(mut label) = LABEL.lock() else {
        return Vec::new();
    };
    if let Some(l) = label.as_ref() {
        return l.clone();
    }
    let mut out = GameString {
        chars: core::ptr::null_mut(),
    };
    // SAFETY: a slot of ours for a string.
    unsafe { mfc::construct_empty(&raw mut out) };
    // SAFETY: the game's text manager pointer, set up before any window draws.
    let texts = unsafe { TEXTS.read() };
    // SAFETY: the text manager `DrawUnitInfo` reads its own labels through, and our string.
    unsafe { text::get_string(texts, LABEL_KEY, &raw mut out) };
    // SAFETY: the string constructed above.
    let read = unsafe { out.as_bytes() }.to_vec();
    // SAFETY: as above, not used again.
    unsafe { mfc::destroy(&raw mut out) };
    *label = Some(read.clone());
    read
}

/// Draws icon `number`, decoded the first time, into `square` (layout units) of the page's device context `dc`.
///
/// # Safety
/// `interface` must be the live interface and `dc` the page's device context, on the game's thread.
unsafe fn draw_icon(interface: *mut Interface, dc: *mut c_void, number: u32, mut square: Rect) {
    let Ok(mut icons) = ICONS.lock() else {
        return;
    };
    let at = match icons.iter().position(|(n, _)| *n == number) {
        Some(at) => at,
        None => {
            let name = CString::new(format!("spell{number:04}")).unwrap_or_default();
            // SAFETY: the caller guarantees the interface and the thread.
            let picture = unsafe { ui::decode_picture(interface, &name) }.map(|p| ui::Picture {
                pixels: opaque(&p.pixels),
                ..p
            });
            icons.push((number, picture));
            icons.len() - 1
        }
    };
    let Some(picture) = &icons[at].1 else {
        return;
    };
    // SAFETY: the live interface.
    unsafe { Interface::scale_rect_to_screen(interface, &mut square) };
    let rect = [square.left, square.top, square.right, square.bottom];
    // SAFETY: the caller guarantees the device context.
    unsafe { gdi::draw_scaled(dc, rect, picture.width, picture.height, &picture.pixels) };
}

/// Draws one spell's line in `rect`: its icon, then its effect.
///
/// # Safety
/// `surface` must be the page's surface with its device context out, `spell` a live spell, on the game's thread.
unsafe fn draw_spell(surface: *mut Surface, spell: *mut Spell, label: &[u8], rect: Rect) {
    // SAFETY: the caller guarantees the surface.
    let s = unsafe { &*surface };
    let (dc, interface) = (s.dc, s.interface);
    // SAFETY: the caller guarantees the spell; its record is its database entry.
    let record = unsafe { Spell::record(spell) };
    // SAFETY: a spell's record is null or a live entry of the spell database.
    if let Some(record) = unsafe { record.as_ref() } {
        let square = Rect {
            left: rect.left,
            top: rect.top,
            right: rect.left + ICON,
            bottom: rect.top + ICON,
        };
        // SAFETY: the surface's live interface and device context.
        unsafe { draw_icon(interface, dc, record.icon, square) };
    }
    // SAFETY: the caller guarantees the spell.
    let line = effect_line(label, ftol(f64::from(unsafe { (*spell).damage })));
    let mut text = Rect {
        left: rect.left + ICON + ICON_GAP,
        ..rect
    };
    // SAFETY: the game's `UiSurface::DrawTextIn`.
    let draw = unsafe { hook::as_fn::<DrawTextIn>(ui::DRAW_TEXT_IN) };
    // SAFETY: the page's surface inside its device context, a NUL-terminated line, the page's own line style.
    unsafe {
        draw(
            surface,
            line.as_ptr().cast(),
            &raw mut text,
            FONT,
            COLOUR,
            0,
            1,
        )
    };
}

/// At [`END`]: draws the spells of a unit outside the party, turns the bar on (while the panel stands in place) with
/// as many positions as lines lie below, then takes the clip away.
extern "C" fn end(_: usize, ctx: &mut Context) {
    let Ok(mut state) = STATE.lock() else {
        return;
    };
    let Some(bar) = bar_of(&state, ctx.esi) else {
        return;
    };
    let surface = core::ptr::with_exposed_provenance_mut::<Surface>(ctx.edi as usize);
    let at = ctx.ebp.wrapping_sub(RECT_LOCAL);
    let mut rect = Rect {
        left: local(ctx, at) as i32,
        top: local(ctx, at + 4) as i32,
        right: local(ctx, at + 8) as i32,
        bottom: local(ctx, at + 12) as i32,
    };
    // The byte at EBP-0xD is the top byte of the word at EBP-0x10.
    let outside_party = local(ctx, ctx.ebp.wrapping_sub(OUTSIDE_PARTY_LOCAL + 3)) >> 24 != 0;
    if outside_party && state.unit != 0 {
        let unit = core::ptr::with_exposed_provenance_mut::<Unit>(state.unit as usize);
        // SAFETY: the unit the page shows, live while it draws, on the game's thread.
        let spells = unsafe { spells(unit) };
        if !spells.is_empty() {
            // SAFETY: the game's thread.
            let label = unsafe { label() };
            for spell in spells {
                // SAFETY: EDI is the page's surface with its device context out; a live spell of the unit.
                unsafe { draw_spell(surface, spell, &label, rect) };
                rect.top += LINE;
                rect.bottom += LINE;
            }
        }
    }
    let panel = core::ptr::with_exposed_provenance::<HudUnitPanel>(ctx.esi as usize);
    // SAFETY: ESI is the live panel; it stands in place when it has not slid at all.
    let in_place = unsafe { (*panel).offset } == 0;
    // SAFETY: the panel's live bar.
    let position = unsafe { (*bar).position };
    let maximum = if in_place {
        lines_below(rect.top, position.saturating_mul(LINE))
    } else {
        0
    };
    // SAFETY: as above.
    if unsafe { (*bar).maximum } != maximum {
        // SAFETY: as above.
        unsafe { UiScrollbar::set_maximum(bar, maximum) };
    }
    // SAFETY: as above.
    unsafe { enable(&mut state, bar, in_place) };
    state.drawn = true;
    if state.saved != 0 {
        // SAFETY: EDI is the page's surface.
        let dc = unsafe { (*surface).dc };
        // SAFETY: its device context, which `start` saved.
        unsafe { gdi::restore(dc, state.saved) };
        state.saved = 0;
    }
}

/// At [`FRAME`]: the bar goes off when the panel in EBX drew another page, or none, this frame.
extern "C" fn frame(_: usize, ctx: &mut Context) {
    let Ok(mut state) = STATE.lock() else {
        return;
    };
    let Some(bar) = bar_of(&state, ctx.ebx) else {
        return;
    };
    let shown = core::ptr::with_exposed_provenance::<HudUnitPanel>(ctx.ebx as usize);
    // SAFETY: EBX is the live panel.
    let values = unsafe { (*shown).page } == VALUES_PAGE;
    if !(state.drawn && values) {
        // SAFETY: the panel's live bar.
        unsafe { enable(&mut state, bar, false) };
    }
    state.drawn = false;
}

/// Whether the bar of the panel at `panel`, on and with somewhere to scroll, takes an event at (`x`, `y`) in screen
/// pixels that falls on `wanted`.
fn bar_takes(panel: u32, x: i32, y: i32, wanted: Spot) -> bool {
    let Ok(state) = STATE.lock() else {
        return false;
    };
    let Some(bar) = bar_of(&state, panel) else {
        return false;
    };
    // SAFETY: the panel's live bar.
    let maximum = unsafe { (*bar).maximum };
    if !state.enabled || maximum <= 0 {
        return false;
    }
    let shown = core::ptr::with_exposed_provenance::<HudUnitPanel>(panel as usize);
    // SAFETY: the live panel whose handler runs; its interface is the screen's.
    let interface = unsafe { (*shown).window.interface };
    let mut at = Point { x, y };
    // SAFETY: the live interface; the panel's region test scales the cursor the same way.
    unsafe { Interface::scale_point_from_screen(interface, &mut at) };
    spot(state.bar_top, at.x, at.y) == wanted
}

/// At [`MOUSE_DOWN`], before its instruction: a click on the bar's strip leaves the panel unhandled, for the bar.
extern "C" fn mouse_down(_: usize, ctx: &mut Context) {
    // Above the saved ESI and the return address: the buttons, then x and y in screen pixels (`RegionAt`'s,
    // 0x648337..0x64833C).
    let (Some(x), Some(y)) = (ctx.stack_word(3), ctx.stack_word(4)) else {
        return;
    };
    if bar_takes(
        ctx.esi.wrapping_sub(INPUT_PART),
        x as i32,
        y as i32,
        Spot::Strip,
    ) {
        ctx.resume = MOUSE_DOWN_UNHANDLED;
    }
}

/// At [`WHEEL`], before its instructions: a wheel turn over the page leaves the panel unhandled, for the bar.
extern "C" fn wheel(_: usize, ctx: &mut Context) {
    // EAX is the region under the cursor, negative for none.
    if (ctx.eax as i32) < 0 {
        return;
    }
    // Above the saved ESI and the return address: two words, then x and y in screen pixels (`RegionAt`'s,
    // 0x6484F0..0x6484FC).
    let (Some(x), Some(y)) = (ctx.stack_word(4), ctx.stack_word(5)) else {
        return;
    };
    if bar_takes(
        ctx.esi.wrapping_sub(INPUT_PART),
        x as i32,
        y as i32,
        Spot::Page,
    ) {
        ctx.resume = WHEEL_UNHANDLED;
    }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let hooks: [(Site, &[u8], Mode, hook::Handler); 6] = [
        (CREATE, &CREATE_BYTES, Mode::Before, create),
        (START, &START_BYTES, Mode::Before, start),
        (END, &END_BYTES, Mode::Before, end),
        (FRAME, &FRAME_BYTES, Mode::Before, frame),
        (MOUSE_DOWN, &MOUSE_DOWN_BYTES, Mode::After, mouse_down),
        (WHEEL, &WHEEL_BYTES, Mode::After, wheel),
    ];
    hooks
        .into_iter()
        .enumerate()
        .map(|(n, (site, bytes, mode, handler))| {
            Ok(Labelled {
                label: format!("UnitPanelSpells #{n}"),
                write: hook::context_hook(site, bytes, mode, handler, 0)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_scrolls_only_past_the_panels_bottom() {
        assert_eq!(lines_below(PAGE_BOTTOM, 0), 0);
        assert_eq!(lines_below(PAGE_BOTTOM - 30, 0), 0);
        assert_eq!(lines_below(PAGE_BOTTOM + 45, 0), 3);
        // A scrolled page ends higher by its offset.
        assert_eq!(lines_below(PAGE_BOTTOM + 15, 30), 3);
        // A part line below counts whole.
        assert_eq!(lines_below(PAGE_BOTTOM + 1, 0), 1);
        assert_eq!(lines_below(PAGE_BOTTOM + 16, 0), 2);
    }

    #[test]
    fn the_bar_takes_its_strip_and_the_page_below_its_top() {
        let strip = BAR_RIGHT - BAR_WIDTH;
        assert_eq!(spot(PAGE_TOP, strip, PAGE_TOP), Spot::Strip);
        assert_eq!(spot(PAGE_TOP, BAR_RIGHT - 1, BAR_BOTTOM - 1), Spot::Strip);
        assert_eq!(spot(PAGE_TOP, strip - 1, PAGE_TOP), Spot::Page);
        assert_eq!(spot(PAGE_TOP, PAGE_LEFT, BAR_BOTTOM - 1), Spot::Page);
        // Above the first line (the unit's name), left of the page (the page buttons), in the margins right of the
        // bar and below it.
        assert_eq!(spot(PAGE_TOP, 100, PAGE_TOP - 1), Spot::Outside);
        assert_eq!(spot(PAGE_TOP, PAGE_LEFT - 1, 100), Spot::Outside);
        assert_eq!(spot(PAGE_TOP, BAR_RIGHT, 100), Spot::Outside);
        assert_eq!(spot(PAGE_TOP, 100, BAR_BOTTOM), Spot::Outside);
        // A page that starts higher takes more.
        assert_eq!(spot(21, 100, 21), Spot::Page);
    }

    #[test]
    fn icon_pixels_lose_their_alpha_and_keep_off_the_clear_colour() {
        assert_eq!(
            opaque(&[0x7F12_3456, 0x8012_3456, 0xFF00_0000, 0x0000_0000]),
            [0, 0x0012_3456, 1, 0]
        );
    }

    #[test]
    fn the_effect_line_is_the_label_and_the_value() {
        assert_eq!(effect_line(b"Effect:", 20).as_bytes(), b"Effect: 20");
        assert_eq!(effect_line(b"Effect:", -3).as_bytes(), b"Effect: -3");
        assert_eq!(effect_line(b"", 7).as_bytes(), b" 7");
        assert_eq!(effect_line(b"Ef\0x", 7).as_bytes(), b"Ef");
    }

    #[test]
    fn the_hooks_sit_at_their_sites() {
        let w = build(&Setup::default()).unwrap();
        let sites: Vec<_> = w
            .iter()
            .map(|l| (l.write.site(), l.write.expected()))
            .collect();
        assert_eq!(
            sites,
            [
                (CREATE, &CREATE_BYTES[..]),
                (START, &START_BYTES[..]),
                (END, &END_BYTES[..]),
                (FRAME, &FRAME_BYTES[..]),
                (MOUSE_DOWN, &MOUSE_DOWN_BYTES[..]),
                (WHEEL, &WHEEL_BYTES[..])
            ]
        );
    }
}
