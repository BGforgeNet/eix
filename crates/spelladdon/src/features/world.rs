//! World features: the info window, the global map, the main menu, sound and music,
//! zones, and the fixes (`ultrafixes`, `expirementalfxes`).
//!
//! Where the original jumps to added code, the site becomes a hook whose handler does that code's work; its own
//! bytes are not written. Data the original keeps outside that code (strings in padding) is written as it does, since
//! the game reads it there.
#![expect(
    unsafe_code,
    reason = "the handlers read the game's frames, objects and zone names, and call its variable lookup"
)]

use std::sync::atomic::{AtomicU32, Ordering};

use eix_core::x86_flags;
use eix_engine::ansi;
use eix_engine::game::player::NamedFloatTable;
use eix_engine::group::SiteWrite;
use eix_engine::hook::Context;
use eix_engine::patch;
use eix_engine::x87::Ext;
use eix_sites::Site;

use super::common::{self, Bytes, PushTail, fixed, read};
use super::{Build, BuildError, Ctx};
use crate::Level;

/// The implemented features: `(section, key, build)`.
pub(crate) const BUILDS: &[(&str, &str, Build)] = &[
    ("gameplay", "rewriteperssize", pers_size),
    ("fonts", "changefont", change_font),
    ("gameplay", "infowindowschanges", info_window_changes),
    ("gameplay", "allodsinfo", allods_info),
    ("gameplay", "showtype", show_type),
    ("gameplay", "expirementalfxes", experimental_fixes),
    ("gameplay", "noruinszoneanimation", no_ruins_zone_animation),
    ("gameplay", "showmanaforeveryone", show_mana_for_everyone),
    ("gameplay", "meterscale", meter_scale),
    ("gameplay", "lightsboost", lights_boost),
    ("gameplay", "mainmenuweather", main_menu_weather),
    ("gameplay", "notrash", no_trash),
    ("gameplay", "dontmergegipathzones", dont_merge_gipat_zones),
    ("gameplay", "musictimings", music_timings),
    ("gameplay", "exploredcolors", explored_colors),
    ("gameplay", "disabledragon", disable_dragon),
    ("gameplay", "newcounters", new_counters),
    ("gameplay", "infowindowremake", info_window_remake),
    ("gameplay", "drawvisiblepatch", draw_visible_patch),
    ("gameplay", "playingnewloadingscreens", new_loading_screens),
    ("gameplay", "newzonescpecificmusic", zone_specific_music),
    ("gameplay", "newallodsacks", new_allods_acks),
    ("gameplay", "segmentsonexitenter", segments_on_exit_enter),
    ("gameplay", "bigmapspatch", big_maps_patch),
    ("gameplay", "ultrafixes", ultra_fixes),
    ("gameplay", "hiddenzonesexits", hidden_zones_exits),
    (
        "gameplay",
        "nogameoveronleadersdeath",
        no_game_over_on_leaders_death,
    ),
];

/// The writes of `table` when `value` is 1, else none: the shape of most switches here.
fn when_on(value: i8, table: &[Bytes]) -> Result<Vec<SiteWrite>, BuildError> {
    if value == 1 {
        fixed(table)
    } else {
        Ok(Vec::new())
    }
}

/// A 32-bit immediate or constant `was` at `site` set to `now`.
fn word(site: Site, was: u32, now: [u8; 4]) -> Result<SiteWrite, BuildError> {
    Ok(SiteWrite::bytes(site, &was.to_le_bytes(), &now)?)
}

// --- Interface ---------------------------------------------------------------------------------------------------

/// The hero model's scale in `CUiWindow::DrawHeroPreview`: the operand of `mov ecx, 0x3F99999A` (1.2) at 0x5EC8F2.
const HERO_PREVIEW_SCALE: Site = eix_sites::C_UI_WINDOW_DRAW_HERO_PREVIEW__5F3;

/// `rewriteperssize` = 1: the hero preview's scale becomes `perssize`.
fn pers_size(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let size = ctx.values.float("gameplay", "perssize")?;
    Ok(vec![SiteWrite::bytes(
        HERO_PREVIEW_SCALE,
        &1.2f32.to_le_bytes(),
        &size.to_le_bytes(),
    )?])
}

/// `infowindowschanges` = 1: `DrawUnitInfo`'s layout constant `[ebp-0x34]` becomes 0x15.
fn info_window_changes(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    when_on(
        value,
        &[(
            eix_sites::C_HUD_UNIT_PANEL_DRAW_UNIT_INFO__67,
            &[0x23],
            &[0x15],
        )],
    )
}

/// `showtype` = 1: the movement-type line of the info window is no longer skipped.
fn show_type(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    when_on(
        value,
        &[(
            eix_sites::C_HUD_UNIT_PANEL_DRAW_UNIT_INFO__175,
            &[0x76, 0x2A],
            &[0x90, 0x90],
        )],
    )
}

/// `showmanaforeveryone` = 1: `CHudUnitPanel::ShowHealth` jumps over its hero test to the mana bar.
fn show_mana_for_everyone(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    when_on(
        value,
        &[(
            eix_sites::C_HUD_UNIT_PANEL_SHOW_HEALTH__1C3,
            &[0x39, 0x39],
            &[0xEB, 0x0F],
        )],
    )
}

/// `meterscale` = 1: the ruler scale constant (15.0 in `.rdata`) becomes `meter`.
fn meter_scale(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let meter = ctx.values.float("gameplay", "meter")?;
    Ok(vec![word(
        eix_sites::RDATA_742430,
        15.0f32.to_bits(),
        meter.to_le_bytes(),
    )?])
}

/// `CHudUnitPanel::DrawUnitInfo`'s attack line, where the original adds the defence and hearing values:
/// `mov eax, [esi+0x1a0]`, which becomes `nop; jmp <added code>`.
const INFO_HEADER: Site = eix_sites::C_HUD_UNIT_PANEL_DRAW_UNIT_INFO__23C;
const INFO_HEADER_BYTES: [u8; 6] = [0x8B, 0x86, 0xA0, 0x01, 0x00, 0x00];
/// Where the added code goes back: `mov ecx, [ebp-0x28]`, then the four NOPs that replace the attack value's push.
const INFO_HEADER_BACK: Site = eix_sites::C_HUD_UNIT_PANEL_DRAW_UNIT_INFO__242;

