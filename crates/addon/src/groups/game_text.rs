//! Text the game reads through a patched operand: game `CString`s and C strings that live as long as the process.
#![expect(
    unsafe_code,
    reason = "constructing a game CString in memory the game then reads"
)]

use eix_engine::game::mfc::{self, GameString};

use super::{BuildError, static_c_str};

/// A game `CString` holding `text`, never destroyed; returns its address, whose first word is the text.
///
/// The game reads it both ways: `mov eax, [string]` (the text) and as a `CString*` it copies or appends to.
pub(crate) fn game_string(text: Vec<u8>) -> Result<usize, BuildError> {
    let text = static_c_str(text)?;
    let string: &'static mut GameString = Box::leak(Box::new(GameString {
        chars: core::ptr::null_mut(),
    }));
    let address = core::ptr::from_mut(string);
    // SAFETY: the build is a supported one (checked before the WinMain hook); the slot is fresh, writable and holds no
    // string; this runs on the game's thread before WinMain.
    unsafe { mfc::construct(address, text) };
    Ok(address.expose_provenance())
}

/// A table of C string pointers ending in a pointer to an empty string, as game.exe's zone tables end (0x57F6C6
/// stops at an entry whose first byte is 0); never freed. Returns the table's address.
pub(crate) fn string_table(entries: &[Vec<u8>]) -> Result<usize, BuildError> {
    let mut table = Vec::with_capacity(entries.len() + 1);
    for e in entries {
        table.push(static_c_str(e.clone())?.as_ptr().expose_provenance());
    }
    table.push(static_c_str(Vec::new())?.as_ptr().expose_provenance());
    Ok(Box::leak(table.into_boxed_slice())
        .as_ptr()
        .expose_provenance())
}
