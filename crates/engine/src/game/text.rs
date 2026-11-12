//! The text manager (`CTextManager`): the game's texts by key, read line by line through one cursor.
//!
//! Names follow ei-rebuild.
#![expect(unsafe_code, reason = "calls into game.exe's text manager")]

use core::ffi::CStr;
use core::mem::offset_of;

use super::mfc::GameString;
use super::{Global, code};

/// The head of `CTextManager`: the open text and the cursor in it; never held by value.
///
/// One text is open at a time. `CTextManager::Open` 0x4DEFC0 sets the three fields (0x4DF000-0x4DF003, the length
/// through the source's `GetFile` at 0x4DEFEA); every reader (`PeekWord` 0x4DF770 at 0x4DF799-0x4DF7B7) tests
/// `cursor - text < length`.
#[repr(C)]
#[derive(Debug)]
pub struct TextManager {
    _unknown0: [u8; 0x20],
    /// The read position in the open text.
    pub cursor: *const u8,
    /// The open text, null when none is.
    pub text: *const u8,
    /// The open text's length in bytes. A failed `Open` clears `text` and `cursor` but leaves it as it was.
    pub length: u32,
}

const _: () = {
    assert!(offset_of!(TextManager, _unknown0) == 0x0);
    assert!(offset_of!(TextManager, cursor) == 0x20);
    assert!(offset_of!(TextManager, text) == 0x24);
    assert!(offset_of!(TextManager, length) == 0x28);
};

/// The text manager the interface, the briefings and the quest texts share: `CBriefingScreen::NextPhrase` 0x604C50
/// reads it at 0x604C96, `CTextManager::GetString` 0x4E0020 is called on it by `CInterface3D::MessageText`
/// (0x5F8D40).
pub const TEXTS: Global<*mut TextManager> = Global::at(eix_sites::DATA_7AF788);

/// Opens the text `key`: `CTextManager::Open` 0x4DEFC0, thiscall `(key)`, `ret 4`. Returns whether a source holds a
/// non-empty text by that key; the cursor is then at its start.
///
/// # Safety
/// The process must be a supported game.exe, on the game's thread, and `this` a live text manager.
pub unsafe fn open(this: *mut TextManager, key: &CStr) -> bool {
    // SAFETY: 0x4DEFC0 takes `this` in ECX and the key on the stack, which it cleans (`ret 4`); it returns 0 or 1 in
    // EAX.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut TextManager, *const u8) -> u32>(
            eix_sites::C_TEXT_MANAGER_OPEN,
        )
    };
    // SAFETY: the caller guarantees the build, the thread and `this`; `key` is NUL-terminated.
    unsafe { f(this, key.as_ptr().cast()) != 0 }
}

/// Reads the text `string <key>` into `out`, trimmed at its right end: `CTextManager::GetString` 0x4E0020, thiscall
/// `(key, out)`, `ret 8`, as `CHudUnitPanel::DrawUnitInfo` reads its labels (0x6471DA). It opens that text, so the
/// manager's open text and cursor change.
///
/// # Safety
/// The process must be a supported game.exe, on the game's thread; `this` must be a live text manager and `out` a
/// constructed string.
pub unsafe fn get_string(this: *mut TextManager, key: &CStr, out: *mut GameString) {
    // SAFETY: 0x4E0020 takes `this` in ECX and the key and the string on the stack, which it cleans (`ret 8`).
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut TextManager, *const u8, *mut GameString)>(
            eix_sites::C_TEXT_MANAGER_GET_STRING,
        )
    };
    // SAFETY: the caller guarantees the build, the thread, `this` and `out`; `key` is NUL-terminated.
    unsafe { f(this, key.as_ptr().cast(), out) }
}
