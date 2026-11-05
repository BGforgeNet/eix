//! PlayerListOnJoin: a joining client's player list shows the players already known when its binding arrives. An
//! update lists its objects in the order the server tracked them, so the players can come before the client's binding;
//! `ClientOnPlayer` 0x41E490 then refreshes the list (`CInterface3D::OnPlayersChanged` 0x5F82D0), but
//! `CChatWindow::ShowPlayers` 0x6091C0 lists nobody while the binding is missing, and `ClientOnBinding` 0x41E430 only
//! stores the binding, so the base screen could stay without faces until the next change of players. Ours refreshes the
//! list at the binding when players came before it, as if it came first.
#![expect(
    unsafe_code,
    reason = "reads the client's player list and calls the game's binding handler and list refresh"
)]

use core::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use eix_engine::hook;

use crate::groups::{BuildError, Labelled};
use crate::log;
use crate::setup::Setup;

/// `mov eax, [ecx+0x25f0]`: the old binding.
const ON_BINDING: usize = eix_sites::CLIENT_ON_BINDING;
const PROLOGUE: [u8; 6] = [0x8B, 0x81, 0xF0, 0x25, 0x00, 0x00];
/// The client's player list, as `ShowPlayers` reads it (0x609232): a pointer to its head node, whose first word is the
/// first node; an empty list's head points at itself.
const PLAYERS: usize = 0x25D8;
/// The interface `ClientOnPlayer` refreshes (0x41E4D8), and the argument it passes for a player added.
const INTERFACE: usize = eix_sites::DATA_7C7D50;
const ADDED: i32 = 1;

type OnBinding = unsafe extern "fastcall" fn(*mut c_void, *mut c_void);
type PlayersChanged = unsafe extern "thiscall" fn(*mut c_void, i32);

static ORIGINAL: AtomicUsize = AtomicUsize::new(0);

/// Whether the player list at `head_slot` holds a player.
///
/// # Safety
/// `head_slot` must be the client's live player list.
unsafe fn has_players(head_slot: *const *const usize) -> bool {
    // SAFETY: the caller guarantees the list.
    let head = unsafe { head_slot.read() };
    // SAFETY: a list's head node is never null.
    let first = unsafe { head.read() };
    first != head.addr()
}

/// `ClientOnBinding`: `this` in ECX, the binding message in EDX, no stack arguments.
extern "fastcall" fn on_binding(client: *mut c_void, binding: *mut c_void) {
    // SAFETY: ORIGINAL holds the trampoline `build` made, of this type; the arguments are the game's.
    let original = unsafe { hook::as_fn::<OnBinding>(ORIGINAL.load(Ordering::Relaxed)) };
    // SAFETY: as above.
    unsafe { original(client, binding) };
    let head_slot = client.wrapping_byte_add(PLAYERS).cast::<*const usize>();
    // SAFETY: `client` is the game's live client session, which holds the list.
    if unsafe { has_players(head_slot) } {
        // SAFETY: the address is `CInterface3D::OnPlayersChanged`, thiscall with one argument (`ret 4`).
        let refresh =
            unsafe { hook::as_fn::<PlayersChanged>(eix_sites::C_INTERFACE_3D_ON_PLAYERS_CHANGED) };
        // SAFETY: the game's interface object, on the thread that handles the client's messages, as `ClientOnPlayer`
        // calls it.
        unsafe { refresh(core::ptr::with_exposed_provenance_mut(INTERFACE), ADDED) };
        log::info(
            "fixes",
            "players came before the binding: player list refreshed",
            "PlayerListOnJoin",
        );
    }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let (write, trampoline) =
        hook::replace_function(ON_BINDING, &PROLOGUE, on_binding as *const () as usize)?;
    ORIGINAL.store(trampoline, Ordering::Relaxed);
    Ok(vec![Labelled {
        label: "PlayerListOnJoin #0".to_owned(),
        write,
    }])
}
