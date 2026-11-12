//! `aigraph {single|multi} mob_name [mpr_name]`: loads the zone without the game's start-up, builds its
//! AI graph with the GraphGenPatches rule and writes it beside the mob file as `.grp`.
#![expect(
    unsafe_code,
    reason = "the graph is built and saved through game.exe's start-up, world, AI map and stream code"
)]

use std::ffi::CStr;

use eix_core::cli::{aigraph_args, graph_file};
use eix_engine::game::ai::{AI_MAP, REGION_MAP, RegionMap};
use eix_engine::game::mfc::{self, GameString};
use eix_engine::game::start;
use eix_engine::game::stream::{self, FILE_STREAM_BYTES, MEMORY_STREAM_BYTES, StreamMemory};
use eix_engine::game::world::{SERVER_WORLD, World};

use super::{is_file, print};
use crate::setup::{ansi, c_string_until_nul, path};

/// Whether the zone file `name` is in the game's `maps\` or the mod's.
fn found(name: &[u8], mod_dir: &[u8]) -> bool {
    is_file(&[&b"maps\\"[..], name].concat()) || is_file(&[mod_dir, b"maps\\", name].concat())
}

/// The text of a constructed game string.
///
/// # Safety
/// `string` must be a constructed game string.
unsafe fn text_of(string: *const GameString) -> Vec<u8> {
    // SAFETY: the caller guarantees the string.
    let string = unsafe { &*string };
    // SAFETY: as above, constructed.
    unsafe { string.as_bytes() }.to_vec()
}

/// What WinMain loads before a zone, as the original repeats it.
fn load_game_data(multi: bool) {
    // SAFETY: on the game's thread before WinMain; the registry key string is constructed by the game's static
    // initialisers.
    unsafe {
        mfc::assign(
            start::REGISTRY_KEY.as_ptr(),
            c"Software\\Nival Interactive\\EvilIslands",
        );
    }
    // SAFETY: as above.
    unsafe { start::store_start_folder() };
    // SAFETY: the start folder string is constructed and was just filled.
    let folder = unsafe { text_of(start::START_FOLDER.as_ptr()) };
    let ai_reg = c_string_until_nul(&[&folder[..], b"config\\ai.reg"].concat());
    // SAFETY: on the game's thread; the path is NUL-terminated.
    unsafe { start::load_logic_settings(&ai_reg) };
    // SAFETY: the registry key string's characters, NUL-terminated.
    let key = unsafe { (*start::REGISTRY_KEY.as_ptr()).chars };
    // SAFETY: on the game's thread; the key lives in the game's string.
    unsafe { start::load_options(key) };
    // SAFETY: the server world is constructed by the game's static initialisers.
    unsafe { World::setup(SERVER_WORLD.as_ptr()) };
    let res: &CStr = if multi {
        c"databaselmp.res"
    } else {
        c"database.res"
    };
    // SAFETY: the options are loaded above.
    unsafe { start::load_databases(res) };
}

