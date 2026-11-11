//! PerkLimit: the trader's skill list stops each skill at its `[AddonPerk]` limit instead of 100. Each of the three
//! sites compares a level with 100 before a `jge`; the hook makes the compare against the skill's limit and leaves
//! its flags for the game's branch.
#![expect(
    unsafe_code,
    reason = "reading the trader panel's skill levels at the hooked sites"
)]

use std::sync::atomic::{AtomicI32, Ordering};

use eix_core::rules::perk_limit_slot;
use eix_core::x86_flags;
use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use super::memory::word;
use super::{BuildError, Labelled, Setup};

/// Melee, Archery, Elemental, Sense and Astral, as [`perk_limit_slot`] numbers them.
static LIMITS: [AtomicI32; 5] = [const { AtomicI32::new(100) }; 5];

/// The trader's skill rows are regions 4..=9 at sites #2 and #3; the skill is the region minus 4.
const FIRST_REGION: u32 = 4;

/// The flags of `cmp level, limit` for `skill`, or, without a limit (Backstab), those of the hook's last index test
/// `cmp index, last`, which reads "below": the game's `jge` then never stops the skill.
fn flags(eflags: u32, level: u32, skill: Option<usize>, index: u32, last: u32) -> u32 {
    match skill {
        Some(s) => x86_flags::cmp(eflags, level, LIMITS[s].load(Ordering::Relaxed) as u32),
        None => x86_flags::cmp(eflags, index, last),
    }
}

/// #1, `TradeDrawLists` 0x6263BA: `cmp dword [eax+ebp+0x10], 100`, the skill index (0..=5) in EBX.
extern "C" fn draw_lists(_: usize, ctx: &mut Context) {
    // SAFETY: at 0x6263BA, EAX + EBP + 0x10 addresses the row's level, which the replaced compare reads.
    let level = unsafe { word(ctx.eax.wrapping_add(ctx.ebp).wrapping_add(0x10) as usize) };
    ctx.eflags = flags(ctx.eflags, level, perk_limit_slot(ctx.ebx), ctx.ebx, 5);
}

/// #2, 0x626A1E (buying a level): `mov edx, [edx+eax+0x10]; cmp edx, 100`, the region (4..=9) in EBX.
extern "C" fn buy_level(_: usize, ctx: &mut Context) {
    // SAFETY: at 0x626A1E, EDX + EAX + 0x10 addresses the row's level, which the replaced load reads.
    ctx.edx = unsafe { word(ctx.edx.wrapping_add(ctx.eax).wrapping_add(0x10) as usize) };
    let skill = perk_limit_slot(ctx.ebx.wrapping_sub(FIRST_REGION));
    ctx.eflags = flags(ctx.eflags, ctx.edx, skill, ctx.ebx, 9);
}

/// #3, `CTradePanel::UpdateHeld` 0x626CB3: `add edx, ecx; cmp dword [edx+0x10], 100`, the region in EDI.
extern "C" fn update_held(_: usize, ctx: &mut Context) {
    ctx.edx = ctx.edx.wrapping_add(ctx.ecx);
    // SAFETY: at 0x626CB3, EDX + ECX + 0x10 addresses the row's level, which the replaced compare reads.
    let level = unsafe { word(ctx.edx.wrapping_add(0x10) as usize) };
    let skill = perk_limit_slot(ctx.edi.wrapping_sub(FIRST_REGION));
    ctx.eflags = flags(ctx.eflags, level, skill, ctx.edi, 9);
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let p = &setup.options.ai.perk;
    for (slot, limit) in LIMITS
        .iter()
        .zip([p.melee, p.archery, p.elemental, p.sense, p.astral])
    {
        slot.store(limit, Ordering::Relaxed);
    }
    let sites: [(&str, Site, &[u8], hook::Handler); 3] = [
        (
            "Perk Limit #1",
            eix_sites::TRADE_DRAW_LISTS__21A,
            &[0x83, 0x7C, 0x28, 0x10, 0x64],
            draw_lists,
        ),
        (
            "Perk Limit #2",
            eix_sites::TEXT_626A1E,
            &[0x8B, 0x54, 0x02, 0x10, 0x83, 0xFA, 0x64],
            buy_level,
        ),
        (
            "Perk Limit #3",
            eix_sites::C_TRADE_PANEL_UPDATE_HELD__63,
            &[0x03, 0xD1, 0x83, 0x7A, 0x10, 0x64],
            update_held,
        ),
    ];
    sites
        .into_iter()
        .map(|(label, site, original, handler)| {
            Ok(Labelled {
                label: label.to_owned(),
                write: hook::context_call(site, original, Mode::Drop, handler, 0)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `jge`: SF equals OF.
    fn ge(f: u32) -> bool {
        (f & 0x80 != 0) == (f & 0x800 != 0)
    }

    #[test]
    fn a_limited_skill_stops_at_its_limit_and_backstab_never_does() {
        LIMITS[0].store(50, Ordering::Relaxed);
        assert!(!ge(flags(0, 49, Some(0), 0, 5)));
        assert!(ge(flags(0, 50, Some(0), 0, 5)));
        // Backstab, index 2 (#1) or region 6 (#2, #3): "below" whatever its level.
        assert!(!ge(flags(0, 1000, None, 2, 5)));
        assert!(!ge(flags(0, 1000, None, 6, 9)));
    }
}
