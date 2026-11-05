//! KeepWindowOnFocusLoss: a windowed game stays on the screen when it loses the focus. On `WA_INACTIVE`, `WindowProc`
//! 0x457D40 pauses, releases the mouse, stops the sound and minimises the window (`ShowWindow(SW_MINIMIZE)` at
//! 0x457E89) in either mode. Windowed, the minimise is skipped; in full screen it stays, as the device has just given
//! the display back (0x457E62). The pause and the rest stay in both.
#![expect(
    unsafe_code,
    reason = "reads the game's device at its window procedure's focus loss"
)]

use eix_engine::game::surface::{DEVICE, DEVICE_FULL_SCREEN};
use eix_engine::hook::{self, Context, Mode};

use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// `mov ecx, [0x79BC28]`: the game's window, for the `push 6; push ecx; call [ShowWindow]` that follows.
const LOAD_WINDOW: usize = eix_sites::WINDOW_PROC__143;
const LOAD_WINDOW_BYTES: [u8; 6] = [0x8B, 0x0D, 0x28, 0xBC, 0x79, 0x00];
/// After the `ShowWindow` call: the procedure's `return 1`.
const AFTER_MINIMISE: usize = eix_sites::WINDOW_PROC__152;

extern "C" fn handler(_: usize, ctx: &mut Context) {
    // SAFETY: WindowProc runs on the game's thread; the global holds the device or null.
    let device = unsafe { DEVICE.read() };
    if device.is_null() {
        // No renderer yet: nothing tells windowed from full screen, so the game's own minimise stands.
        return;
    }
    // SAFETY: a non-null device pointer is the game's live CTextureDevice.
    let flags = unsafe { (*device).flags };
    if flags & DEVICE_FULL_SCREEN == 0 {
        ctx.resume = AFTER_MINIMISE;
    }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "KeepWindowOnFocusLoss #0".to_owned(),
        write: hook::context_hook(LOAD_WINDOW, &LOAD_WINDOW_BYTES, Mode::Before, handler, 0)?,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skip_lands_past_the_minimise_call() {
        // `push 6; push ecx; call [ShowWindow]` is 2 + 1 + 6 bytes after the hooked load.
        assert_eq!(AFTER_MINIMISE, LOAD_WINDOW + LOAD_WINDOW_BYTES.len() + 9);
        let w = build(&Setup::default()).unwrap();
        assert_eq!(
            (w[0].write.site(), w[0].write.expected()),
            (LOAD_WINDOW, &LOAD_WINDOW_BYTES[..])
        );
    }
}
