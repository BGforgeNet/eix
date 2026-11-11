//! FixMoveObject: `CSectorMap::Move` 0x5C11D0 refuses a position with a negative or NaN x or y (core's
//! [`move_allowed`]), where the game computed a negative sector; a refused or failed move logs a warning.
#![expect(
    unsafe_code,
    reason = "reading the object and position the game passes, and calling the original Move"
)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use eix_core::fixes::move_allowed;
use eix_engine::game::object::Vector3;
use eix_engine::game::unit::MapObject;
use eix_engine::hook;
use eix_sites::Site;

use super::{BuildError, Labelled, Setup};
use crate::log;

/// `push ebp; mov ebp, esp; sub esp, 0x20`.
const SITE: Site = eix_sites::C_SECTOR_MAP_MOVE;
const ORIGINAL: [u8; 6] = [0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x20];

/// `bool __thiscall CSectorMap::Move(CMapObject*, const Vector3*)`, `ret 8`.
type Move = unsafe extern "thiscall" fn(*mut c_void, *mut MapObject, *const Vector3) -> bool;

/// The trampoline that runs the original.
static ORIGINAL_MOVE: AtomicUsize = AtomicUsize::new(0);

extern "thiscall" fn sector_map_move(
    this: *mut c_void,
    object: *mut MapObject,
    to: *const Vector3,
) -> bool {
    // SAFETY: the game passes the position the object moves to, on its thread.
    let at = unsafe { *to };
    if move_allowed(at.x, at.y) {
        // SAFETY: the trampoline runs the original, which has this type.
        let original: Move = unsafe { hook::as_fn_at(ORIGINAL_MOVE.load(Ordering::SeqCst)) };
        // SAFETY: the game's arguments, passed on.
        if unsafe { original(this, object, to) } {
            return true;
        }
    }
    // The message reads the position and the id after the move, as the original's does.
    // SAFETY: as above.
    let at = unsafe { *to };
    // SAFETY: the game passes a live object.
    let id = unsafe { (*object).transform.base.id };
    let text = format!(
        "Failed to move object {id} to ({:.1}, {:.1}, {:.1})",
        at.x, at.y, at.z
    );
    // The original's console logger drops warnings at its default level, so the player sees nothing; addon.log
    // keeps the line.
    log::warn("fixes", &text, "");
    false
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let (write, trampoline) =
        hook::replace_function(SITE, &ORIGINAL, sector_map_move as *const () as usize)?;
    ORIGINAL_MOVE.store(trampoline, Ordering::SeqCst);
    Ok(vec![Labelled {
        label: "FixMoveObject #0".to_owned(),
        write,
    }])
}
