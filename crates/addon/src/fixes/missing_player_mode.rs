//! MissingPlayerMode: a party member without a Player mode does not count in `PlayerMode::StaysWithParty` 0x5D4860,
//! where the game read through null. For each other unit of the party it looks for the unit's mode of type 6 (the
//! Player mode) to test whether that unit follows this leader; with no such mode (a script's `UMClear` or `Idle` and
//! an AI tick before the next mode) the search leaves ESI at 0 (0x5D493A) and `cmp [esi+0xc], ebp` faults. Ours
//! treats that unit as not following the leader, as the game does a unit whose Player mode follows another: the walk
//! goes on with the next unit (0x5D4988).

use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// `test ecx, ecx; mov [esp+0x40], ecx; je 0x5D493A`: ECX the first node of the unit's mode list.
const LIST_START: Site = eix_sites::PLAYER_MODE_STAYS_WITH_PARTY__AD;
const LIST_START_BYTES: [u8; 8] = [0x85, 0xC9, 0x89, 0x4C, 0x24, 0x40, 0x74, 0x25];
/// The search's body, from the list's first node on.
const SEARCH: Site = eix_sites::PLAYER_MODE_STAYS_WITH_PARTY__B5;
/// `mov eax, [esp+0x40]; test eax, eax; jne 0x5D4918`: the next node after a mode of another type.
const LIST_NEXT: Site = eix_sites::PLAYER_MODE_STAYS_WITH_PARTY__D2;
const LIST_NEXT_BYTES: [u8; 8] = [0x8B, 0x44, 0x24, 0x40, 0x85, 0xC0, 0x75, 0xDE];
const SEARCH_NEXT: Site = eix_sites::PLAYER_MODE_STAYS_WITH_PARTY__B8;
/// The step to the party's next unit, where a unit following another leader goes.
const NEXT_UNIT: Site = eix_sites::PLAYER_MODE_STAYS_WITH_PARTY__128;
/// The search's position, a local at `[esp+0x40]` at both sites.
const POSITION: usize = 0x40;

/// The first node, stored as the game's `mov` stores it; an empty list goes to the next unit.
extern "C" fn list_start(_: usize, ctx: &mut Context) {
    ctx.set_stack_at(ctx.stack() + POSITION, ctx.ecx);
    ctx.resume = if ctx.ecx == 0 { NEXT_UNIT } else { SEARCH };
}

/// The next node; past the list's end, the next unit.
extern "C" fn list_next(_: usize, ctx: &mut Context) {
    ctx.eax = ctx.stack_at(ctx.stack() + POSITION).unwrap_or(0);
    ctx.resume = if ctx.eax == 0 { NEXT_UNIT } else { SEARCH_NEXT };
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![
        Labelled {
            label: "MissingPlayerMode #0".to_owned(),
            write: hook::context_hook(LIST_START, &LIST_START_BYTES, Mode::Drop, list_start, 0)?,
        },
        Labelled {
            label: "MissingPlayerMode #1".to_owned(),
            write: hook::context_hook(LIST_NEXT, &LIST_NEXT_BYTES, Mode::Drop, list_next, 0)?,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sites_branch_where_the_replaced_jumps_went() {
        // `je 0x5D493A` (+0x25 after the site's end) and `jne 0x5D4918` (-0x22).
        assert_eq!(LIST_START.offset(LIST_START_BYTES.len()), SEARCH);
        assert_eq!(
            LIST_NEXT.offset(LIST_NEXT_BYTES.len()).back(0x22),
            SEARCH_NEXT
        );
    }
}
