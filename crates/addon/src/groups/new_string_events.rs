//! NewStringEvents: the string event `gameover [title [text]]` ends the game with a message window.
//!
//! `CInterface3D::OnStringEvent` 0x5F8070 splits the event into lower-cased words (the array at its stack +0x10)
//! and compares the first with its own events. The hook at 0x5F80EF, after the first comparison, catches
//! `gameover`: the sounds and the ambience stop, `buttons\gameover.wav` plays, and a message window with the title
//! and text keys (defaults `game_over`, `game_over_msg`) opens; the function then returns through its clean-up
//! (0x5F8259).
#![expect(
    unsafe_code,
    reason = "reading the event's words and calling the game's interface code"
)]

use std::ffi::CStr;

use eix_engine::game::flow;
use eix_engine::game::mfc::{self, GameArray, GameString};
use eix_engine::game::ui::{self, Interface};
use eix_engine::hook::{self, Context, Mode};

use super::{BuildError, Labelled, Setup};

/// `add esp, 8; test eax, eax`, after the `_stricmp` with the first event name.
const SITE: usize = eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__7F;
const ORIGINAL: [u8; 5] = [0x83, 0xC4, 0x08, 0x85, 0xC0];
/// Where the function's clean-up starts.
const DONE: usize = eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__1E9;
/// The words, at the site's stack +0x10.
const WORDS: usize = 0x10;

/// Word `i` of the event, when there is one.
///
/// # Safety
/// `words` must be the event's constructed word array.
unsafe fn word(words: &GameArray<GameString>, i: usize) -> Option<&CStr> {
    // SAFETY: the caller guarantees the array; its strings are constructed and NUL-terminated.
    let s = unsafe { words.as_slice() }.get(i)?;
    // SAFETY: a constructed game string's characters are NUL-terminated.
    Some(unsafe { CStr::from_ptr(s.chars.cast()) })
}

extern "C" fn game_over(_: usize, ctx: &mut Context) {
    let words = (ctx.stack() + WORDS) as *const GameArray<GameString>;
    // SAFETY: at the site the word array OnStringEvent built lies at its stack +0x10 (0x5F8098), constructed.
    let words = unsafe { &*words };
    // SAFETY: as above.
    let first = unsafe { word(words, 0) };
    if first.map(CStr::to_bytes) != Some(b"gameover") {
        return;
    }
    // SAFETY: as above.
    let title = unsafe { word(words, 1) }.unwrap_or(c"game_over");
    // SAFETY: as above.
    let text = unsafe { word(words, 2) }.unwrap_or(c"game_over_msg");
    let interface = ctx.esi as *mut Interface;
    // SAFETY: the game's thread, in a string event of the live interface (ESI is OnStringEvent's `this`).
    unsafe { flow::stop_sounds(true) };
    // SAFETY: as above.
    unsafe { flow::stop_ambience() };
    // SAFETY: as above.
    unsafe { Interface::play_sound(interface, c"buttons\\gameover.wav") };
    // SAFETY: the game's allocator; the interface owns the window once pushed.
    let block = unsafe { mfc::new(ui::MESSAGE_BOX_SIZE) };
    // The original pushes a null window when the allocation fails; nothing is pushed here.
    if !block.is_null() {
        // SAFETY: a fresh block of the message box's size; the texts outlive the call (the window copies them).
        unsafe { ui::construct_message_box(block, interface, title, text) };
        // SAFETY: as above; the window is constructed.
        unsafe { Interface::push_window(interface, block) };
    }
    ctx.resume = DONE;
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "NewStringEvents #0".to_owned(),
        write: hook::context_hook(SITE, &ORIGINAL, Mode::Before, game_over, 0)?,
    }])
}
