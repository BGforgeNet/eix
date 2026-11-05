//! ImproveInfoWindows: the spell lines of four info windows show the damage with one decimal and the cast time in
//! seconds, and the unit panel's rate line adds its percentage. Each hook runs right after the game formatted the
//! line and formats it again into the game's own string.
#![expect(
    unsafe_code,
    reason = "reading the windows' spells and strings and formatting the game's strings at the hooked sites"
)]

use std::ffi::CStr;

use eix_engine::game::mfc::{self, FormatArg, GameString};
use eix_engine::game::spell::{Spell, SpellRecord};
use eix_engine::hook::{self, Context, Mode};
use eix_engine::x87::Ext;

use super::memory::word;
use super::{BuildError, Labelled, Setup};

/// Which line a hook rewrites.
const DAMAGE: usize = 0;
const CAST_TIME: usize = 1;

/// The text of entry `offset` of the string table the window keeps at `window + table`: a `CString`'s characters.
///
/// # Safety
/// `window` must be the hooked code's window, which keeps its table pointer at `table`.
unsafe fn label(window: u32, table: u32, offset: u32) -> FormatArg {
    // SAFETY: the caller's guarantee: the window's field.
    let entries = unsafe { word(window.wrapping_add(table) as usize) };
    // SAFETY: as above; each entry is a CString, whose one word is its characters.
    let text = unsafe { word(entries.wrapping_add(offset) as usize) };
    FormatArg::Str(core::ptr::with_exposed_provenance(text as usize))
}

/// A cast time in seconds: ticks (unsigned) / 15.
fn seconds(ticks: u32) -> FormatArg {
    FormatArg::Double(Ext::from_u32(ticks).fdiv(15.0).to_f64())
}

/// Formats into the game's string at `string`.
///
/// # Safety
/// `string` must be a constructed game string and `args` what `format` asks for.
unsafe fn reformat(string: u32, format: &CStr, args: &[FormatArg]) -> *mut GameString {
    let string = core::ptr::with_exposed_provenance_mut::<GameString>(string as usize);
    // SAFETY: the caller's guarantee, on the game's thread.
    unsafe { mfc::format(string, format, args) };
    string
}

/// What one window's spell lines are made of.
struct Window {
    /// Where the window keeps its string table pointer.
    table: u32,
    /// The table entries: the damage label, the cast-time label, the seconds suffix.
    damage_label: u32,
    cast_label: u32,
    suffix: u32,
    /// The formats, with the spacing each window's own lines have.
    damage_format: &'static CStr,
    cast_format: &'static CStr,
}

/// `CItemInfoPanel`: the item and spell-record info windows (#0-#3); the game draws from the line's characters.
const INFO_PANEL: Window = Window {
    table: 0x1CC,
    damage_label: 0x20,
    cast_label: 0x24,
    suffix: 0x6C,
    damage_format: c"%s %.1f",
    cast_format: c"%s %.1f%s",
};

/// `CHudQuickItems::ItemTip` (#4, #5).
const ITEM_TIP: Window = Window {
    table: 0x190,
    damage_label: 0x4,
    cast_label: 0x8,
    suffix: 0x28,
    damage_format: c"%s %.1f  ",
    cast_format: c"%s %.1f%s \n",
};

/// `CHudSpellColumn::SetUnit` (#6, #7).
const SPELL_COLUMN: Window = Window {
    table: 0x15C,
    damage_label: 0x8,
    cast_label: 0xC,
    suffix: 0x2C,
    damage_format: c"%s %.1f  ",
    cast_format: c"%s %.1f%s \n",
};

/// What a spell's lines show.
#[derive(Debug, Clone, Copy)]
struct Shown {
    /// Whether the damage is a number (else the game's text stays).
    numeric_damage: bool,
    damage: f32,
    cast_time: u32,
}

