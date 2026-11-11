//! The console: its command maps, the line executor, its output stream and its window.
//!
//! Names follow ei-rebuild.
#![expect(unsafe_code, reason = "calls into game.exe's console code")]

use core::ffi::CStr;
use core::mem::{MaybeUninit, offset_of};

use super::mfc::{self, GameArray, GameString};
use super::{Global, code};

/// A console command: fastcall, the arguments (lower case) in ECX. `Fps` 0x5E0830 reads the count at ECX+8.
pub type CommandHandler = unsafe extern "fastcall" fn(args: *mut GameArray<GameString>);

/// An STLport `map<CString, CommandHandler>`; only the game's functions touch its nodes.
#[repr(C)]
#[derive(Debug)]
pub struct CommandMap {
    _opaque: [u8; 0xC],
}

/// The commands always available: [`execute`] looks a line up here after [`CHEAT_COMMANDS`] (0x5DC7CA).
pub const PUBLIC_COMMANDS: Global<CommandMap> = Global::at(eix_sites::DATA_7C7B28);
/// The commands after the cheat word: [`execute`] looks a line up here first (0x5DC65A) and registers the game's
/// commands in it (0x5DD31C).
pub const CHEAT_COMMANDS: Global<CommandMap> = Global::at(eix_sites::DATA_7C7B38);

/// One of the game's old-iostream `ostream`s; only its functions touch it.
#[repr(C)]
#[derive(Debug)]
pub struct Ostream {
    _opaque: [u8; 8],
}

/// The console's output stream: its static initialiser constructs it on the console's stream buffer 0x7C5868 with
/// filter 0 (0x5DC5E0).
pub const CONSOLE_OUT: Global<Ostream> = Global::at(eix_sites::DATA_7C55A8);

/// Runs one console line, as typed: `Execute` 0x5DC620 (fastcall, the text in ECX, plain `ret`).
///
/// # Safety
/// The process must be a supported game.exe, on the game's thread, with the console's statics constructed.
pub unsafe fn execute(line: &CStr) {
    // SAFETY: 0x5DC620 takes the text in ECX, never reads EDX before writing it, and returns with a plain `ret`.
    let f = unsafe { code::<unsafe extern "fastcall" fn(*const u8)>(eix_sites::TEXT_5DC620) };
    // SAFETY: the caller guarantees the build, the thread and the statics; `line` is NUL-terminated.
    unsafe { f(line.as_ptr().cast()) }
}

/// The handler slot for `name` in `map`, inserted empty when missing: `Commands::Subscript` 0x5E0B20 (thiscall, the
/// key's `CString*` on the stack, `ret 4`).
///
/// # Safety
/// As for [`execute`]; `map` must be [`PUBLIC_COMMANDS`] or [`CHEAT_COMMANDS`] and `name` a constructed string.
pub unsafe fn command_slot(
    map: *mut CommandMap,
    name: *const GameString,
) -> *mut Option<CommandHandler> {
    // SAFETY: 0x5E0B20 is `CommandHandler& __thiscall (const CString&)`.
    let f = unsafe {
        code::<
            unsafe extern "thiscall" fn(
                *mut CommandMap,
                *const GameString,
            ) -> *mut Option<CommandHandler>,
        >(eix_sites::COMMANDS_SUBSCRIPT)
    };
    // SAFETY: the caller guarantees the map, the key and the thread.
    unsafe { f(map, name) }
}

/// Registers `handler` as the command `name` (lower case) in `map`, as the game registers its own (0x5DD31C..).
///
/// # Safety
/// As for [`command_slot`]; `handler` must be a [`CommandHandler`] that stays valid while the game runs.
pub unsafe fn register_command(map: *mut CommandMap, name: &CStr, handler: CommandHandler) {
    let mut key = MaybeUninit::<GameString>::uninit();
    // SAFETY: `key` is writable and holds no string yet; the caller guarantees the build.
    unsafe { mfc::construct(key.as_mut_ptr(), name) };
    // SAFETY: `key` is now a constructed string; the caller guarantees the map.
    let slot = unsafe { command_slot(map, key.as_ptr()) };
    // SAFETY: the function returns the map's live value slot for the key.
    unsafe { slot.write(Some(handler)) };
    // SAFETY: `key` is a constructed string, not used again; the map copied it.
    unsafe { mfc::destroy(key.as_mut_ptr()) };
}

