//! ImproveConsole: the console keeps no repeated history line and gets the clipboard keys.
//!
//! In `ConsoleWindow::OnChar` 0x5E44A0:
//! - on Enter (0x5E44DD) a line equal to the newest history entry (compared with case) is not added again: the
//!   code goes on past the history insert (0x5E4503);
//! - with Ctrl and not Shift (0x5E46CF, before the typed character is inserted): Ctrl+A selects the line, Ctrl+C
//!   and Ctrl+X copy the selection to the clipboard as Unicode text (Ctrl+X also removes it), Ctrl+V pastes the
//!   clipboard's first line over the selection. The game then ignores the key, since Ctrl is held.
#![expect(unsafe_code, reason = "editing the game's console window")]

use eix_core::console_edit;
use eix_engine::game::console::{self, ConsoleWindow};
use eix_engine::game::mfc::{self, GameString};
use eix_engine::hook::{self, Context, Mode};
use eix_engine::win::{clipboard_text, from_ansi, set_clipboard_text, to_ansi};

use super::{BuildError, Labelled, Setup};

/// `lea edi, [esi+0x24]; lea ebp, [esi+0xC]`, the start of the Enter case.
const ENTER: usize = eix_sites::CONSOLE_WINDOW_ON_CHAR__3D;
const ENTER_BYTES: [u8; 6] = [0x8D, 0x7E, 0x24, 0x8D, 0x6E, 0x0C];
/// Past the history insert.
const AFTER_INSERT: usize = eix_sites::CONSOLE_WINDOW_ON_CHAR__63;
/// `mov al, [esi+0x78]; test al, al`, the Ctrl test before a character is typed.
const CHARACTER: usize = eix_sites::CONSOLE_WINDOW_ON_CHAR__22F;
const CHARACTER_BYTES: [u8; 5] = [0x8A, 0x46, 0x78, 0x84, 0xC0];
/// OnChar's key argument, as a stack word index at the character site.
const KEY: usize = 0x54 / 4;

/// The bytes of a constructed game string.
///
/// # Safety
/// `s` must be a constructed game string that does not change while the bytes are used.
unsafe fn bytes(s: &GameString) -> Vec<u8> {
    // SAFETY: passed on.
    unsafe { s.as_bytes() }.to_vec()
}

extern "C" fn skip_repeat(_: usize, ctx: &mut Context) {
    let window = ctx.esi as *const ConsoleWindow;
    // SAFETY: ESI is OnChar's `this`, the live console window, on the game's thread.
    let w = unsafe { &*window };
    // SAFETY: the history is a constructed string array.
    let Some(newest) = (unsafe { w.history.as_slice() }).first() else {
        return;
    };
    // SAFETY: a constructed game string, unchanged during the comparison.
    let newest = unsafe { newest.as_bytes() };
    // SAFETY: as above.
    let same = newest == unsafe { w.input.as_bytes() };
    if same {
        ctx.resume = AFTER_INSERT;
    }
}

/// Replaces the window's input with `text` and puts the caret and the anchor at `position`, then redraws.
///
/// # Safety
/// `window` must be the live console window, on the game's thread.
unsafe fn set_input(window: *mut ConsoleWindow, text: &[u8], position: i32) {
    // A NUL inside the line cannot come from the console's own text; it ends the line there.
    let line = crate::setup::c_string_until_nul(text);
    // SAFETY: a field address of the live window; nothing is read.
    let input = unsafe { &raw mut (*window).input };
    // SAFETY: the window's input is a constructed game string.
    unsafe { mfc::assign(input, &line) };
    // SAFETY: the window is live.
    let w = unsafe { &mut *window };
    w.caret = position;
    w.anchor = position;
    // SAFETY: as above.
    unsafe { console::draw_input(window) };
}

extern "C" fn clipboard(_: usize, ctx: &mut Context) {
    let window = ctx.esi as *mut ConsoleWindow;
    // SAFETY: ESI is OnChar's `this`, the live console window, on the game's thread.
    let w = unsafe { &mut *window };
    if w.ctrl == 0 || w.shift != 0 {
        return;
    }
    let Some(key) = ctx.stack_word(KEY) else {
        return;
    };
    let letter = (key as u8).wrapping_add(0x60);
    let (low, high) = console_edit::selection(w.caret, w.anchor);
    // SAFETY: the input is a constructed game string.
    let input = unsafe { bytes(&w.input) };
    match letter {
        b'a' => {
            w.anchor = 0;
            w.caret = i32::try_from(input.len()).unwrap_or(i32::MAX);
            // SAFETY: as above.
            unsafe { console::draw_input(window) };
        }
        b'c' | b'x' if w.caret != w.anchor => {
            let selected = console_edit::mid(console_edit::left(&input, high), low);
            set_clipboard_text(&from_ansi(selected));
            if letter == b'x' {
                // SAFETY: as above.
                unsafe { set_input(window, &console_edit::cut(&input, low, high), low) };
            }
        }
        b'v' => {
            let clip = to_ansi(&clipboard_text());
            let line = console_edit::first_line(&clip);
            let (text, position) =
                console_edit::paste(&input, low, high, line, w.anchor != w.caret);
            // SAFETY: as above.
            unsafe { set_input(window, &text, position) };
        }
        _ => {}
    }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![
        Labelled {
            label: "ImproveConsole #0".to_owned(),
            write: hook::context_hook(ENTER, &ENTER_BYTES, Mode::Before, skip_repeat, 0)?,
        },
        Labelled {
            label: "ImproveConsole #1".to_owned(),
            write: hook::context_hook(CHARACTER, &CHARACTER_BYTES, Mode::Before, clipboard, 0)?,
        },
    ])
}
