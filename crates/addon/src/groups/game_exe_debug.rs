//! GameExeDebug: game.exe's debug print (0x44C470, a cdecl `printf`-like function whose body is a bare `ret` in the
//! release build) writes its lines to `[DEV] GameDebugFile`, a path relative to the game's working folder.
//!
//! The hook runs before the `ret`, which then returns to the caller; the caller pops the arguments.
#![expect(
    unsafe_code,
    reason = "formatting the game's format string with its stack arguments"
)]

use std::io::Write;
use std::sync::OnceLock;

use eix_engine::hook::{self, Context, Mode};

use super::{BuildError, Labelled, Setup};
use crate::setup::path;

static FILE: OnceLock<std::path::PathBuf> = OnceLock::new();

extern "C" fn handler(_: usize, ctx: &mut Context) {
    let (Some(file), Some(format)) = (FILE.get(), ctx.stack_word(1)) else {
        return;
    };
    let format = core::ptr::with_exposed_provenance::<u8>(format as usize);
    if format.is_null() {
        return;
    }
    let args = core::ptr::with_exposed_provenance::<core::ffi::c_void>(ctx.stack() + 8);
    // SAFETY: at 0x44C470 the caller passed a NUL-terminated format and, after it on the stack, the arguments its
    // conversions read (word 0 is the return address, word 1 the format).
    let text = unsafe { eix_engine::win::wvsprintf(format, args) };
    // The text, a line end, and every `\n` as `\r\n`: what the original's text-mode `fprintf("%s\n")` writes.
    let out = eix_core::cli::crt_text(&[&text[..], b"\n"].concat());
    // The debug file is the game's own diagnostics; a line that cannot be written is dropped like the original's.
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
    {
        let _ = f.write_all(&out);
    }
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let _ = FILE.set(path(&setup.options.config.dev.game_debug_file));
    let write = hook::context_hook(
        eix_sites::DEBUG_LOG_STUB,
        &[0xC3, 0x90, 0x90, 0x90, 0x90, 0x90],
        Mode::After,
        handler,
        0,
    )?;
    Ok(vec![Labelled {
        label: "Game.exe Debug log".to_owned(),
        write,
    }])
}