/// Writes `text` and a line break to the console and flushes it, as the game ends its console messages:
/// `Ostream::PutText` 0x6FC31D, `PutChar` 0x6FC0B0 (both thiscall, `ret 4`) and `Flush` 0x6FC346 (plain `ret`).
///
/// # Safety
/// As for [`execute`].
pub unsafe fn print_line(text: &CStr) {
    let out = CONSOLE_OUT.as_ptr();
    // SAFETY: 0x6FC31D is `ostream& __thiscall operator<<(const char*)`.
    let put_text = unsafe {
        code::<unsafe extern "thiscall" fn(*mut Ostream, *const u8) -> *mut Ostream>(
            eix_sites::OSTREAM_PUT_TEXT,
        )
    };
    // SAFETY: 0x6FC0B0 is `ostream& __thiscall operator<<(char)`; it reads the low byte of its stack argument.
    let put_char = unsafe {
        code::<unsafe extern "thiscall" fn(*mut Ostream, u32) -> *mut Ostream>(
            eix_sites::TEXT_6FC0B0,
        )
    };
    // SAFETY: 0x6FC346 is `ostream& __thiscall flush()`.
    let flush = unsafe {
        code::<unsafe extern "thiscall" fn(*mut Ostream) -> *mut Ostream>(
            eix_sites::LIBCRT_OSTREAM_FLUSH,
        )
    };
    // SAFETY: the caller guarantees the build, the thread and the constructed stream; `text` is NUL-terminated.
    unsafe { put_text(out, text.as_ptr().cast()) };
    // SAFETY: as above.
    unsafe { put_char(out, u32::from(b'\n')) };
    // SAFETY: as above.
    unsafe { flush(out) };
}

/// `ConsoleWindow`: the console's window, kept by the interface ([`super::ui::Interface::console`]).
#[repr(C)]
#[derive(Debug)]
pub struct ConsoleWindow {
    _unknown0: [u8; 0xC],
    /// Commands entered, newest first: `ConsoleWindow::OnChar` 0x5E44A0 inserts the input at 0 on Enter.
    pub history: GameArray<GameString>,
    _unknown20: [u8; 0x24 - 0x20],
    /// The line being typed (0x5E44A0).
    pub input: GameString,
    /// The caret's byte position: `ConsoleWindow::DrawInput` 0x5E4A10 keeps it within the input.
    pub caret: i32,
    /// The selection's other end, the caret's position when nothing is selected (0x5E4A10).
    pub anchor: i32,
    _unknown30: [u8; 0x58 - 0x30],
    /// The lines shown: `ConsoleWindow::AddText` 0x5E4090 continues the last one.
    pub lines: GameArray<GameString>,
    _unknown6c: [u8; 0x78 - 0x6C],
    /// Ctrl is held: `OnKeyDown` 0x5E4180 sets it on 0x11, `OnKeyUp` 0x5E47E0 clears it.
    pub ctrl: u8,
    /// Shift is held: set on 0x10 (0x5E4180), cleared (0x5E47E0).
    pub shift: u8,
    /// The window is shown: `ConsoleWindow::Show` 0x5E3FD0 stores its argument.
    pub shown: u8,
}

const _: () = {
    assert!(offset_of!(ConsoleWindow, history) == 0xC);
    assert!(offset_of!(ConsoleWindow, input) == 0x24);
    assert!(offset_of!(ConsoleWindow, caret) == 0x28);
    assert!(offset_of!(ConsoleWindow, anchor) == 0x2C);
    assert!(offset_of!(ConsoleWindow, lines) == 0x58);
    assert!(offset_of!(ConsoleWindow, ctrl) == 0x78);
    assert!(offset_of!(ConsoleWindow, shift) == 0x79);
    assert!(offset_of!(ConsoleWindow, shown) == 0x7A);
};

/// Adds `text` to the window, continuing its last line; a `\n` starts a new one: `ConsoleWindow::AddText` 0x5E4090
/// (thiscall, `ret 4`).
///
/// # Safety
/// As for [`execute`]; `window` must be the interface's live console window.
pub unsafe fn add_text(window: *mut ConsoleWindow, text: &CStr) {
    // SAFETY: 0x5E4090 is `void __thiscall (const char*)`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut ConsoleWindow, *const u8)>(
            eix_sites::CONSOLE_WINDOW_ADD_TEXT,
        )
    };
    // SAFETY: the caller guarantees the window and the thread; `text` is NUL-terminated.
    unsafe { f(window, text.as_ptr().cast()) }
}

/// Redraws the input line, first keeping the caret and the anchor within it: `ConsoleWindow::DrawInput` 0x5E4A10
/// (thiscall, plain `ret`).
///
/// # Safety
/// As for [`add_text`].
pub unsafe fn draw_input(window: *mut ConsoleWindow) {
    // SAFETY: 0x5E4A10 is `void __thiscall ()`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut ConsoleWindow)>(
            eix_sites::CONSOLE_WINDOW_DRAW_INPUT,
        )
    };
    // SAFETY: the caller guarantees the window and the thread.
    unsafe { f(window) }
}
