//! The registry's `AutoRecover` (default on): before the crash box, the original writes the hosting player's party
//! to `<mod folder>\mp\backup\<DD.MM.YYYY-n>\` (after the dump) and the box names that folder. Nothing is copied: the
//! game serialises the party twice, through its own party and save-file calls. Names and texts are core's
//! (`eix_core::auto_recover`).
//!
//! The backup runs inside the crash filter, on the crashing thread with every other thread stopped, as in the
//! original. It needs a live server session and a player 0: a hosted multiplayer game, not a single-player one.
#![expect(
    unsafe_code,
    reason = "calls into game.exe's world, party and stream code to write the party files"
)]

use std::ffi::{CStr, CString};
use std::sync::OnceLock;

use eix_core::auto_recover as names;
use eix_engine::game::net::live_server;
use eix_engine::game::player::{HERO_FILE, Player};
use eix_engine::game::stream::{self, FILE_STREAM_BYTES, MEMORY_STREAM_BYTES, StreamMemory};
use eix_engine::game::world::{SAVE_VERSION, SERVER_WORLD, World};
use eix_engine::win::{ansi_text, local_time};

use crate::log;
use crate::setup::path;

/// The kind `SaveFileWrite` gets for a party file.
const PARTY_FILE_KIND: u32 = 0x114;

/// The mod folder (ending in `\`, empty for no mod), set once the options are read when `AutoRecover` is on and the
/// game is known to be the supported build; until then, and with the setting off, a crash backs nothing up.
static MOD_DIR: OnceLock<Vec<u8>> = OnceLock::new();

/// Turns the backup on for crashes from now on.
pub(crate) fn enable(mod_dir: &[u8]) {
    let _ = MOD_DIR.set(mod_dir.to_vec());
}

/// Whether `dir` is a folder afterwards: it is, or `CreateDirectory` makes it (one level, as the original's call).
fn make_dir(dir: &[u8]) -> bool {
    let dir = path(dir);
    dir.is_dir() || std::fs::create_dir(&dir).is_ok()
}

/// The hosting player: player 0 of the server world, when the game has a live server session.
///
/// # Safety
/// The process must be a supported game.exe.
unsafe fn player_zero() -> Option<*mut Player> {
    // SAFETY: the caller guarantees the build.
    unsafe { live_server() }?;
    // SAFETY: the server world is constructed at start-up.
    let player = unsafe { World::get_player(SERVER_WORLD.as_ptr(), 0) };
    (!player.is_null()).then_some(player)
}

/// Writes `player`'s party to the file `file`: the party into a memory stream, then the stream into a
/// file stream as a save record. Returns false when the file name is invalid or the file does not open.
///
/// # Safety
/// `player` must be a live player of a supported game.exe.
unsafe fn write_party(player: *mut Player, file: &[u8]) -> bool {
    let Ok(file) = CString::new(file) else {
        log::warn("crash", "a party file name holds a NUL", "");
        return false;
    };
    let mut memory = StreamMemory::<MEMORY_STREAM_BYTES>::new();
    let memory = memory.as_mut_ptr();
    let mut stream_memory = StreamMemory::<FILE_STREAM_BYTES>::new();
    let out = stream_memory.as_mut_ptr();
    // The calls are the original's, in its order.
    // SAFETY: memory for the stream, on the crashing thread.
    unsafe { stream::construct_memory_stream(memory) };
    // SAFETY: the caller's live player; the constructed memory stream.
    unsafe { Player::save_party(player, memory, 1) };
    // SAFETY: memory for the stream.
    unsafe { stream::construct_file_stream(out) };
    // SAFETY: the constructed file stream; `file` is NUL-terminated.
    let opened = unsafe { stream::open_file_stream_for_writing(out, &file) };
    if opened {
        // SAFETY: the global holds the save version; both streams are constructed and the output file is open.
        let version = unsafe { SAVE_VERSION.read() };
        // SAFETY: as above.
        unsafe { stream::save_file_write(out, PARTY_FILE_KIND, version, memory) };
        // SAFETY: the constructed file stream has an open file.
        unsafe { stream::close_file(out) };
    } else {
        log::warn(
            "crash",
            "cannot write the party file",
            &path(file.to_bytes()).display().to_string(),
        );
    }
    // SAFETY: as above; the file stream is not used afterwards.
    unsafe { stream::destruct_file_stream(out) };
    // SAFETY: as above.
    unsafe { stream::destroy_in_place(out) };
    // SAFETY: the constructed memory stream, not used afterwards.
    unsafe { stream::destruct_memory_stream(memory) };
    // SAFETY: as above.
    unsafe { stream::destroy_in_place(memory) };
    opened
}

