//! QuestSort: the journal's quest sort stays inside its array when quests have no record.
//! `CObjectivesWindow::ShowZone` 0x64CC10 sorts the quest names (introsort, 0x64F310) with a comparator inlined in
//! each sort routine:
//! a quest without a record (`QuestTable` lookup 0x4CE110 / 0x64EE90 on 0x7C87B4) is less than any quest, itself and
//! other record-less ones included; between two with records the larger key (+0x1C) is less. With a record-less pivot,
//! the partition's unguarded scans (`__unguarded_partition` 0x64F650) never meet a quest that stops them and run past
//! the array (a fault at 0x40C5E9 with twelve such quests). Ours makes two record-less quests equal in the
//! partition's two copies of the comparator, so each scan stops at the pivot's own copy at the latest; record-less
//! quests still come first. The insertion and heap routines keep their copies: their loops are bounded by the array.

use eix_core::x86_flags;
use eix_engine::hook::{self, Context, Mode};

use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// Left scan, `while comp(*first, pivot)`: `cmp esi, ebx; je less; cmp edi, ebx` with ESI the record of `*first`,
/// EDI the pivot's, EBX zero; then `je not_less` (0x64F749) and the key test.
const LEFT: usize = eix_sites::TEXT_64F743;
const LEFT_BYTES: [u8; 6] = [0x3B, 0xF3, 0x74, 0x13, 0x3B, 0xFB];
const LEFT_NULL_TEST: usize = eix_sites::TEXT_64F749;
const LEFT_LESS: usize = eix_sites::TEXT_64F75A;
const LEFT_NOT_LESS: usize = eix_sites::TEXT_64F762;
/// Right scan, `while comp(pivot, *--last)`: `cmp esi, ebx; mov [esp+0x7c], bl` with ESI the pivot's record, EDI
/// that of `*last`; then `je less` (0x64F7E9), and at 0x64F7EF the test of EDI.
const RIGHT: usize = eix_sites::TEXT_64F7E3;
const RIGHT_BYTES: [u8; 6] = [0x3B, 0xF3, 0x88, 0x5C, 0x24, 0x7C];
const RIGHT_LESS: usize = eix_sites::TEXT_64F762;
const RIGHT_NOT_LESS: usize = eix_sites::TEXT_64F806;

/// The left scan's comparator: ESI's quest against EDI's.
extern "C" fn left(_: usize, ctx: &mut Context) {
    if ctx.esi == ctx.ebx {
        ctx.resume = if ctx.edi == ctx.ebx {
            LEFT_NOT_LESS
        } else {
            LEFT_LESS
        };
    } else {
        // The dropped `cmp edi, ebx`, for the game's own null test and key comparison that follow.
        ctx.eflags = x86_flags::cmp(ctx.eflags, ctx.edi, ctx.ebx);
        ctx.resume = LEFT_NULL_TEST;
    }
}

/// The right scan's comparator: the pivot's quest (ESI) against EDI's. A pivot with a record goes the game's way.
extern "C" fn right(_: usize, ctx: &mut Context) {
    if ctx.esi == ctx.ebx {
        ctx.resume = if ctx.edi == ctx.ebx {
            RIGHT_NOT_LESS
        } else {
            RIGHT_LESS
        };
    }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![
        Labelled {
            label: "QuestSort #0".to_owned(),
            write: hook::context_hook(LEFT, &LEFT_BYTES, Mode::Drop, left, 0)?,
        },
        Labelled {
            label: "QuestSort #1".to_owned(),
            write: hook::context_hook(RIGHT, &RIGHT_BYTES, Mode::Before, right, 0)?,
        },
    ])
}
