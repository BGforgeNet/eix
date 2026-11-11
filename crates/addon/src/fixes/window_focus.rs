//! KeepRunningOnFocusLoss and ClipCursorToWindow, which share one hook: the entry of `WindowProc` 0x457D40, where the
//! window's messages arrive.
//!
//! On `WM_ACTIVATE` with `WA_INACTIVE` the game clears the active flag 0x79BC5C (the main tick returns at once while it
//! is 0, so a multiplayer client falls out of step), stops the sound samples, unacquires DirectInput and minimises the
//! window. In a window KeepRunningOnFocusLoss answers that message itself, as handled, and the game's code for it never
//! runs: the tick, the sound and the input device stay as they are, and the window is not minimised either. Minimising
//! the window yourself (`WM_ACTIVATE` with the minimised flag) stays the game's. Full screen stays the game's: the
//! device has to give the display back.
//!
//! ClipCursorToWindow confines the cursor to the client area while the window is active (core's
//! [`cursor_clip`] names the messages), and lets it go on focus loss, minimising and destroying the window.
#![expect(
    unsafe_code,
    reason = "asks whether the game is windowed from its window procedure, which runs on the game's thread"
)]

use std::sync::atomic::{AtomicBool, Ordering};

use eix_core::engine_fixes::{CursorClip, cursor_clip, is_focus_loss};
use eix_engine::game::surface::is_windowed;
use eix_engine::hook::{self, Context, Mode};
use eix_engine::win;
use eix_sites::Site;

use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

use super::Warnings;

/// `sub esp, 0x50; push ebx; push esi`: the procedure's first instructions.
const ENTRY: Site = eix_sites::WINDOW_PROC;
const ENTRY_BYTES: [u8; 5] = [0x83, 0xEC, 0x50, 0x53, 0x56];
/// `mov eax, ebx; pop esi; pop ebx; add esp, 0x50; ret 0x10`: the procedure's `return 1`.
const RETURN_HANDLED: Site = eix_sites::WINDOW_PROC__152;
/// Where the hook sees the arguments, in stack words from the stack's top: the entry's 0x50 and two pushes, the return
/// address, then the window, the message, `wParam` and `lParam`.
const WINDOW_WORD: usize = (0x50 + 8) / 4 + 1;

/// The hook's argument: which settings are on.
const KEEP_RUNNING: usize = 1;
const CLIP_CURSOR: usize = 2;

/// Whether this fix holds the cursor clip; the game's thread alone changes it.
static CLIPPED: AtomicBool = AtomicBool::new(false);
static WARNINGS: Warnings = Warnings::new("WindowFocus");

fn clip(window: usize) {
    if win::clip_cursor_to_client(window) {
        CLIPPED.store(true, Ordering::Relaxed);
    } else {
        WARNINGS.warn(|| "the cursor could not be confined to the window".to_owned());
    }
}

fn release() {
    if CLIPPED.swap(false, Ordering::Relaxed) {
        win::release_cursor_clip();
    }
}

extern "C" fn handler(settings: usize, ctx: &mut Context) {
    let (Some(window), Some(message), Some(wparam)) = (
        ctx.stack_word(WINDOW_WORD),
        ctx.stack_word(WINDOW_WORD + 1),
        ctx.stack_word(WINDOW_WORD + 2),
    ) else {
        return;
    };
    let window = window as usize;
    // SAFETY: WindowProc runs on the game's thread.
    let windowed = unsafe { is_windowed() };
    if settings & CLIP_CURSOR != 0 {
        match cursor_clip(message, wparam, windowed) {
            CursorClip::Set => clip(window),
            CursorClip::Refresh if win::is_foreground(window) => clip(window),
            CursorClip::Refresh | CursorClip::Release => release(),
            CursorClip::Keep => {}
        }
    }
    if settings & KEEP_RUNNING != 0 && windowed && is_focus_loss(message, wparam) {
        // The procedure returns 1, which is what the game's own handling returns.
        ctx.ebx = 1;
        ctx.resume = RETURN_HANDLED;
    }
}

/// Off while both settings are 0.
pub(super) fn off(setup: &Setup) -> Option<String> {
    let s = &setup.options.eix.fixes;
    (s.keep_running_on_focus_loss == 0 && s.clip_cursor_to_window == 0)
        .then(|| "KeepRunningOnFocusLoss and ClipCursorToWindow are 0".to_owned())
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let s = &setup.options.eix.fixes;
    let settings = if s.keep_running_on_focus_loss != 0 {
        KEEP_RUNNING
    } else {
        0
    } | if s.clip_cursor_to_window != 0 {
        CLIP_CURSOR
    } else {
        0
    };
    Ok(vec![Labelled {
        label: "WindowFocus #0".to_owned(),
        write: hook::context_hook(ENTRY, &ENTRY_BYTES, Mode::Before, handler, settings)?,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hook_sits_at_the_procedures_entry() {
        let w = build(&Setup::default()).unwrap();
        assert_eq!(w.len(), 1);
        assert_eq!(
            (w[0].write.site(), w[0].write.expected()),
            (ENTRY, &ENTRY_BYTES[..])
        );
        // 0x58 bytes of frame, the return address, the window: `WindowProc(hwnd, msg, wparam, lparam)`.
        assert_eq!(WINDOW_WORD, 23);
    }

    #[test]
    fn it_is_off_only_when_both_settings_are_0() {
        let with = |keep, clip| {
            let mut setup = Setup::default();
            setup.options.eix.fixes.keep_running_on_focus_loss = keep;
            setup.options.eix.fixes.clip_cursor_to_window = clip;
            off(&setup)
        };
        assert!(with(0, 0).is_some());
        assert!(with(1, 0).is_none());
        assert!(with(0, 1).is_none());
        assert!(with(1, 1).is_none());
    }
}
