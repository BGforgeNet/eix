//! UnknownScreen: a game state that names no screen is ignored, as state 0 is, where the game took the state's number
//! for a screen object. `CInterface3D::CreateScreen` 0x5F5BA0 dispatches states 0..=9 through its table 0x5F5EFC;
//! entry 6 and every state above 9 (the `ja` at 0x5F5BC3) reach 0x5F5EDD, which passes the number itself to the
//! activation 0x5F71C0, which uses it as a pointer. The states come from the host's game-state message
//! (`CPlayerClient::OnGameState` 0x669D50), so a broken or hostile host could crash a client. Ours sends those two
//! branches to a stub that logs the state and goes to the function's return; 0x5F5EDD itself stays as it is, since the
//! other states' screens join its code at 0x5F5EE1.

use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context};

use super::Warnings;
use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// `ja 0x5F5EDD`: a state above 9.
const ABOVE_NINE: usize = eix_sites::C_INTERFACE_3D_CREATE_SCREEN__23;
const ABOVE_NINE_BYTES: [u8; 6] = [0x0F, 0x87, 0x14, 0x03, 0x00, 0x00];
/// The jump table's entry for state 6.
const ENTRY_SIX: usize = eix_sites::C_INTERFACE_3D_CREATE_SCREEN__374;
/// Where both went: `mov eax, [esp+0x20]`, the state as the screen.
const NUMBER_AS_SCREEN: usize = eix_sites::C_INTERFACE_3D_CREATE_SCREEN__33D;
/// The function's return, where state 0 goes.
const RETURN: usize = eix_sites::C_INTERFACE_3D_CREATE_SCREEN__349;
/// The state's stack word on both branches (`[esp+0x20]`).
const STATE_WORD: usize = 8;
/// A host sending such states keeps sending them.
static WARNINGS: Warnings = Warnings::new("UnknownScreen");

extern "C" fn handler(_: usize, ctx: &mut Context) {
    let state = ctx.stack_word(STATE_WORD).unwrap_or(0);
    WARNINGS.warn(|| format!("game state {state} names no screen: ignored"));
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let stub = hook::stub(0, &[], &[], false, handler, 0, RETURN)?;
    let mut ja = ABOVE_NINE_BYTES;
    let rel = (stub as u32).wrapping_sub((ABOVE_NINE + ABOVE_NINE_BYTES.len()) as u32);
    ja[2..].copy_from_slice(&rel.to_le_bytes());
    Ok(vec![
        Labelled {
            label: "UnknownScreen #0".to_owned(),
            write: SiteWrite::bytes(ABOVE_NINE, &ABOVE_NINE_BYTES, &ja)?,
        },
        Labelled {
            label: "UnknownScreen #1".to_owned(),
            write: SiteWrite::pointer(ENTRY_SIX, NUMBER_AS_SCREEN, stub),
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_branches_went_to_the_number_as_screen() {
        let rel = i32::from_le_bytes(ABOVE_NINE_BYTES[2..].try_into().unwrap());
        let target = (ABOVE_NINE + ABOVE_NINE_BYTES.len()).wrapping_add_signed(rel as isize);
        assert_eq!(target, NUMBER_AS_SCREEN);
        // The table holds 4-byte entries for states 0..=9.
        assert_eq!(
            ENTRY_SIX,
            eix_sites::C_INTERFACE_3D_CREATE_SCREEN__349 + 0x13 + 6 * 4
        );
    }
}