/// Line `line` of `shown` into the game's string at `string`; `None` when the game's own line stays.
///
/// # Safety
/// `window` and `string` must be the hooked code's window and the string it just formatted the line into.
unsafe fn spell_line(
    w: &Window,
    line: usize,
    window: u32,
    string: u32,
    shown: Shown,
) -> Option<*mut GameString> {
    if line == DAMAGE {
        if !shown.numeric_damage {
            return None;
        }
        // SAFETY: the caller's guarantee.
        let text = unsafe { label(window, w.table, w.damage_label) };
        let args = [text, FormatArg::Double(shown.damage.into())];
        // SAFETY: as above.
        return Some(unsafe { reformat(string, w.damage_format, &args) });
    }
    // SAFETY: the caller's guarantee.
    let text = unsafe { label(window, w.table, w.cast_label) };
    // SAFETY: as above.
    let suffix = unsafe { label(window, w.table, w.suffix) };
    let args = [text, seconds(shown.cast_time), suffix];
    // SAFETY: as above.
    Some(unsafe { reformat(string, w.cast_format, &args) })
}

/// What the spell at `address` shows: its damage and cast time, and its record's damage flag.
///
/// # Safety
/// `address` must be a live spell.
unsafe fn spell(address: u32) -> Shown {
    let spell = core::ptr::with_exposed_provenance_mut::<Spell>(address as usize);
    // SAFETY: the caller's guarantee, on the game's thread.
    let record = unsafe { Spell::record(spell) };
    // SAFETY: as above.
    let s = unsafe { &*spell };
    // SAFETY: as above; the record is the spell's database entry.
    let flag = unsafe { (*record).shows_damage };
    Shown {
        numeric_damage: flag != 0,
        damage: s.damage,
        cast_time: s.cast_time,
    }
}

/// The string's characters, for a register the game draws from.
///
/// # Safety
/// `s` must be a constructed game string.
unsafe fn chars(s: *mut GameString) -> u32 {
    // SAFETY: the caller's guarantee.
    unsafe { (*s).chars.expose_provenance() as u32 }
}

/// #0, #1, `CItemInfoPanel::DescribeCarriedSpell`: the window in EBX, the spell in EBP, the line's string at the
/// site's ESP+0x10, whose characters the game passes in EDX to the draw that follows.
extern "C" fn carried_spell(line: usize, ctx: &mut Context) {
    let string = (ctx.stack() + 0x10) as u32;
    // SAFETY: at these sites EBP is the described spell.
    let shown = unsafe { spell(ctx.ebp) };
    // SAFETY: EBX is the panel, and the string the line the game just formatted.
    let new = unsafe { spell_line(&INFO_PANEL, line, ctx.ebx, string, shown) };
    if let Some(s) = new {
        // SAFETY: the game's string, just formatted.
        ctx.edx = unsafe { chars(s) };
    }
}

/// #2, #3, `CItemInfoPanel::DescribeSpellRecord`: the window in EBP, the spell record in EBX, the string as above.
extern "C" fn spell_record(line: usize, ctx: &mut Context) {
    let string = (ctx.stack() + 0x10) as u32;
    let record = core::ptr::with_exposed_provenance::<SpellRecord>(ctx.ebx as usize);
    // SAFETY: at these sites EBX is the described record.
    let record = unsafe { &*record };
    let shown = Shown {
        numeric_damage: record.shows_damage != 0,
        damage: record.damage,
        cast_time: record.cast_time,
    };
    // SAFETY: EBP is the panel, and the string the line the game just formatted.
    let new = unsafe { spell_line(&INFO_PANEL, line, ctx.ebp, string, shown) };
    if let Some(s) = new {
        // SAFETY: the game's string, just formatted.
        ctx.edx = unsafe { chars(s) };
    }
}

/// #4, #5, `CHudQuickItems::ItemTip`: the window in EDI, the spell in ESI, the line's string at ESP+0x28, which the
/// game appends next.
extern "C" fn item_tip(line: usize, ctx: &mut Context) {
    let string = (ctx.stack() + 0x28) as u32;
    // SAFETY: at these sites ESI is the spell of the tip.
    let shown = unsafe { spell(ctx.esi) };
    // SAFETY: EDI is the window, and the string the line the game just formatted.
    unsafe { spell_line(&ITEM_TIP, line, ctx.edi, string, shown) };
}

/// #6, #7, `CHudSpellColumn::SetUnit`: the window in ESI, the spell in EDI, the line's string at ESP+0x48.
extern "C" fn spell_column(line: usize, ctx: &mut Context) {
    let string = (ctx.stack() + 0x48) as u32;
    // SAFETY: at these sites EDI is the column's spell.
    let shown = unsafe { spell(ctx.edi) };
    // SAFETY: ESI is the window, and the string the line the game just formatted.
    unsafe { spell_line(&SPELL_COLUMN, line, ctx.esi, string, shown) };
}