/// Builds the graph of the loaded zone and writes it for `mob`; the exit code.
fn build_and_save(mob: &CStr) -> i32 {
    // SAFETY: a zone is loaded into the server world.
    unsafe { World::remove_all_units(SERVER_WORLD.as_ptr()) };
    // SAFETY: the region map and the AI map of the loaded zone.
    unsafe { RegionMap::build(REGION_MAP.as_ptr(), AI_MAP.as_ptr()) };
    let mut graph = StreamMemory::<MEMORY_STREAM_BYTES>::new();
    let graph = graph.as_mut_ptr();
    // SAFETY: memory for a memory stream, on the game's thread.
    unsafe { stream::construct_memory_stream(graph) };
    // SAFETY: the built region map, into the constructed stream.
    unsafe { RegionMap::save_blocks(REGION_MAP.as_ptr(), graph) };
    let mut mapped = GameString {
        chars: std::ptr::null_mut(),
    };
    // SAFETY: `mapped` holds no constructed string yet.
    unsafe { mfc::construct_empty(&raw mut mapped) };
    // SAFETY: the server world and the constructed string; `mob` is NUL-terminated.
    unsafe { World::map_path(SERVER_WORLD.as_ptr(), &raw mut mapped, mob) };
    // SAFETY: constructed above.
    let mob_path = unsafe { text_of(&raw const mapped) };
    let code = match std::path::absolute(path(&mob_path)) {
        Ok(full) if !mob_path.is_empty() => {
            let graph_path = graph_file(&ansi(full.as_os_str()));
            let (text, code) = save_result(&graph_path, write_graph(graph, &graph_path));
            print(&text);
            code
        }
        Ok(_) | Err(_) => {
            print(&[&b"Failed get full path of "[..], &mob_path, b"\n"].concat());
            1
        }
    };
    // SAFETY: constructed above, not used afterwards.
    unsafe { mfc::destroy(&raw mut mapped) };
    // SAFETY: the memory stream constructed above.
    unsafe { stream::destruct_memory_stream(graph) };
    // SAFETY: as above; the stream is not used afterwards.
    unsafe { stream::destroy_in_place(graph) };
    code
}

/// Writes the graph stream to `file` through a game file stream; a
/// file that does not open is written nowhere, as in the original.
fn write_graph(graph: *mut std::ffi::c_void, file: &[u8]) -> bool {
    let name = c_string_until_nul(file);
    let mut memory = StreamMemory::<FILE_STREAM_BYTES>::new();
    let out = memory.as_mut_ptr();
    // SAFETY: memory for a file stream, on the game's thread.
    unsafe { stream::construct_file_stream(out) };
    // SAFETY: the constructed file stream; `name` is NUL-terminated.
    let opened = unsafe { stream::open_file_stream_for_writing(out, &name) };
    if opened {
        // SAFETY: both streams are constructed and the output file is open.
        unsafe { stream::copy_from(out, graph) };
    }
    // SAFETY: the constructed file stream.
    unsafe { stream::destruct_file_stream(out) };
    // SAFETY: as above; the stream is not used afterwards.
    unsafe { stream::destroy_in_place(out) };
    opened
}

/// The message and exit code after attempting to save the graph.
fn save_result(file: &[u8], saved: bool) -> (Vec<u8>, i32) {
    let lead: &[u8] = if saved {
        b"AI Graph successfully generated and saved to "
    } else {
        b"Failed to save AI Graph to "
    };
    ([lead, file, b"\n"].concat(), i32::from(!saved))
}

/// Runs `aigraph` with `args`; the exit code.
pub(super) fn run(args: &[Vec<u8>], runner: &[u8], mod_dir: &[u8]) -> i32 {
    let a = match aigraph_args(args, runner) {
        Ok(a) => a,
        Err((text, code)) => {
            print(&text);
            return code;
        }
    };
    for name in [&a.mob, &a.mpr] {
        if !found(name, mod_dir) {
            print(&[&name[..], b": File not found\n"].concat());
            return 3;
        }
    }
    load_game_data(a.multi);
    let (mpr, mob) = (c_string_until_nul(&a.mpr), c_string_until_nul(&a.mob));
    // SAFETY: the server world, set up above; the names are NUL-terminated.
    if unsafe { World::new_zone(SERVER_WORLD.as_ptr(), &mpr, &mob, c"") } == 0 {
        print(b"Failed to load zone\n");
        return 1;
    }
    build_and_save(&mob)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_graph_write_reports_failure_and_a_nonzero_exit() {
        assert_eq!(
            save_result(b"maps\\zone.grp", false),
            (b"Failed to save AI Graph to maps\\zone.grp\n".to_vec(), 1)
        );
        assert_eq!(
            save_result(b"maps\\zone.grp", true),
            (
                b"AI Graph successfully generated and saved to maps\\zone.grp\n".to_vec(),
                0
            )
        );
    }
}
