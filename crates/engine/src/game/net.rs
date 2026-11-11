//! The network game: its sessions, as far as the addon's reports reach them, and the delta streams objects write
//! their network state to and read it from (`DeltaField`).
//!
//! Names follow ei-rebuild.
#![expect(
    unsafe_code,
    reason = "calls into game.exe's session and network state code"
)]

use core::ffi::{CStr, c_void};

use eix_sites::Site;

use super::{Global, code};

/// The game client (`g_gameClient`), a counted pointer, null when there is none: `IsMultiplayerClient` 0x57F440
/// reads it.
pub const GAME_CLIENT: Global<*mut c_void> = Global::at(eix_sites::G_GAME_CLIENT);
/// The game server (`g_gameServer`), a counted pointer, null when there is none.
pub const GAME_SERVER: Global<*mut c_void> = Global::at(eix_sites::G_GAME_SERVER);

/// Whether this game is a network client: `IsMultiplayerClient` 0x57F440, true when [`GAME_CLIENT`] is set and bit 31
/// of its +4 word is clear. It reads no register, so the caller's `ecx` does not matter.
///
/// # Safety
/// The process must be a supported game.exe, on the game's thread.
pub unsafe fn is_multiplayer_client() -> bool {
    // SAFETY: 0x57F440 takes no arguments and returns 0 or 1 in eax (plain `ret`).
    let f = unsafe { code::<unsafe extern "C" fn() -> u32>(eix_sites::TEXT_57F440) };
    // SAFETY: the caller guarantees the build and the thread; the function only reads the client global.
    unsafe { f() != 0 }
}

/// The live server session of a hosted network game, or `None`: [`GAME_SERVER`] when it is set and bit 31 of its +4
/// word is clear (a set bit marks a detached session).
///
/// # Safety
/// The process must be a supported game.exe.
pub unsafe fn live_server() -> Option<*mut c_void> {
    // SAFETY: the caller guarantees the build; the global holds the server session or null.
    let server = unsafe { GAME_SERVER.read() };
    // SAFETY: a non-null `g_gameServer` is a session object.
    (!server.is_null() && unsafe { session_word(server) } >> 31 == 0).then_some(server)
}

/// The +4 word of the session at `session`; [`is_multiplayer_client`] ignores a client whose bit 31 is set.
///
/// # Safety
/// `session` must be a live session object of a supported game.exe, on the game's thread.
pub unsafe fn session_word(session: *const c_void) -> u32 {
    let word = session.cast::<u32>().wrapping_add(1);
    // SAFETY: the caller guarantees a live session, whose second word 0x57F440 reads the same way.
    unsafe { word.read() }
}

/// Sends `text` to every player of the game `server` runs: `ServerSession::Announce` 0x41A310 (thiscall, `ret 8`;
/// the second argument 0 as the addon passes it).
///
/// # Safety
/// `server` must be the live [`GAME_SERVER`], on the game's thread.
pub unsafe fn announce(server: *mut c_void, text: &CStr) {
    // SAFETY: 0x41A310 is `void __thiscall ServerSession::Announce(const char*, int)`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void, *const u8, u32)>(
            eix_sites::SERVER_SESSION_ANNOUNCE,
        )
    };
    // SAFETY: the caller guarantees the server and the thread; `text` is NUL-terminated and outlives the call.
    unsafe { f(server, text.as_ptr().cast(), 0) }
}

/// A delta stream; only the game makes one, and its layout stays the game's.
#[derive(Debug)]
pub enum DeltaStream {}

/// The stream functions that take a field and its size: thiscall, `ret 8`.
type FieldCall = unsafe extern "thiscall" fn(*mut DeltaStream, *mut c_void, u32);

/// Calls the field function at `address`.
///
/// # Safety
/// `address` must be one of the functions below, `stream` a live stream of the kind it takes, `field` `size` bytes
/// of live memory, on the game's thread.
unsafe fn field_call(address: Site, stream: *mut DeltaStream, field: *mut c_void, size: u32) {
    // SAFETY: the caller names one of the `FieldCall` functions.
    let f = unsafe { code::<FieldCall>(address) };
    // SAFETY: the caller guarantees the stream, the field and the thread.
    unsafe { f(stream, field, size) }
}

impl DeltaStream {
    /// Writes `size` bytes of `field` into the open field group, as changed since the last write:
    /// `DeltaField::WriteDelta` 0x47DCA0 (`CUnit::WriteNetState` writes the animation entries with it, 0x52EE71).
    ///
    /// # Safety
    /// `this` must be a live writing stream of a supported game.exe with a field group open, `field` `size` bytes of
    /// live memory, on the game's thread.
    pub unsafe fn write_delta(this: *mut Self, field: *mut c_void, size: u32) {
        // SAFETY: passed on to the caller.
        unsafe { field_call(eix_sites::DELTA_FIELD_WRITE_DELTA, this, field, size) }
    }

    /// Closes the open field group: `DeltaField::EndField` 0x47DC20 (thiscall, no arguments, plain `ret`).
    ///
    /// # Safety
    /// `this` must be a live writing stream of a supported game.exe with a field group open, on the game's thread.
    pub unsafe fn end_field(this: *mut Self) -> u32 {
        // SAFETY: 0x47DC20 is a thiscall without stack arguments.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self) -> u32>(eix_sites::DELTA_FIELD_END_FIELD)
        };
        // SAFETY: the caller guarantees the stream and the thread.
        unsafe { f(this) }
    }

    /// Reads `size` bytes into `field` when the writer sent them: `DeltaField::ReadDelta` 0x47E000
    /// (`CUnit::ReadNetState` reads the animation entries with it, 0x52F3A4).
    ///
    /// # Safety
    /// `this` must be a live reading stream of a supported game.exe, `field` `size` bytes of live memory, on the game's
    /// thread.
    pub unsafe fn read_delta(this: *mut Self, field: *mut c_void, size: u32) {
        // SAFETY: passed on to the caller.
        unsafe { field_call(eix_sites::DELTA_FIELD_READ_DELTA, this, field, size) }
    }

    /// Writes `size` bytes of `field` unconditionally: `DeltaField::WriteBoth` 0x47E140 (`MapObjectWriteNetState`
    /// writes each effect record's fields with it, 0x50EBD8).
    ///
    /// # Safety
    /// As for [`write_delta`](Self::write_delta).
    pub unsafe fn write_both(this: *mut Self, field: *mut c_void, size: u32) {
        // SAFETY: passed on to the caller.
        unsafe { field_call(eix_sites::DELTA_FIELD_WRITE_BOTH, this, field, size) }
    }

    /// Reads `size` bytes into `field`: `DeltaField::ReadRaw` 0x47E170, the reader of [`write_both`](Self::write_both)'s
    /// fields (`CMapObject::ReadNetState` 0x50EF65).
    ///
    /// # Safety
    /// As for [`read_delta`](Self::read_delta).
    pub unsafe fn read_raw(this: *mut Self, field: *mut c_void, size: u32) {
        // SAFETY: passed on to the caller.
        unsafe { field_call(eix_sites::DELTA_FIELD_READ_RAW, this, field, size) }
    }
}