/// The game's current hero file name as bytes (empty when the global is null).
///
/// # Safety
/// The process must be a supported game.exe.
unsafe fn hero_file() -> Vec<u8> {
    // SAFETY: the caller guarantees the build; the global holds a C string or null.
    let name = unsafe { HERO_FILE.read() };
    if name.is_null() {
        return Vec::new();
    }
    // SAFETY: a non-null value is the characters of the game's string.
    unsafe { CStr::from_ptr(name) }.to_bytes().to_vec()
}

/// The two party files of the original's backup: the party as the records stand, then after
/// the dead units' loot was gathered and the player's list emptied, which the original empties once more at the end.
///
/// # Safety
/// `player` must be the live player 0 of a supported game.exe.
unsafe fn write_files(player: *mut Player, folder: &[u8]) -> bool {
    let world = SERVER_WORLD.as_ptr();
    // The calls are the original's, in its order; the hero file's name is read again for the second file.
    // SAFETY: the caller guarantees the live player and the build, on the crashing thread.
    unsafe { World::sync_party_records(world, player) };
    // SAFETY: as above.
    let first = unsafe { hero_file() };
    // SAFETY: as above.
    let first_written = unsafe { write_party(player, &names::party_files(folder, &first)[0]) };
    // SAFETY: as above.
    unsafe { World::clear_player_items(world, player) };
    // SAFETY: as above.
    unsafe { World::loot_dead_units(world) };
    // SAFETY: as above.
    unsafe { World::sync_party_records(world, player) };
    // SAFETY: as above.
    let second = unsafe { hero_file() };
    // SAFETY: as above.
    let second_written = unsafe { write_party(player, &names::party_files(folder, &second)[1]) };
    // SAFETY: as above.
    unsafe { World::clear_player_items(world, player) };
    first_written && second_written
}

/// Backs the party up as the crash filter does; the backup folder (ending in `\`), or empty when nothing was backed
/// up: the setting is off, the game has no live server or no player 0, a folder cannot be made, or a party file
/// cannot be written.
pub(crate) fn run() -> Vec<u8> {
    let Some(mod_dir) = MOD_DIR.get() else {
        return Vec::new();
    };
    // SAFETY: `enable` runs only for the supported build.
    let Some(player) = (unsafe { player_zero() }) else {
        return Vec::new();
    };
    let dir = names::backups_dir(mod_dir);
    if !make_dir(&dir) {
        log::warn(
            "crash",
            "no backup: cannot make the folder",
            &ansi_text(&dir),
        );
        return Vec::new();
    }
    let [year, month, day, ..] = local_time();
    // The original falls through with the folder `\` when all 1000 names are taken and writes into the drive's root;
    // here that case backs nothing up.
    let Some(name) = names::first_free_name(day, month, year, |n| {
        path(&[&dir[..], n.as_bytes()].concat()).exists()
    }) else {
        log::warn(
            "crash",
            "no backup: all of today's folders exist",
            &ansi_text(&dir),
        );
        return Vec::new();
    };
    let folder = names::folder_path(&dir, &name);
    if !make_dir(&folder) {
        log::warn(
            "crash",
            "no backup: cannot make the folder",
            &ansi_text(&folder),
        );
        return Vec::new();
    }
    // SAFETY: `player_zero` found the live player 0 above.
    if !unsafe { write_files(player, &folder) } {
        log::warn(
            "crash",
            "party backup incomplete",
            &path(&folder).display().to_string(),
        );
        return Vec::new();
    }
    log::info("crash", "saves backed up", &ansi_text(&folder));
    folder
}
