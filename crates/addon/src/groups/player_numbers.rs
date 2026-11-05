//! PlayerNumbers: a new session player (`CPlayerManager::CreateSessionPlayer` 0x66C180, its call of
//! `Player::SetPlayerId(0)` at 0x66C1C2) gets the highest number among the server world's other players plus one,
//! or the asked number when the world has no player list or no other player numbered 0 or more.
#![expect(
    unsafe_code,
    reason = "walking the server world's player list and setting the new player's number"
)]

use eix_engine::game::player::{Player, PlayerIterator};
use eix_engine::game::world::{SERVER_WORLD, World};
use eix_engine::hook;

use super::{BuildError, Labelled, Setup};

/// The new player's number, given the numbers of the list's other players; the original's `+ 1` wraps.
fn next_number(asked: i32, others: impl IntoIterator<Item = i32>) -> i32 {
    let highest = others.into_iter().fold(-1, i32::max);
    if highest >= 0 {
        highest.wrapping_add(1)
    } else {
        asked
    }
}

/// Stands in for `Player::SetPlayerId` (thiscall, `ret 4`) at the one call site.
extern "thiscall" fn set_player_id(player: *mut Player, asked: i32) {
    // SAFETY: the build is RU 1.06 and the call comes from the game's thread; the server world is static.
    let list = unsafe { World::players(SERVER_WORLD.as_ptr()) };
    let number = if list.is_null() {
        asked
    } else {
        let mut numbers = Vec::new();
        let mut it = PlayerIterator::new();
        // SAFETY: as above; `list` is the server world's live player list.
        let mut p = unsafe { it.first(list) };
        while !p.is_null() {
            if p != player {
                // SAFETY: the list yields live players.
                numbers.push(unsafe { (*p).number });
            }
            // SAFETY: the walk was started by `first`.
            p = unsafe { it.next() };
        }
        // SAFETY: as above.
        unsafe { it.release() };
        next_number(asked, numbers)
    };
    // SAFETY: the game passes the player it is creating, a live object.
    unsafe { (*player).number = number };
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let (write, _) = hook::redirect_call(
        eix_sites::C_PLAYER_MANAGER_CREATE_SESSION_PLAYER__42,
        [0xE8, 0x99, 0x98, 0xFF, 0xFF],
        set_player_id as *const () as usize,
    )?;
    Ok(vec![Labelled {
        label: "PlayerNumbers".to_owned(),
        write,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_follow_the_highest_other() {
        assert_eq!(next_number(0, []), 0);
        assert_eq!(next_number(0, [-1, -1]), 0);
        assert_eq!(next_number(0, [0]), 1);
        assert_eq!(next_number(0, [3, 1, -1]), 4);
    }
}