/// The handler of `infowindowremake`: EAX is the unit's stats block; the handler pushes its attack (+0x34), the unit's
/// hearing byte (+0x25 of the record at `[ebp-0x1c]`) and its defence (+0x4c) for one combined format.
extern "C" fn info_header(arg: usize, ctx: &mut Context) {
    // SAFETY: `arg` is the tail `info_window_remake` kept.
    let tail = unsafe { common::kept::<&'static PushTail>(arg) };
    // SAFETY: ESI is the live unit panel, whose +0x1a0 is the unit's stats block the game reads here.
    let stats = unsafe { read::<u32>(ctx.esi + 0x1A0) };
    // SAFETY: `[ebp-0x1c]` is a local of `DrawUnitInfo` that holds the unit record the next lines read.
    let record = unsafe { read::<u32>(ctx.ebp.wrapping_sub(0x1C)) };
    // SAFETY: the stats block's attack, as the replaced code reads it.
    let attack = unsafe { read::<u32>(stats + 0x34) };
    // SAFETY: the record's hearing byte, which the original reads.
    let hearing = u32::from(unsafe { read::<u8>(record + 0x25) });
    // SAFETY: the stats block's defence, which the original reads.
    let defence = unsafe { read::<u32>(stats + 0x4C) };
    ctx.eax = stats;
    ctx.edx = defence;
    tail.resume(ctx, &[attack, hearing, defence]);
}

/// The format the moved `CString::Format` call reads. The original writes it over 15 bytes at 0x72e2e9, the last
/// eight of which are an exception-unwind handler of game code; kept here instead, as the same text.
static INFO_FORMAT: &[u8; 15] = b"%s %d    %s %d\0";

/// `DrawUnitInfo`+0x2aa..+0x34e: the attack and defence lines' two formats and draws, which the original rewrites
/// as one format of both values and one draw, padded with NOPs.
const INFO_LINES: Site = eix_sites::C_HUD_UNIT_PANEL_DRAW_UNIT_INFO__2AA;
const INFO_LINES_BYTES: [u8; 164] = [
    0x0C, 0x50, 0x68, 0x14, 0x2F, 0x79, 0x00, 0x51, 0xE8, 0x68, 0x6C, 0x0B, 0x00, 0x8B, 0x45, 0xEC,
    0x83, 0xC4, 0x10, 0x8D, 0x55, 0xC8, 0x8B, 0xCF, 0x6A, 0x01, 0x6A, 0x00, 0x68, 0xE4, 0xD7, 0xA7,
    0x00, 0x6A, 0x00, 0x52, 0x50, 0xE8, 0x0C, 0x6B, 0xFB, 0xFF, 0x6A, 0x0F, 0x8D, 0x4D, 0xC8, 0x6A,
    0x00, 0x51, 0xFF, 0xD3, 0x8B, 0x86, 0xA0, 0x01, 0x00, 0x00, 0x8B, 0x55, 0x94, 0x52, 0x8D, 0x55,
    0xEC, 0x8B, 0x48, 0x10, 0x51, 0x68, 0x14, 0x2F, 0x79, 0x00, 0x52, 0xE8, 0x25, 0x6C, 0x0B, 0x00,
    0x8B, 0x4D, 0xEC, 0x83, 0xC4, 0x10, 0x8D, 0x45, 0xC8, 0x6A, 0x01, 0x6A, 0x00, 0x68, 0xE4, 0xD7,
    0xA7, 0x00, 0x6A, 0x00, 0x50, 0x51, 0x8B, 0xCF, 0xE8, 0xC9, 0x6A, 0xFB, 0xFF, 0x6A, 0x0F, 0x8D,
    0x55, 0xC8, 0x6A, 0x00, 0x52, 0xFF, 0xD3, 0x8B, 0x85, 0x7C, 0xFF, 0xFF, 0xFF, 0x83, 0xF8, 0x07,
    0x73, 0x22, 0x50, 0x8D, 0x45, 0xEC, 0x68, 0x70, 0x2F, 0x79, 0x00, 0x50, 0xE8, 0xE4, 0x6B, 0x0B,
    0x00, 0x8B, 0x55, 0xEC, 0x83, 0xC4, 0x0C, 0x8D, 0x4D, 0xE8, 0x51, 0x8B, 0x4D, 0xE0, 0x52, 0xE8,
    0x92, 0x8C, 0xE9, 0xFF,
];
/// `CString::Format`, `CUiWindow::DrawText` and `CTextList::Add`, which the rewritten lines call.
const CSTRING_FORMAT: Site = eix_sites::C_STRING_FORMAT;
const DRAW_TEXT: Site = eix_sites::C_UI_SURFACE_DRAW_TEXT_IN;
const TEXT_LIST_ADD: Site = eix_sites::C_TEXT_MANAGER_GET_STRING;

/// The rewritten lines, `format` being the address of the combined format.
fn info_lines(format: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(INFO_LINES_BYTES.len());
    let call = |b: &mut Vec<u8>, to: Site| {
        let at = INFO_LINES.offset(b.len());
        b.extend_from_slice(&patch::call_rel32(at, to.ru()));
    };
    // The byte ends `mov eax, [edx+0x10]` (the defence); push eax; mov eax, [ebp-0x70]; push eax;
    // mov eax, [edx+0xc]; push eax; push format; push ecx.
    b.extend_from_slice(&[
        0x10, 0x50, 0x8B, 0x45, 0x90, 0x50, 0x8B, 0x42, 0x0C, 0x50, 0x68,
    ]);
    b.extend_from_slice(&format.to_le_bytes());
    b.push(0x51);
    call(&mut b, CSTRING_FORMAT);
    // mov ecx, [ebp-0x14]; add esp, 0x18; lea eax, [ebp-0x38]; push 1; push 0; push 0xa7d7e4; push 0; push eax;
    // push ecx; mov ecx, edi.
    b.extend_from_slice(&[
        0x8B, 0x4D, 0xEC, 0x83, 0xC4, 0x18, 0x8D, 0x45, 0xC8, 0x6A, 0x01, 0x6A, 0x00, 0x68, 0xE4,
        0xD7, 0xA7, 0x00, 0x6A, 0x00, 0x50, 0x51, 0x8B, 0xCF,
    ]);
    call(&mut b, DRAW_TEXT);
    // push 0xf; lea edx, [ebp-0x38]; push 0; push edx; call ebx; mov eax, [ebp-0x84]; cmp eax, 7; jae +0x22;
    // push eax; lea eax, [ebp-0x14]; push 0x792f70; push eax.
    b.extend_from_slice(&[
        0x6A, 0x0F, 0x8D, 0x55, 0xC8, 0x6A, 0x00, 0x52, 0xFF, 0xD3, 0x8B, 0x85, 0x7C, 0xFF, 0xFF,
        0xFF, 0x83, 0xF8, 0x07, 0x73, 0x22, 0x50, 0x8D, 0x45, 0xEC, 0x68, 0x70, 0x2F, 0x79, 0x00,
        0x50,
    ]);
    call(&mut b, CSTRING_FORMAT);
    // mov edx, [ebp-0x14]; add esp, 0xc; lea ecx, [ebp-0x18]; push ecx; mov ecx, [ebp-0x20]; push edx.
    b.extend_from_slice(&[
        0x8B, 0x55, 0xEC, 0x83, 0xC4, 0x0C, 0x8D, 0x4D, 0xE8, 0x51, 0x8B, 0x4D, 0xE0, 0x52,
    ]);
    call(&mut b, TEXT_LIST_ADD);
    b.resize(INFO_LINES_BYTES.len(), 0x90);
    b
}

/// `infowindowremake` = 1: attack and defence share one line, with hearing; the panel gets two more rows.
fn info_window_remake(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let tail = PushTail::new(3, INFO_HEADER_BACK)?;
    let mut writes = fixed(&[
        // The first format, in padding of `CSceneRenderer::SetSunAndAmbient`.
        (
            eix_sites::C_SCENE_RENDERER_SET_SUN_AND_AMBIENT__91,
            &[0x90; 15],
            b"%s %d  %s %d%s\0",
        ),
        // CHudUnitPanel::SetupVanilla: cmp ebx, 0x14.
        (eix_sites::C_HUD_UNIT_PANEL_SETUP__7C3, &[0x12], &[0x14]),
        // push 0x500891 instead of 0x792f48, and two more arguments popped.
        (
            eix_sites::C_HUD_UNIT_PANEL_DRAW_UNIT_INFO__252,
            &[0x48, 0x2F, 0x79],
            &[0x91, 0x08, 0x50],
        ),
        (
            eix_sites::C_HUD_UNIT_PANEL_DRAW_UNIT_INFO__261,
            &[0x14],
            &[0x1C],
        ),
        // mov ecx, [ebp-0x6c] instead of [ebp-0x70].
        (
            eix_sites::C_HUD_UNIT_PANEL_DRAW_UNIT_INFO__2A3,
            &[0x90],
            &[0x94],
        ),
    ])?;
    writes.extend(common::jump_at(
        INFO_HEADER,
        &INFO_HEADER_BYTES,
        1,
        info_header,
        common::keep(tail),
    )?);
    writes.push(SiteWrite::nop(
        eix_sites::C_HUD_UNIT_PANEL_DRAW_UNIT_INFO__245,
        &[0x8B, 0x50, 0x34, 0x52],
    ));
    writes.push(SiteWrite::bytes(
        INFO_LINES,
        &INFO_LINES_BYTES,
        &info_lines(INFO_FORMAT.as_ptr() as u32),
    )?);
    Ok(writes)
}

// --- The global map ----------------------------------------------------------------------------------------------

/// `CGlobalMap::Create`'s description window: `push 0x46; push 0xaa`, which becomes `nop; nop; jmp <added code>`; the
/// handler pushes `framey` and `framex` instead.
const ALLOD_WINDOW: Site = eix_sites::C_GLOBAL_MAP_CREATE__14C4;
const ALLOD_WINDOW_BYTES: [u8; 7] = [0x6A, 0x46, 0x68, 0xAA, 0x00, 0x00, 0x00];
const ALLOD_WINDOW_BACK: Site = eix_sites::C_GLOBAL_MAP_CREATE__14CB;

/// What the description window's handler pushes.
#[derive(Debug)]
struct AllodFrame {
    tail: &'static PushTail,
    height: u32,
    width: u32,
}

/// The handler of `allodsinfo`: the window's size from the INI (`atoi` of each key), height first.
extern "C" fn allod_window(arg: usize, ctx: &mut Context) {
    // SAFETY: `arg` is the frame `allods_info` kept.
    let f = unsafe { common::kept::<AllodFrame>(arg) };
    f.tail.resume(ctx, &[f.height, f.width]);
}

/// The immediate of a `mov dword [esp+disp32], imm32` holding `was`, set to `now`.
fn imm(site: Site, was: u32, now: i32) -> Result<SiteWrite, BuildError> {
    word(site, was, now.to_le_bytes())
}

/// `allodsinfo` = 1: the allod's description (`adesc <allod>` from texts.res) shows in a frame of `framex` by
/// `framey`.
fn allods_info(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let (fy, fx) = (
        ctx.values.float("gameplay", "framey")?,
        ctx.values.float("gameplay", "framex")?,
    );
    let (y, x) = (
        ctx.values.text_int("gameplay", "framey")?,
        ctx.values.text_int("gameplay", "framex")?,
    );
    let frame = AllodFrame {
        tail: PushTail::new(2, ALLOD_WINDOW_BACK)?,
        height: y as u32,
        width: x as u32,
    };
    let mut writes = common::jump_at(
        ALLOD_WINDOW,
        &ALLOD_WINDOW_BYTES,
        2,
        allod_window,
        common::keep(frame),
    )?;
    writes.extend([
        // The two frames' pushed sizes, as floats: the outer one 5 smaller.
        word(
            eix_sites::C_GLOBAL_MAP_CREATE__13BC,
            75.0f32.to_bits(),
            fy.to_le_bytes(),
        )?,
        word(
            eix_sites::C_GLOBAL_MAP_CREATE__13C1,
            175.0f32.to_bits(),
            fx.to_le_bytes(),
        )?,
        word(
            eix_sites::C_GLOBAL_MAP_CREATE__135E,
            70.0f32.to_bits(),
            (fy - 5.0).to_le_bytes(),
        )?,
        word(
            eix_sites::C_GLOBAL_MAP_CREATE__1363,
            170.0f32.to_bits(),
            (fx - 5.0).to_le_bytes(),
        )?,
        // The two text boxes' style.
        SiteWrite::bytes(eix_sites::C_GLOBAL_MAP_CREATE__1544, &[0x25], &[0x11])?,
        SiteWrite::bytes(eix_sites::C_GLOBAL_MAP_CREATE__14FD, &[0x25], &[0x11])?,
        // Their rectangles, as integers.
        imm(
            eix_sites::C_GLOBAL_MAP_CREATE__1529,
            0x96,
            x.wrapping_sub(20),
        )?,
        imm(
            eix_sites::C_GLOBAL_MAP_CREATE__1534,
            0x19,
            y.wrapping_sub(45),
        )?,
        imm(
            eix_sites::C_GLOBAL_MAP_CREATE__1565,
            0x19,
            y.wrapping_sub(45),
        )?,
        imm(
            eix_sites::C_GLOBAL_MAP_CREATE__1570,
            0x96,
            x.wrapping_sub(20),
        )?,
        imm(
            eix_sites::C_GLOBAL_MAP_CREATE__157B,
            0x2D,
            y.wrapping_sub(25),
        )?,
    ]);
    writes.extend(fixed(&[
        // The text key's prefix, in padding of `SaveEntry::ZoneTitle`, and `CGlobalMap::AllodTitle` pushing it.
        (
            eix_sites::SAVE_ENTRY_ZONE_TITLE__F9,
            &[0x90; 7],
            b"adesc \0",
        ),
        (
            eix_sites::C_GLOBAL_MAP_ALLOD_TITLE__3E,
            &[0x00, 0x42, 0x79, 0x00],
            &[0xC9, 0x3C, 0x65, 0x00],
        ),
    ])?);
    Ok(writes)
}

/// `exploredcolors` = 1: the explored zones' colour (0.5, 0.5, 0.5) in `CGlobalMap::SelectZone` becomes `exred`,
/// `exgreen`, `exblue`, read as floats.
fn explored_colors(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    [
        (eix_sites::C_GLOBAL_MAP_SELECT_ZONE__120, "exred"),
        (eix_sites::C_GLOBAL_MAP_SELECT_ZONE__132, "exgreen"),
        (eix_sites::C_GLOBAL_MAP_SELECT_ZONE__13A, "exblue"),
    ]
    .into_iter()
    .map(|(site, key)| {
        let v = ctx.values.float("gameplay", key)?;
        word(site, 0.5f32.to_bits(), v.to_le_bytes())
    })
    .collect()
}

/// `dontmergegipathzones` = 1: `CGlobalMap::Create` keeps the Gipat zones apart.
fn dont_merge_gipat_zones(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    when_on(
        value,
        &[(eix_sites::C_GLOBAL_MAP_CREATE__1726, &[0x75], &[0xEB])],
    )
}

// --- The main menu, fonts, light -------------------------------------------------------------------------------

/// `mainmenuweather`: 1 makes `CMainMenu::Create`'s weather type 2 at strength 40, 2 skips the weather.
fn main_menu_weather(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    match value {
        1 => fixed(&[(
            eix_sites::C_MAIN_MENU_CREATE__211,
            &[0xA0, 0x41, 0x6A, 0x01],
            &[0x20, 0x42, 0x6A, 0x02],
        )]),
        2 => fixed(&[(eix_sites::C_MAIN_MENU_CREATE__202, &[0x74], &[0xEB])]),
        _ => Ok(Vec::new()),
    }
}

/// `lightsboost` = 1: the light strength constant (0.001 in `.rdata`) becomes `newlightsvalue`.
fn lights_boost(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let v = ctx.values.float("gameplay", "newlightsvalue")?;
    Ok(vec![word(
        eix_sites::RDATA_74040C,
        0.001f32.to_bits(),
        v.to_le_bytes(),
    )?])
}

/// The four font slots in `.data`, 0x30 bytes each, that `newfont1` .. `newfont4` are copied to.
const FONT_SLOTS: [Site; 4] = [
    eix_sites::DATA_79A290,
    eix_sites::DATA_79A2C0,
    eix_sites::DATA_79A2F0,
    eix_sites::DATA_79A320,
];
/// `CInterface3D::Create`'s four `push 0x793034` ("Times New Roman"), in the order of the slots they get.
const FONT_PUSHES: [Site; 4] = [
    eix_sites::C_INTERFACE_3D_CREATE__FD,
    eix_sites::C_INTERFACE_3D_CREATE__173,
    eix_sites::C_INTERFACE_3D_CREATE__1D8,
    eix_sites::C_INTERFACE_3D_CREATE__65,
];
const TIMES_PUSH: [u8; 5] = [0x68, 0x34, 0x30, 0x79, 0x00];

/// `changefont` >= 1: the interface's four fonts are `newfont1` .. `newfont4`. 2 also adds the font file
/// `newfont5` from spelladdon.ini's folder, for this process only.
fn change_font(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    let mut writes = Vec::with_capacity(8);
    for (i, (slot, push)) in FONT_SLOTS.into_iter().zip(FONT_PUSHES).enumerate() {
        // The original copies its whole 0x30-byte buffer, which it clears before the read, so zeros follow the name.
        let mut name = ctx
            .values
            .text("fonts", &format!("newfont{}", i + 1))?
            .to_vec();
        name.resize(0x30, 0);
        writes.push(SiteWrite::bytes(slot, &[0; 0x30], &name)?);
        let mut new = [0x68, 0, 0, 0, 0];
        new[1..].copy_from_slice(&slot.ru32().to_le_bytes());
        writes.push(SiteWrite::bytes(push, &TIMES_PUSH, &new)?);
    }
    if value == 2 {
        let mut path = ctx.ini_dir.clone();
        path.push(b'\\');
        path.extend_from_slice(ctx.values.text("fonts", "newfont5")?);
        // The original ignores the result; a failure is logged, the four names still apply.
        if ansi::add_private_font(&path) == 0 {
            (ctx.log)(
                Level::Warn,
                "spelladdon",
                &format!(
                    "changefont: the font file {} was not added",
                    String::from_utf8_lossy(&path)
                ),
                "",
            );
        }
    }
    Ok(writes)
}

/// `drawvisiblepatch` = 1: `GameFlow::StartZone` no longer resets "draw visible".
fn draw_visible_patch(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    when_on(
        value,
        &[(eix_sites::GAME_FLOW_START_ZONE__24D, &[0x75], &[0xEB])],
    )
}

/// `SelectLoadingMovie`'s "gipat zone: progres2.bik" test, which becomes `mov [0x791ee0], ecx` (the movie is named
/// after the zone) and NOPs.
const LOADING_TEST: Bytes = (
    eix_sites::SELECT_LOADING_MOVIE__50,
    &[
        0x68, 0x4C, 0x01, 0x79, 0x00, 0x51, 0xE8, 0xB0, 0x33, 0x15, 0x00, 0x83, 0xC4, 0x08, 0x85,
        0xC0, 0x75, 0x0C, 0xC7, 0x05, 0xE0, 0x1E, 0x79, 0x00, 0x9C, 0x0A, 0x79, 0x00,
    ],
    &[
        0x89, 0x0D, 0xE0, 0x1E, 0x79, 0x00, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
        0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    ],
);

/// `playingnewloadingscreens`: 1 names the loading movie after the zone; 2 also pushes a name kept in the
/// function's padding instead of 0x78b0ec.
fn new_loading_screens(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    match value {
        1 => fixed(&[LOADING_TEST]),
        2 => fixed(&[
            LOADING_TEST,
            (
                eix_sites::SELECT_LOADING_MOVIE__BF,
                &[0xEC, 0xB0, 0x78],
                &[0x99, 0x51, 0x59],
            ),
            (eix_sites::SELECT_LOADING_MOVIE__119, &[0x90; 5], b"gz1g\0"),
        ]),
        _ => Ok(Vec::new()),
    }
}

// --- Sound and music ---------------------------------------------------------------------------------------------

/// `notrash` = 1: a `.data` pointer (0x78bbd4) points at 0x792564, and `AdvanceTime` tests motion mode 1.
fn no_trash(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    when_on(
        value,
        &[
            (
                eix_sites::C_UNIT_CLIENT_SPECIFIC_ADVANCE_TIME__143,
                &[0x02, 0x75],
                &[0x01, 0x74],
            ),
            (
                eix_sites::DATA_79337C,
                &[0xD4, 0xBB, 0x78],
                &[0x64, 0x25, 0x79],
            ),
        ],
    )
}

/// `musictimings` = 1: `CMusicSystem::UpdateMusic`'s pause between tracks is `minmusictime` plus a random number
/// modulo `maxmusictime - minmusictime`, at its three draws. With `maxmusictime` not above `minmusictime` the modulus
/// is 1, so the pause is `minmusictime` (the original's modulus is then 0, and the game's division faults, or wraps).
fn music_timings(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let min = ctx.values.int("gameplay", "minmusictime")?;
    let max = ctx.values.int("gameplay", "maxmusictime")?;
    let range = if max > min { max.wrapping_sub(min) } else { 1 }.to_le_bytes();
    let mut writes = Vec::with_capacity(6);
    for (modulus, minimum) in [
        (
            eix_sites::C_MUSIC_SYSTEM_UPDATE_MUSIC__B1,
            eix_sites::C_MUSIC_SYSTEM_UPDATE_MUSIC__BB,
        ),
        (
            eix_sites::C_MUSIC_SYSTEM_UPDATE_MUSIC__102,
            eix_sites::C_MUSIC_SYSTEM_UPDATE_MUSIC__10C,
        ),
        (
            eix_sites::C_MUSIC_SYSTEM_UPDATE_MUSIC__241,
            eix_sites::C_MUSIC_SYSTEM_UPDATE_MUSIC__24A,
        ),
    ] {
        writes.push(word(modulus, 0xEA61, range)?);
        writes.push(word(minimum, 120_000, min.to_le_bytes())?);
    }
    Ok(writes)
}

/// `CWorldClient::LoadZone`'s `mov ebx, [esi+0x6e0]` before the zone music, which becomes `nop; jmp <added code>`.
const ZONE_MUSIC: Site = eix_sites::C_WORLD_CLIENT_LOAD_ZONE__6C3;
const ZONE_MUSIC_BYTES: [u8; 6] = [0x8B, 0x9E, 0xE0, 0x06, 0x00, 0x00];
/// The zone the handler looks for, kept by the original in padding of `LoadZone`; its address is also the music name
/// it passes.
const ZONE_MUSIC_NAME: Site = eix_sites::C_WORLD_CLIENT_LOAD_ZONE__8E5;
/// `LoadZone`'s own path (`mov edi, [edi]`) and the call of `CMusicSystem::SetZoneMode`.
const ZONE_MUSIC_OWN: Site = eix_sites::C_WORLD_CLIENT_LOAD_ZONE__6C9;
const ZONE_MUSIC_CALL: Site = eix_sites::C_WORLD_CLIENT_LOAD_ZONE__6D8;

/// The handler of `newzonescpecificmusic` = 1: in zone `gz12d2` (EDX the zone's name), the music is the one named
/// `gz12d2`, its flag `[esi+0x6e0] == 1`; elsewhere `LoadZone` goes on as before.
extern "C" fn zone_music(arg: usize, ctx: &mut Context) {
    // SAFETY: `arg` is the tail `zone_specific_music` kept.
    let tail = unsafe { common::kept::<&'static PushTail>(arg) };
    // SAFETY: ESI is the live client world, whose +0x6e0 the replaced instruction reads.
    let mode = unsafe { read::<u32>(ctx.esi + 0x6E0) };
    ctx.ebx = mode;
    ctx.ecx = ctx.ebp.wrapping_add(8);
    // SAFETY: EDX is the zone's name, a C string, as the original's `_mbscmp` takes it.
    if unsafe { common::c_str_is(ctx.edx, b"gz12d2") } {
        let flag = u32::from(mode == 1);
        ctx.edx = flag;
        tail.resume(ctx, &[flag, ZONE_MUSIC_NAME.here32()]);
    } else {
        ctx.resume = ZONE_MUSIC_OWN;
    }
}

/// `newzonescpecificmusic`: 1 gives zone `gz12d2` its own music; 2 and 3 take the music name from another field in
/// `LoadZone`, `CBriefingScreen::Create` and `CCampPages::Create` (3 leaves the camp's push) and cut
/// `CMusicSystem::SetZoneMode` short.
fn zone_specific_music(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    // The operand byte of `mov edi, [edi]` at 0x568179: `mov edi, edx`.
    const LOAD_ZONE: Bytes = (eix_sites::C_WORLD_CLIENT_LOAD_ZONE__6CA, &[0x3F], &[0xFA]);
    const BRIEFING: Bytes = (
        eix_sites::C_BRIEFING_SCREEN_CREATE__426,
        &[0x8B, 0x47, 0x04],
        &[0x8D, 0x41, 0x0C],
    );
    const SET_ZONE_MODE: Bytes = (
        eix_sites::C_MUSIC_SYSTEM_SET_ZONE_MODE__7D,
        &[0x53, 0x8D],
        &[0xEB, 0x72],
    );
    match value {
        1 => {
            let tail = PushTail::new(2, ZONE_MUSIC_CALL)?;
            let mut writes = fixed(&[(ZONE_MUSIC_NAME, &[0x90; 8], b"gz12d2\0\xCC")])?;
            writes.extend(common::jump_at(
                ZONE_MUSIC,
                &ZONE_MUSIC_BYTES,
                1,
                zone_music,
                common::keep(tail),
            )?);
            Ok(writes)
        }
        2 => fixed(&[
            LOAD_ZONE,
            BRIEFING,
            (
                eix_sites::C_CAMP_PAGES_CREATE__18EE,
                &[0x8B, 0x76, 0x04],
                &[0x8D, 0x41, 0x0C],
            ),
            (eix_sites::C_CAMP_PAGES_CREATE__18F7, &[0x56], &[0x50]),
            SET_ZONE_MODE,
        ]),
        3 => fixed(&[
            LOAD_ZONE,
            BRIEFING,
            (
                eix_sites::C_CAMP_PAGES_CREATE__18EE,
                &[0x8B, 0x76, 0x04],
                &[0x8D, 0x76, 0x04],
            ),
            SET_ZONE_MODE,
        ]),
        _ => Ok(Vec::new()),
    }
}

// --- World and zones ---------------------------------------------------------------------------------------------

/// `disabledragon` = 1: `CWorldClient::OnQuestVariable` skips its dragon handling and `CEffectMoshka::Tick` returns
/// at once.
fn disable_dragon(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    when_on(
        value,
        &[
            (
                eix_sites::C_WORLD_CLIENT_ON_QUEST_VARIABLE__D4,
                &[0x74],
                &[0xEB],
            ),
            (
                eix_sites::C_EFFECT_MOSHKA_TICK,
                &[0xDB, 0x05],
                &[0xEB, 0x0F],
            ),
        ],
    )
}

/// `noruinszoneanimation` = 1: `CWorldServer::CreateUnits` skips zone 1's intro script.
fn no_ruins_zone_animation(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    when_on(
        value,
        &[(
            eix_sites::C_WORLD_SERVER_CREATE_UNITS__AFC,
            &[0x74],
            &[0xEB],
        )],
    )
}

/// `bigmapspatch` = 1: the network position encoders scale by two floats kept in padding of
/// `CHudSpellColumn::Construct` instead of 0x73caa0 and 0x73caa4.
fn big_maps_patch(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    when_on(
        value,
        &[
            (
                eix_sites::DELTA_FIELD_WRITE_POSITION_2D__18,
                &[0xA0, 0xCA, 0x73],
                &[0x38, 0xE4, 0x63],
            ),
            (
                eix_sites::DELTA_FIELD_WRITE_POSITION_2D__DD,
                &[0xA0, 0xCA, 0x73],
                &[0x38, 0xE4, 0x63],
            ),
            (
                eix_sites::DELTA_FIELD_READ_POSITION_2D__B1,
                &[0xA4, 0xCA, 0x73],
                &[0x3C, 0xE4, 0x63],
            ),
            (
                eix_sites::DELTA_FIELD_READ_POSITION_2D__163,
                &[0xA4, 0xCA, 0x73],
                &[0x3C, 0xE4, 0x63],
            ),
            (
                eix_sites::C_HUD_SPELL_COLUMN_CONSTRUCT__198,
                &[0x90; 8],
                &[0x00, 0xF0, 0x70, 0x41, 0x88, 0x00, 0x88, 0x3D],
            ),
        ],
    )
}

/// `CWorldServer::SetZoneExits`+0x50: `mov [esp+0x10], esi; cmp eax, esi`, which becomes `nop; jmp <added code>`. EAX
/// is the zone's exit count, which the handler lowers by (1) or sets to (2) the host's script variable `Exits`.
const ZONE_EXITS: Site = eix_sites::C_WORLD_SERVER_SET_ZONE_EXITS__50;
const ZONE_EXITS_BYTES: [u8; 6] = [0x89, 0x74, 0x24, 0x10, 0x3B, 0xC6];
const ZONE_EXITS_BACK: Site = eix_sites::C_WORLD_SERVER_SET_ZONE_EXITS__56;

/// The handler of `hiddenzonesexits`; `arg` is 1 to subtract the variable, 2 to replace the count by it.
extern "C" fn zone_exits(arg: usize, ctx: &mut Context) {
    if let Some(player) = common::player_zero() {
        // SAFETY: a player GetPlayer returns is live, so its field's place is valid.
        let table = unsafe { &raw const (*player).variables };
        // SAFETY: the player's variable table, read on the game's thread.
        let v = unsafe { NamedFloatTable::get(table, c"Exits") };
        // An absent variable reads as the lookup's default, 0.
        let v = v.unwrap_or(0.0);
        // The original stores the lookup's ST0 with `fistp`, under the game's rounding.
        let n = Ext::new(f64::from(v)).fistp() as u32;
        ctx.eax = if arg == 1 { ctx.eax.wrapping_sub(n) } else { n };
        // SAFETY: EBP is the zone record `SetZoneExits` reads its count from at +8.
        unsafe { common::write(ctx.ebp + 8, ctx.eax) };
    }
    ctx.set_stack_word(4, ctx.esi);
    ctx.eflags = x86_flags::cmp(ctx.eflags, ctx.eax, ctx.esi);
    ctx.resume = ZONE_EXITS_BACK;
}

/// `hiddenzonesexits` >= 1: a zone's exits are reduced by the host's script variable `Exits`; >= 2 they are that
/// variable.
fn hidden_zones_exits(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    // The variable's name, in padding of `NegateVector`, which the original passes.
    let mut writes = fixed(&[(eix_sites::NEGATE_VECTOR__19, &[0x90; 7], b"Exits\0\0")])?;
    writes.extend(common::jump_at(
        ZONE_EXITS,
        &ZONE_EXITS_BYTES,
        1,
        zone_exits,
        if value >= 2 { 2 } else { 1 },
    )?);
    Ok(writes)
}

/// `segmentsonexitenter` = 1: `EnterZone`'s segment count is capped at `maxsegments` and gets `addsegments` added;
/// its activity radius becomes `exitenteractivityradius`.
fn segments_on_exit_enter(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let max = ctx.values.int("gameplay", "maxsegments")?.to_le_bytes();
    let add = ctx.values.int("gameplay", "addsegments")?.to_le_bytes();
    let radius = ctx.values.float("gameplay", "exitenteractivityradius")?;
    let mut add_eax = [0x90, 0x05, 0, 0, 0, 0];
    add_eax[2..].copy_from_slice(&add);
    Ok(vec![
        word(eix_sites::ENTER_ZONE__4B2, 1000, max)?,
        // jl to the `add` below.
        SiteWrite::bytes(eix_sites::ENTER_ZONE__4B7, &[0x07], &[0x05])?,
        word(eix_sites::ENTER_ZONE__4B9, 1000, max)?,
        // `jmp +4; test eax, eax; jle` becomes `nop; add eax, addsegments`.
        SiteWrite::bytes(
            eix_sites::ENTER_ZONE__4BD,
            &[0xEB, 0x04, 0x85, 0xC0, 0x7E, 0x62],
            &add_eax,
        )?,
        word(
            eix_sites::ENTER_ZONE__507,
            70.0f32.to_bits(),
            radius.to_le_bytes(),
        )?,
    ])
}

/// `CWorldClient::EnvironmentFlags`+0x70, `mov eax, [edi+0x184]` (the zone's name) before its last name test,
/// which becomes `nop; jmp <added code>`.
const ZONE_FLAGS: Site = eix_sites::C_WORLD_CLIENT_ENVIRONMENT_FLAGS__70;
const ZONE_FLAGS_BYTES: [u8; 6] = [0x8B, 0x87, 0x84, 0x01, 0x00, 0x00];
const ZONE_FLAGS_OWN: Site = eix_sites::C_WORLD_CLIENT_ENVIRONMENT_FLAGS__76;
const ZONE_FLAGS_DONE: Site = eix_sites::C_WORLD_CLIENT_ENVIRONMENT_FLAGS__8B;
/// The new allods' zone names; the n-th sets environment flag bit 15 + n.
const NEW_ALLODS: [&[u8]; 9] = [
    b"jigran", b"neloya", b"eyn", b"bebesh", b"kvatokh", b"nokks", b"umoir", b"yazes", b"yiul",
];

/// The handler of `newallodsacks`: a zone named after a new allod gets that allod's flag in ESI.
extern "C" fn zone_flags(_: usize, ctx: &mut Context) {
    // SAFETY: EDI is the live client world, whose +0x184 is the zone's name the replaced instruction loads.
    let name = unsafe { read::<u32>(ctx.edi + 0x184) };
    let found = NEW_ALLODS.iter().position(|n| {
        // SAFETY: the zone's name is a C string, which the game's own tests compare.
        unsafe { common::c_str_is(name, n) }
    });
    match found {
        Some(i) => {
            ctx.esi |= 0x8000 << i;
            ctx.eax = 0;
            ctx.resume = ZONE_FLAGS_DONE;
        }
        None => {
            ctx.eax = name;
            ctx.resume = ZONE_FLAGS_OWN;
        }
    }
}

/// `newallodsacks` = 1: the new allods' zones get environment flags 0x8000 .. 0x800000.
fn new_allods_acks(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(common::jump_at(
        ZONE_FLAGS,
        &ZONE_FLAGS_BYTES,
        1,
        zone_flags,
        0,
    )?)
}

/// `newcounters` = 1: `UnitVitals::Regenerate` keeps its counter at +4 in one byte.
fn new_counters(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    when_on(
        value,
        &[
            (eix_sites::UNIT_VITALS_REGENERATE__9, &[0x8B], &[0x8A]),
            (eix_sites::UNIT_VITALS_REGENERATE__F, &[0x89], &[0x88]),
            (eix_sites::UNIT_VITALS_REGENERATE__2D, &[0xC7], &[0xC6]),
            (
                eix_sites::UNIT_VITALS_REGENERATE__31,
                &[0x00, 0x00, 0x00],
                &[0x90, 0x90, 0x90],
            ),
        ],
    )
}

/// `nogameoveronleadersdeath` = 1: `CInterface3D::OnGameLost` does not end the game.
fn no_game_over_on_leaders_death(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    when_on(
        value,
        &[(eix_sites::C_INTERFACE_3D_ON_GAME_LOST__8, &[0x74], &[0xEB])],
    )
}

// --- Fixes -------------------------------------------------------------------------------------------------------

/// `CPlayerServer::SendObjectsAdded`+0xde: `mov ebx, [ebp+0x170]`, which becomes `nop; jmp <added code>`.
const SEND_OBJECT: Site = eix_sites::C_PLAYER_SERVER_SEND_OBJECTS_ADDED__DE;
const SEND_OBJECT_BYTES: [u8; 6] = [0x8B, 0x9D, 0x70, 0x01, 0x00, 0x00];
const SEND_OBJECT_ON: Site = eix_sites::C_PLAYER_SERVER_SEND_OBJECTS_ADDED__E4;
const SEND_OBJECT_NEXT: Site = eix_sites::C_PLAYER_SERVER_SEND_OBJECTS_ADDED__29;

/// The handler of `ultrafixes`: an object without the pointer at +0x170 goes to the loop's next element.
extern "C" fn send_object(_: usize, ctx: &mut Context) {
    // SAFETY: EBP is the live object the replaced instruction reads.
    ctx.ebx = unsafe { read::<u32>(ctx.ebp + 0x170) };
    ctx.resume = if ctx.ebx == 0 {
        SEND_OBJECT_NEXT
    } else {
        SEND_OBJECT_ON
    };
}

/// `ultrafixes` = 1: `SendObjectsAdded` tests `[ebp+0x170]` for null before it uses it.
fn ultra_fixes(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(common::jump_at(
        SEND_OBJECT,
        &SEND_OBJECT_BYTES,
        1,
        send_object,
        0,
    )?)
}

/// `UnitOrderLess::Less`: its first null test becomes `jmp` to "not less"; and the second object's field is loaded
/// after the first's test.
const LESS_ALWAYS_NO: Bytes = (eix_sites::UNIT_ORDER_LESS_LESS__8, &[0x74], &[0xEB]);
const LESS_REORDERED: Bytes = (
    eix_sites::UNIT_ORDER_LESS_LESS__18,
    &[0x8B, 0x8E, 0x94, 0x00, 0x00, 0x00, 0x85, 0xC0, 0x74, 0x62],
    &[0x85, 0xC0, 0x74, 0x68, 0x8B, 0x8E, 0x94, 0x00, 0x00, 0x00],
);
/// `CWorldServer::LoadSave`+0x8b9: `or ebx, -1` moved after the read it set up, so the read's target keeps EBX.
const LOAD_OR_LATER: Bytes = (
    eix_sites::C_WORLD_SERVER_LOAD_SAVE__8B9,
    &[
        0x83, 0xCB, 0xFF, 0x6A, 0x04, 0x50, 0x8B, 0xCE, 0x89, 0x5C, 0x24, 0x1C, 0xFF, 0x52, 0x04,
    ],
    &[
        0x6A, 0x04, 0x50, 0x8B, 0xCE, 0x89, 0x5C, 0x24, 0x1C, 0xFF, 0x52, 0x04, 0x83, 0xCB, 0xFF,
    ],
);
/// As [`LOAD_OR_LATER`], with `and ebx, 0` instead of `or ebx, -1`.
const LOAD_ZERO_LATER: Bytes = (
    eix_sites::C_WORLD_SERVER_LOAD_SAVE__8B9,
    &[
        0x83, 0xCB, 0xFF, 0x6A, 0x04, 0x50, 0x8B, 0xCE, 0x89, 0x5C, 0x24, 0x1C, 0xFF, 0x52, 0x04,
    ],
    &[
        0x6A, 0x04, 0x50, 0x8B, 0xCE, 0x89, 0x5C, 0x24, 0x1C, 0xFF, 0x52, 0x04, 0x83, 0xE3, 0x00,
    ],
);
/// `LoadSave`+0x8ad, +0xb68, +0xbd7: the three section-count tests jump unconditionally.
const LOAD_SKIP_FIRST: Bytes = (
    eix_sites::C_WORLD_SERVER_LOAD_SAVE__8AD,
    &[0x0F, 0x8E],
    &[0x90, 0xE9],
);
const LOAD_SKIP_SECOND: Bytes = (eix_sites::C_WORLD_SERVER_LOAD_SAVE__B68, &[0x7E], &[0xEB]);
const LOAD_SKIP_THIRD: Bytes = (
    eix_sites::C_WORLD_SERVER_LOAD_SAVE__BD7,
    &[0x0F, 0x8E],
    &[0x90, 0xE9],
);

/// `LoadSave`+0xb7e: `mov eax, [esp+0x10]; xor ebp, ebp` after an object id is read, which becomes
/// `jmp <added code>; nop`.
const LOADED_ID: Site = eix_sites::C_WORLD_SERVER_LOAD_SAVE__B7E;
const LOADED_ID_BYTES: [u8; 6] = [0x8B, 0x44, 0x24, 0x10, 0x33, 0xED];
const LOADED_ID_BACK: Site = eix_sites::C_WORLD_SERVER_LOAD_SAVE__B84;

/// The handler of `expirementalfxes` 2 to 4: the loaded id keeps its low 16 bits.
extern "C" fn loaded_id(_: usize, ctx: &mut Context) {
    ctx.eax = ctx.stack_word(4).unwrap_or(0) & 0xFFFF;
    ctx.set_stack_word(4, ctx.eax);
    ctx.ebp = 0;
    ctx.resume = LOADED_ID_BACK;
}

/// `LoadSave`+0x8c1: `mov [esp+0x1c], ebx; call [edx+4]` (the read of the next object's id, -1 at the end), which
/// becomes `jmp <added code>; nop; nop`. The handler makes the read without setting its target to -1 first.
const NEXT_ID: Site = eix_sites::C_WORLD_SERVER_LOAD_SAVE__8C1;
const NEXT_ID_BYTES: [u8; 7] = [0x89, 0x5C, 0x24, 0x1C, 0xFF, 0x52, 0x04];
/// The NOPs after the jump, before the loop's end test, and the loop's exit.
const NEXT_ID_BACK: Site = eix_sites::C_WORLD_SERVER_LOAD_SAVE__8C6;
const LOAD_LOOP_EXIT: Site = eix_sites::C_WORLD_SERVER_LOAD_SAVE__937;
/// The handler's iteration count, a word of header slack (0x4004fc) in the original.
static LOAD_ITERATIONS: AtomicU32 = AtomicU32::new(0);
/// Past this count the handler leaves the loop and starts over at 0.
const LOAD_ITERATIONS_MAX: u32 = 0x1_9445;

/// The handler of `expirementalfxes` 7, 8 and 10, after the read: a read id other than EBX (-1) counts an iteration;
/// past [`LOAD_ITERATIONS_MAX`] of them the loop ends.
extern "C" fn next_id(_: usize, ctx: &mut Context) {
    if ctx.stack_word(5) == Some(ctx.ebx) {
        ctx.resume = NEXT_ID_BACK;
        return;
    }
    let n = LOAD_ITERATIONS.load(Ordering::Relaxed);
    if n > LOAD_ITERATIONS_MAX {
        LOAD_ITERATIONS.store(0, Ordering::Relaxed);
        ctx.resume = LOAD_LOOP_EXIT;
    } else {
        LOAD_ITERATIONS.store(n + 1, Ordering::Relaxed);
        ctx.resume = NEXT_ID_BACK;
    }
}

/// The site of [`next_id`]: a jump to a stub that makes the replaced `call [edx+4]` and then runs the handler.
fn next_id_writes() -> Result<Vec<SiteWrite>, BuildError> {
    let stub = common::jump_stub(
        NEXT_ID.offset(4),
        &NEXT_ID_BYTES[4..],
        next_id,
        0,
        NEXT_ID_BACK,
    )?;
    Ok(vec![SiteWrite::jmp(NEXT_ID, &NEXT_ID_BYTES, stub)?])
}

/// `CUnitServer::PropagateHostility`+0x89: `mov eax, 1`, which becomes `jmp <added code>`.
const HOSTILITY: Site = eix_sites::C_UNIT_SERVER_PROPAGATE_HOSTILITY__89;
const HOSTILITY_ON: Site = eix_sites::C_UNIT_SERVER_PROPAGATE_HOSTILITY__8E;
const HOSTILITY_NEXT: Site = eix_sites::C_UNIT_SERVER_PROPAGATE_HOSTILITY__118;

/// The handler at `PropagateHostility`: an entry without a unit (ESI) goes to the next.
extern "C" fn hostility(_: usize, ctx: &mut Context) {
    ctx.eax = 1;
    ctx.resume = if ctx.esi == 0 {
        HOSTILITY_NEXT
    } else {
        HOSTILITY_ON
    };
}

/// `AIViewer::See`+0x68: `mov [esp+0x20], ecx; mov ecx, esi`, which becomes `jmp <added code>`; the original moves the
/// following `mov edx, [esi]` two bytes up, followed by a NOP.
const SEE: Site = eix_sites::AI_VIEWER_SEE__68;
const SEE_ON: Site = eix_sites::AI_VIEWER_SEE__6D;
const SEE_DONE: Site = eix_sites::AI_VIEWER_SEE__447;

/// The handler at `AIViewer::See`: a null object (ESI) ends the function.
extern "C" fn see(_: usize, ctx: &mut Context) {
    ctx.set_stack_word(8, ctx.ecx);
    ctx.ecx = ctx.esi;
    ctx.resume = if ctx.esi == 0 { SEE_DONE } else { SEE_ON };
}

/// The drawing skips of `expirementalfxes` 10 in `CWorldClient::DrawPass`, and `RemoveObject`'s jump to a new
/// `jmp 0x5939c7` in `CreateObject`'s padding.
const DRAW_PASS_SKIPS: [Bytes; 10] = [
    (
        eix_sites::C_WORLD_CLIENT_DRAW_PASS__50B,
        &[0x8B, 0x7D],
        &[0xEB, 0x22],
    ),
    (
        eix_sites::C_WORLD_CLIENT_DRAW_PASS__558,
        &[0x03, 0xC9],
        &[0xEB, 0x19],
    ),
    (
        eix_sites::C_WORLD_CLIENT_DRAW_PASS__59C,
        &[0x8D, 0x14],
        &[0xEB, 0x1A],
    ),
    (
        eix_sites::C_WORLD_CLIENT_DRAW_PASS__5E1,
        &[0x8D, 0x04],
        &[0xEB, 0x1A],
    ),
    (
        eix_sites::C_WORLD_CLIENT_DRAW_PASS__9D8,
        &[0x8B, 0x7D],
        &[0xEB, 0x21],
    ),
    (
        eix_sites::C_WORLD_CLIENT_DRAW_PASS__A24,
        &[0x8D, 0x14],
        &[0xEB, 0x1A],
    ),
    (
        eix_sites::C_WORLD_CLIENT_DRAW_PASS__A69,
        &[0x8D, 0x04],
        &[0xEB, 0x1A],
    ),
    (
        eix_sites::C_WORLD_CLIENT_DRAW_PASS__AAE,
        &[0x03, 0xC9],
        &[0xEB, 0x19],
    ),
    (
        eix_sites::C_WORLD_SERVER_CREATE_OBJECT__DFB,
        &[0x90; 5],
        &[0xE9, 0x07, 0x04, 0x00, 0x00],
    ),
    (
        eix_sites::C_WORLD_SERVER_REMOVE_OBJECT__5E,
        &[0x07],
        &[0x9C],
    ),
];

/// `expirementalfxes`: ten separate changes to the unit sort comparator and the save loading loop (value 10 also
/// null tests in `PropagateHostility` and `AIViewer::See`, and drawing skips); 9 changes nothing.
fn experimental_fixes(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    let id_mask = || common::jump_at(LOADED_ID, &LOADED_ID_BYTES, 0, loaded_id, 0);
    let mut writes = Vec::new();
    match value {
        1 => writes.extend(fixed(&[LESS_ALWAYS_NO, LESS_REORDERED])?),
        2 => {
            writes.extend(id_mask()?);
            writes.extend(fixed(&[LOAD_OR_LATER])?);
        }
        3 => {
            writes.extend(id_mask()?);
            writes.extend(fixed(&[LESS_ALWAYS_NO, LESS_REORDERED, LOAD_OR_LATER])?);
        }
        4 => {
            writes.extend(id_mask()?);
            writes.extend(fixed(&[LESS_ALWAYS_NO, LESS_REORDERED, LOAD_SKIP_FIRST])?);
        }
        5 => writes.extend(fixed(&[LESS_ALWAYS_NO, LESS_REORDERED, LOAD_ZERO_LATER])?),
        6 => writes.extend(fixed(&[
            LOAD_SKIP_FIRST,
            LOAD_SKIP_SECOND,
            LOAD_SKIP_THIRD,
        ])?),
        7 => writes.extend(next_id_writes()?),
        8 => {
            writes.extend(next_id_writes()?);
            writes.extend(fixed(&[LESS_ALWAYS_NO, LESS_REORDERED])?);
        }
        10 => {
            writes.extend(next_id_writes()?);
            writes.extend(common::jump_at(
                HOSTILITY,
                &[0xB8, 0x01, 0x00, 0x00, 0x00],
                0,
                hostility,
                0,
            )?);
            writes.extend(fixed(&DRAW_PASS_SKIPS)?);
            writes.extend(fixed(&[LESS_REORDERED])?);
            writes.extend(common::jump_at(
                SEE,
                &[0x89, 0x4C, 0x24, 0x20, 0x8B],
                0,
                see,
                0,
            )?);
            writes.extend(fixed(&[(
                eix_sites::AI_VIEWER_SEE__6D,
                &[0xCE, 0x8B, 0x16],
                &[0x8B, 0x16, 0x90],
            )])?);
        }
        _ => {}
    }
    Ok(writes)
}

#[cfg(test)]
mod tests {
    use super::super::testing::{no_log, plain, values};
    use super::*;

    #[test]
    fn the_hero_preview_scale_becomes_perssize() {
        let v = values(&[("gameplay", "perssize", "1.5")]);
        let ctx = Ctx::new(&v, no_log);
        assert_eq!(
            plain(&pers_size(&ctx, 1).unwrap()),
            [(
                eix_sites::C_UI_WINDOW_DRAW_HERO_PREVIEW__5F3,
                vec![0x9A, 0x99, 0x99, 0x3F],
                1.5f32.to_le_bytes().to_vec()
            )]
        );
        assert!(pers_size(&ctx, 2).unwrap().is_empty());
        // The built-in default is the game's own value.
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        let w = plain(&pers_size(&ctx, 1).unwrap());
        assert_eq!(w[0].1, w[0].2);
    }

    #[test]
    fn switches_take_only_their_tested_values() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        let on_one: [Build; 15] = [
            info_window_changes,
            show_type,
            show_mana_for_everyone,
            dont_merge_gipat_zones,
            draw_visible_patch,
            no_trash,
            disable_dragon,
            no_ruins_zone_animation,
            big_maps_patch,
            new_counters,
            no_game_over_on_leaders_death,
            info_window_remake,
            ultra_fixes,
            new_allods_acks,
            allods_info,
        ];
        for b in on_one {
            assert!(!b(&ctx, 1).unwrap().is_empty());
            assert!(b(&ctx, 2).unwrap().is_empty());
            assert!(b(&ctx, -1).unwrap().is_empty());
        }
        for b in [hidden_zones_exits, change_font] {
            assert!(!b(&ctx, 3).unwrap().is_empty());
            assert!(b(&ctx, 0).unwrap().is_empty());
        }
        assert!(main_menu_weather(&ctx, 3).unwrap().is_empty());
        assert!(zone_specific_music(&ctx, 4).unwrap().is_empty());
        assert!(experimental_fixes(&ctx, 9).unwrap().is_empty());
        assert!(experimental_fixes(&ctx, 11).unwrap().is_empty());
    }

    #[test]
    fn parameters_land_in_their_operands() {
        let v = values(&[
            ("gameplay", "meter", "17.5"),
            ("gameplay", "newlightsvalue", "0.25"),
            ("gameplay", "exred", "1"),
            ("gameplay", "exgreen", "2"),
            ("gameplay", "exblue", "3.9"),
            ("gameplay", "minmusictime", "100000"),
            ("gameplay", "maxmusictime", "250000"),
            ("gameplay", "maxsegments", "1234"),
            ("gameplay", "addsegments", "700"),
            ("gameplay", "exitenteractivityradius", "175.0"),
        ]);
        let ctx = Ctx::new(&v, no_log);
        assert_eq!(
            plain(&meter_scale(&ctx, 1).unwrap())[0].2,
            [0, 0, 0x8C, 0x41]
        );
        assert_eq!(
            plain(&lights_boost(&ctx, 1).unwrap())[0].2,
            [0, 0, 0x80, 0x3E]
        );
        // Fractional colour components are kept.
        let w = plain(&explored_colors(&ctx, 1).unwrap());
        let got: Vec<f32> = w
            .iter()
            .map(|s| f32::from_le_bytes([s.2[0], s.2[1], s.2[2], s.2[3]]))
            .collect();
        assert_eq!(got, [1.0, 2.0, 3.9]);
        // As the original: 150000 (0x249f0) and 100000 (0x186a0) at each draw.
        let w = plain(&music_timings(&ctx, 1).unwrap());
        assert_eq!(
            w[0],
            (
                eix_sites::C_MUSIC_SYSTEM_UPDATE_MUSIC__B1,
                vec![0x61, 0xEA, 0, 0],
                vec![0xF0, 0x49, 0x02, 0]
            )
        );
        assert_eq!(
            w[5],
            (
                eix_sites::C_MUSIC_SYSTEM_UPDATE_MUSIC__24A,
                vec![0xC0, 0xD4, 0x01, 0],
                vec![0xA0, 0x86, 0x01, 0]
            )
        );
        let w = plain(&segments_on_exit_enter(&ctx, 1).unwrap());
        assert_eq!(w[0].2, [0xD2, 0x04, 0, 0]);
        assert_eq!(w[3].2, [0x90, 0x05, 0xBC, 0x02, 0x00, 0x00]);
        assert_eq!(
            w[4],
            (
                eix_sites::ENTER_ZONE__507,
                vec![0, 0, 0x8C, 0x42],
                vec![0, 0, 0x2F, 0x43]
            )
        );
    }

    #[test]
    fn allods_info_writes_floats_then_integers() {
        // As the original, with framex 500.7 and framey 300.2.
        let v = values(&[
            ("gameplay", "framex", "500.7"),
            ("gameplay", "framey", "300.2"),
        ]);
        let ctx = Ctx::new(&v, no_log);
        let w = plain(&allods_info(&ctx, 1).unwrap());
        let at = |site: Site| w.iter().find(|s| s.0 == site).unwrap().2.clone();
        assert_eq!(
            at(eix_sites::C_GLOBAL_MAP_CREATE__135E),
            [0x9A, 0x99, 0x93, 0x43]
        );
        assert_eq!(
            at(eix_sites::C_GLOBAL_MAP_CREATE__1363),
            [0x9A, 0xD9, 0xF7, 0x43]
        );
        assert_eq!(
            at(eix_sites::C_GLOBAL_MAP_CREATE__13BC),
            [0x9A, 0x19, 0x96, 0x43]
        );
        assert_eq!(
            at(eix_sites::C_GLOBAL_MAP_CREATE__13C1),
            [0x9A, 0x59, 0xFA, 0x43]
        );
        assert_eq!(at(eix_sites::C_GLOBAL_MAP_CREATE__1529), [0xE0, 0x01, 0, 0]);
        assert_eq!(at(eix_sites::C_GLOBAL_MAP_CREATE__1534), [0xFF, 0, 0, 0]);
        assert_eq!(at(eix_sites::C_GLOBAL_MAP_CREATE__1565), [0xFF, 0, 0, 0]);
        assert_eq!(at(eix_sites::C_GLOBAL_MAP_CREATE__1570), [0xE0, 0x01, 0, 0]);
        assert_eq!(at(eix_sites::C_GLOBAL_MAP_CREATE__157B), [0x13, 0x01, 0, 0]);
        // The window's site keeps the original's two NOPs before the jump.
        assert_eq!(&at(ALLOD_WINDOW)[..], &[0x90, 0x90]);
        assert_eq!(at(ALLOD_WINDOW.offset(2))[0], 0xE9);
    }

    #[test]
    fn the_info_lines_are_the_original_rewrite() {
        // The original's bytes, with its format at 0x72e2e9.
        let want: [u8; 164] = {
            let mut b = [0x90; 164];
            let code: &[u8] = &[
                0x10, 0x50, 0x8B, 0x45, 0x90, 0x50, 0x8B, 0x42, 0x0C, 0x50, 0x68, 0xE9, 0xE2, 0x72,
                0x00, 0x51, 0xE8, 0x60, 0x6C, 0x0B, 0x00, 0x8B, 0x4D, 0xEC, 0x83, 0xC4, 0x18, 0x8D,
                0x45, 0xC8, 0x6A, 0x01, 0x6A, 0x00, 0x68, 0xE4, 0xD7, 0xA7, 0x00, 0x6A, 0x00, 0x50,
                0x51, 0x8B, 0xCF, 0xE8, 0x04, 0x6B, 0xFB, 0xFF, 0x6A, 0x0F, 0x8D, 0x55, 0xC8, 0x6A,
                0x00, 0x52, 0xFF, 0xD3, 0x8B, 0x85, 0x7C, 0xFF, 0xFF, 0xFF, 0x83, 0xF8, 0x07, 0x73,
                0x22, 0x50, 0x8D, 0x45, 0xEC, 0x68, 0x70, 0x2F, 0x79, 0x00, 0x50, 0xE8, 0x1F, 0x6C,
                0x0B, 0x00, 0x8B, 0x55, 0xEC, 0x83, 0xC4, 0x0C, 0x8D, 0x4D, 0xE8, 0x51, 0x8B, 0x4D,
                0xE0, 0x52, 0xE8, 0xCD, 0x8C, 0xE9, 0xFF,
            ];
            b[..code.len()].copy_from_slice(code);
            b
        };
        assert_eq!(info_lines(eix_sites::TEXT_72E2E9.ru32()), want);
    }

    #[test]
    fn music_pauses_never_divide_by_zero() {
        let modulus = |min: &str, max: &str| {
            let v = values(&[
                ("gameplay", "minmusictime", min),
                ("gameplay", "maxmusictime", max),
            ]);
            let w = plain(&music_timings(&Ctx::new(&v, no_log), 1).unwrap());
            (w[0].0, w[0].2.clone())
        };
        let site = eix_sites::C_MUSIC_SYSTEM_UPDATE_MUSIC__B1;
        assert_eq!(
            modulus("1000", "3000"),
            (site, 2000i32.to_le_bytes().to_vec())
        );
        assert_eq!(modulus("1000", "1000"), (site, 1i32.to_le_bytes().to_vec()));
        assert_eq!(modulus("3000", "1000"), (site, 1i32.to_le_bytes().to_vec()));
    }

    #[test]
    fn fonts_fill_their_slots() {
        let v = values(&[("fonts", "newfont1", "Arial")]);
        let ctx = Ctx::new(&v, no_log);
        let w = plain(&change_font(&ctx, 1).unwrap());
        assert_eq!(w.len(), 8);
        assert_eq!(
            (w[0].0, &w[0].2[..6], w[0].2[0x2F]),
            (eix_sites::DATA_79A290, &b"Arial\0"[..], 0)
        );
        assert_eq!(
            w[1],
            (
                eix_sites::C_INTERFACE_3D_CREATE__FD,
                TIMES_PUSH.to_vec(),
                vec![0x68, 0x90, 0xA2, 0x79, 0x00]
            )
        );
        assert_eq!(w[7].2, [0x68, 0x20, 0xA3, 0x79, 0x00]);
    }

    #[test]
    fn experimental_fixes_match_the_original_values() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        let sites = |value| -> Vec<Site> {
            let mut s: Vec<Site> = plain(&experimental_fixes(&ctx, value).unwrap())
                .iter()
                .map(|w| w.0)
                .collect();
            s.sort_unstable();
            s
        };
        assert_eq!(
            sites(1),
            [
                eix_sites::UNIT_ORDER_LESS_LESS__8,
                eix_sites::UNIT_ORDER_LESS_LESS__18
            ]
        );
        assert_eq!(
            sites(2),
            [
                eix_sites::C_WORLD_SERVER_LOAD_SAVE__8B9,
                eix_sites::C_WORLD_SERVER_LOAD_SAVE__B7E
            ]
        );
        assert_eq!(
            sites(4),
            [
                eix_sites::UNIT_ORDER_LESS_LESS__8,
                eix_sites::UNIT_ORDER_LESS_LESS__18,
                eix_sites::C_WORLD_SERVER_LOAD_SAVE__8AD,
                eix_sites::C_WORLD_SERVER_LOAD_SAVE__B7E
            ]
        );
        assert_eq!(
            sites(6),
            [
                eix_sites::C_WORLD_SERVER_LOAD_SAVE__8AD,
                eix_sites::C_WORLD_SERVER_LOAD_SAVE__B68,
                eix_sites::C_WORLD_SERVER_LOAD_SAVE__BD7
            ]
        );
        assert_eq!(sites(7), [eix_sites::C_WORLD_SERVER_LOAD_SAVE__8C1]);
        assert_eq!(sites(10).len(), 15);
        let w = plain(&experimental_fixes(&ctx, 7).unwrap());
        assert_eq!((w[0].2[0], &w[0].2[5..]), (0xE9, &[0x90, 0x90][..]));
    }
}