/// The rate as a percentage of the normal 15: `100 / (15 / v)`, 0 for 0, stored as an `f32`.
fn percent(v: u32) -> f32 {
    if v == 0 {
        0.0
    } else {
        Ext::new(v.into()).fdivr(15.0).fdivr(100.0).to_f32()
    }
}

/// #8, `CHudUnitPanel::DrawUnitInfo`: the rate line, "%s %d (%.0f%%)". The window is ESI, the line's string at
/// EBP-0x14 and the unit's parameters at [EBP-0x1C], whose byte +0x15 is the rate. The game has already loaded the
/// string's old characters into ECX for the draw; ECX gets the new ones, so the new text shows whether or not the
/// format kept the buffer.
extern "C" fn rate_line(_: usize, ctx: &mut Context) {
    let string = ctx.ebp.wrapping_sub(0x14);
    // SAFETY: EBP is DrawUnitInfo's frame at 0x647404; its local at -0x1C points at the unit's parameters.
    let params = unsafe { word(ctx.ebp.wrapping_sub(0x1C) as usize) };
    let rate = core::ptr::with_exposed_provenance::<u8>(params.wrapping_add(0x15) as usize);
    // SAFETY: as above: the byte the game formatted.
    let v = u32::from(unsafe { rate.read() });
    // SAFETY: ESI is the panel, whose table entry +0x3C is the line's label.
    let text = unsafe { label(ctx.esi, 0x1A0, 0x3C) };
    let args = [
        text,
        FormatArg::Int(v as i32),
        FormatArg::Double(percent(v).into()),
    ];
    // SAFETY: the string the game just formatted the line into.
    let s = unsafe { reformat(string, c"%s %d (%.0f%%)", &args) };
    // SAFETY: as above.
    ctx.ecx = unsafe { chars(s) };
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let add_lea = |x: u8, d: u8| [0x83, 0xC4, 0x10, 0x8D, x, 0x24, d];
    let sites: [(usize, [u8; 7], hook::Handler, usize); 8] = [
        (
            eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_CARRIED_SPELL__34F,
            add_lea(0x4C, 0x38),
            carried_spell,
            DAMAGE,
        ),
        (
            eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_CARRIED_SPELL__3C0,
            add_lea(0x4C, 0x38),
            carried_spell,
            CAST_TIME,
        ),
        (
            eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_SPELL_RECORD__2CE,
            add_lea(0x4C, 0x34),
            spell_record,
            DAMAGE,
        ),
        (
            eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_SPELL_RECORD__33F,
            add_lea(0x4C, 0x34),
            spell_record,
            CAST_TIME,
        ),
        (
            eix_sites::C_HUD_QUICK_ITEMS_ITEM_TIP__12A,
            add_lea(0x4C, 0x28),
            item_tip,
            DAMAGE,
        ),
        (
            eix_sites::C_HUD_QUICK_ITEMS_ITEM_TIP__180,
            add_lea(0x54, 0x28),
            item_tip,
            CAST_TIME,
        ),
        (
            eix_sites::C_HUD_SPELL_COLUMN_SET_UNIT__252,
            add_lea(0x4C, 0x48),
            spell_column,
            DAMAGE,
        ),
        (
            eix_sites::C_HUD_SPELL_COLUMN_SET_UNIT__2AA,
            add_lea(0x54, 0x48),
            spell_column,
            CAST_TIME,
        ),
    ];
    let mut out = Vec::with_capacity(9);
    for (n, (site, original, handler, line)) in sites.into_iter().enumerate() {
        out.push(Labelled {
            label: format!("ImproveInfoWindows #{n}"),
            write: hook::context_hook(site, &original, Mode::Before, handler, line)?,
        });
    }
    out.push(Labelled {
        label: "ImproveInfoWindows #8".to_owned(),
        write: hook::context_hook(
            eix_sites::C_HUD_UNIT_PANEL_DRAW_UNIT_INFO__3C4,
            &[0x8B, 0x4D, 0xEC, 0x83, 0xC4, 0x10],
            Mode::Before,
            rate_line,
            0,
        )?,
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_normal_rate_is_100_percent() {
        assert_eq!(percent(15), 100.0);
        assert_eq!(percent(0), 0.0);
        assert_eq!(percent(30), 200.0);
    }
}
