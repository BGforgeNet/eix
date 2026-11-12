//! `check {PATH}...`: compiles mob files' scripts and text scripts with the game's script compiler and
//! prints each file's result.
#![expect(
    unsafe_code,
    reason = "the check compiles through game.exe's script and world code"
)]

use std::ffi::CString;

use eix_core::cli::{CheckTally, FileCheck, check_line, check_usage, is_mob_path};
use eix_core::mod_files::with_slash;
use eix_engine::game::mfc::{self, GameString};
use eix_engine::game::script::{MISSION_SCRIPT, Script};
use eix_engine::game::world::{SERVER_WORLD, World};
use eix_engine::win::{find_files, from_ansi, to_ansi};

use super::{Folders, is_file, print};
use crate::setup::{c_string_until_nul, path};

/// The memory for the text-script compiler, 0x250 bytes; a
/// script is 0x1F8 bytes (the server world embeds two that far apart, 0x589F5F, 0x589F6F).
#[repr(C, align(8))]
struct ScriptMemory([u8; 0x250]);

const _: () = {
    assert!(core::mem::offset_of!(ScriptMemory, 0) == 0x0);
};

/// The script text files compile into: made on the first text file, kept for the rest of the command.
struct TextScript(Option<Box<ScriptMemory>>);

impl TextScript {
    /// The script, constructed on first use.
    fn get(&mut self) -> *mut Script {
        let memory = self.0.get_or_insert_with(|| {
            let mut m = Box::new(ScriptMemory([0; 0x250]));
            // SAFETY: zeroed memory larger than a script, on the game's thread; nothing is constructed in it yet.
            unsafe { Script::construct(m.0.as_mut_ptr().cast()) };
            m
        });
        memory.0.as_mut_ptr().cast()
    }
}

impl Drop for TextScript {
    fn drop(&mut self) {
        if let Some(m) = &mut self.0 {
            // SAFETY: the script constructed in `get`, not used afterwards.
            unsafe { Script::destruct(m.0.as_mut_ptr().cast()) };
        }
    }
}

/// Compiles `text` (up to its first NUL, as the game string takes it) into `script`.
fn compile_text(script: *mut Script, text: &[u8]) {
    let text = c_string_until_nul(text);
    let mut string = GameString {
        chars: std::ptr::null_mut(),
    };
    // SAFETY: on the game's thread; `string` holds no constructed string yet.
    unsafe { mfc::construct_empty(&raw mut string) };
    // SAFETY: the string constructed above; `text` is NUL-terminated.
    unsafe { mfc::assign(&raw mut string, &text) };
    // SAFETY: a constructed script, on the game's thread.
    unsafe { Script::reset(script) };
    // SAFETY: as above; the string lives through the call.
    unsafe { Script::load_text(script, &raw const string, 0) };
    // SAFETY: the string constructed above, not used afterwards.
    unsafe { mfc::destroy(&raw mut string) };
}

/// Checks one file and prints its line.
fn check_one(file: &[u8], text_script: &mut TextScript) -> FileCheck {
    let script = if is_mob_path(file) {
        let Ok(name) = CString::new(file) else {
            return FileCheck::Unreadable;
        };
        // SAFETY: the server world is constructed before WinMain runs; on the game's thread.
        unsafe { World::load_zone_scripts(SERVER_WORLD.as_ptr(), &name) };
        MISSION_SCRIPT.as_ptr()
    } else {
        match std::fs::read(path(file)) {
            Ok(text) => {
                let script = text_script.get();
                compile_text(script, &text);
                script
            }
            Err(_) => {
                print(&check_line(file, FileCheck::Unreadable, 0, b""));
                return FileCheck::Unreadable;
            }
        }
    };
    // SAFETY: a constructed script, which nothing changes while it is read here.
    let script = unsafe { &*script };
    // SAFETY: the script's error text is a constructed game string.
    let message = unsafe { script.error_text.as_bytes() }.to_vec();
    let result = if script.error_code != 0 {
        FileCheck::ScriptError
    } else {
        FileCheck::Ok
    };
    print(&check_line(file, result, script.error_line, &message));
    result
}

/// Checks the non-folder entries `found` of `folder` (ending in `\`).
fn check_entries(
    folder: &[u8],
    found: &[eix_engine::win::FoundFile],
    tally: &mut CheckTally,
    text_script: &mut TextScript,
) {
    for entry in found.iter().filter(|e| !e.directory) {
        let file = [folder, &to_ansi(&entry.name)[..]].concat();
        tally.file(check_one(&file, text_script));
    }
}

/// The folder part of `file` up to its last `\`, kept.
fn folder_of(file: &[u8]) -> Vec<u8> {
    match file.iter().rposition(|&c| c == b'\\') {
        Some(at) => file[..=at].to_vec(),
        None => Vec::new(),
    }
}

/// Runs `check` over `args`; the exit code.
pub(super) fn run(args: &[Vec<u8>], runner: &[u8], folders: &Folders) -> i32 {
    if args.is_empty() {
        print(&check_usage(runner));
        return 0;
    }
    let mut tally = CheckTally::default();
    let mut text_script = TextScript(None);
    for arg in args {
        let full = folders.resolve(arg);
        if is_file(&full) {
            tally.file(check_one(&full, &mut text_script));
        } else if path(&full).is_dir() {
            let folder = with_slash(full);
            match find_files(&from_ansi(&[&folder[..], b"*.mob"].concat())) {
                Some(found) => check_entries(&folder, &found, &mut tally, &mut text_script),
                None => {
                    print(&[&folder[..], b": There are no .mob files in directory\n"].concat());
                    tally.nothing();
                }
            }
        } else {
            match find_files(&from_ansi(&full)) {
                Some(found) => check_entries(
                    &with_slash(folder_of(&full)),
                    &found,
                    &mut tally,
                    &mut text_script,
                ),
                None => {
                    print(&[&full[..], b": File(s) not found\n"].concat());
                    tally.nothing();
                }
            }
        }
    }
    tally.code()
}
