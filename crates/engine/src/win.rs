//! Small Win32 helpers without std: the last error, wide and ANSI strings, local time, files, error boxes, critical
//! sections, ending the process.
#![expect(
    unsafe_code,
    reason = "GetLastError, the code page conversions, GetLocalTime, the file calls, MessageBoxW, the critical sections, the process calls, timeBeginPeriod and the cursor clip"
)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::fmt;

use windows_sys::Win32::Foundation::{
    GetLastError, GlobalFree, HWND, INVALID_HANDLE_VALUE, POINT, RECT, SYSTEMTIME,
};
use windows_sys::Win32::Globalization::{CP_ACP, MultiByteToWideChar, WideCharToMultiByte};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::Media::{TIMERR_NOERROR, timeBeginPeriod};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FindClose, FindFirstFileW, FindNextFileW, GetFileAttributesW,
    INVALID_FILE_ATTRIBUTES, WIN32_FIND_DATAW, WriteFile,
};
use windows_sys::Win32::System::Console::{GetStdHandle, STD_OUTPUT_HANDLE};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::LibraryLoader::DisableThreadLibraryCalls;
use windows_sys::Win32::System::Memory::{
    GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::Threading::{
    CRITICAL_SECTION, EnterCriticalSection, ExitProcess, GetCurrentProcess, LeaveCriticalSection,
    TerminateProcess,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CharLowerA, CharUpperA, ClipCursor, GetClientRect, GetForegroundWindow, MB_ICONERROR, MB_OK,
    MessageBoxW, wvsprintfA,
};

/// Asks for a system timer resolution of `ms` milliseconds for this process (`timeBeginPeriod`), kept until it ends;
/// false when the system refuses the period.
pub fn raise_timer_resolution(ms: u32) -> bool {
    // SAFETY: no preconditions; the period lasts until a matching `timeEndPeriod` or the end of the process.
    unsafe { timeBeginPeriod(ms) == TIMERR_NOERROR }
}

/// `<what>: OS error <code>` for the calling thread's last error.
pub fn os_error(what: &str) -> String {
    // SAFETY: no preconditions.
    let code = unsafe { GetLastError() };
    format!("{what}: OS error {code}")
}

/// `s` as UTF-16 with a terminating NUL.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// UTF-16 (no NUL) as a String; unpaired surrogates become U+FFFD.
pub fn narrow(w: &[u16]) -> String {
    String::from_utf16_lossy(w)
}

/// Bytes in the system ANSI code page as UTF-8 text for logs and Unicode messages.
pub fn ansi_text(bytes: &[u8]) -> String {
    narrow(&from_ansi(bytes))
}

/// UTF-16 (no NUL) in the ANSI code page, which game.exe's strings use; unmappable characters become the code
/// page's default character.
pub fn to_ansi(w: &[u16]) -> Vec<u8> {
    if w.is_empty() {
        return Vec::new();
    }
    let Ok(len) = i32::try_from(w.len()) else {
        return Vec::new();
    };
    // SAFETY: `w` has `len` units; null output asks for the size.
    let size = unsafe {
        WideCharToMultiByte(
            CP_ACP,
            0,
            w.as_ptr(),
            len,
            core::ptr::null_mut(),
            0,
            core::ptr::null(),
            core::ptr::null_mut(),
        )
    };
    let Ok(n) = usize::try_from(size) else {
        return Vec::new();
    };
    let mut out = alloc::vec![0u8; n];
    // SAFETY: `out` holds the `size` bytes asked for.
    let written = unsafe {
        WideCharToMultiByte(
            CP_ACP,
            0,
            w.as_ptr(),
            len,
            out.as_mut_ptr(),
            size,
            core::ptr::null(),
            core::ptr::null_mut(),
        )
    };
    out.truncate(usize::try_from(written).unwrap_or(0));
    out
}

/// ANSI code page bytes (no NUL) as UTF-16.
pub fn from_ansi(a: &[u8]) -> Vec<u16> {
    if a.is_empty() {
        return Vec::new();
    }
    let Ok(len) = i32::try_from(a.len()) else {
        return Vec::new();
    };
    // SAFETY: `a` has `len` bytes; null output asks for the size.
    let size = unsafe { MultiByteToWideChar(CP_ACP, 0, a.as_ptr(), len, core::ptr::null_mut(), 0) };
    let Ok(n) = usize::try_from(size) else {
        return Vec::new();
    };
    let mut out = alloc::vec![0u16; n];
    // SAFETY: `out` holds the `size` units asked for.
    let written =
        unsafe { MultiByteToWideChar(CP_ACP, 0, a.as_ptr(), len, out.as_mut_ptr(), size) };
    out.truncate(usize::try_from(written).unwrap_or(0));
    out
}

/// `format` filled from `args` by user32's `wvsprintfA`, which writes at most 1024 bytes: the C formatter a
/// game-side printf hook needs (no floats, as in that function).
///
/// # Safety
/// `format` is a NUL-terminated string and `args` points at the arguments its conversions read, as a `va_list` does
/// on 32-bit x86: each argument in turn on the stack.
pub unsafe fn wvsprintf(format: *const u8, args: *const c_void) -> Vec<u8> {
    let mut buf = alloc::vec![0u8; 1025];
    // SAFETY: `buf` has room for wvsprintfA's 1024 bytes and the NUL; the caller guarantees format and arguments.
    let len = unsafe { wvsprintfA(buf.as_mut_ptr(), format, args.cast()) };
    buf.truncate(usize::try_from(len).unwrap_or(0).min(1024));
    buf
}

/// The local date and time: year, month, day, hour, minute, second, millisecond.
pub fn local_time() -> [u16; 7] {
    let mut t = SYSTEMTIME {
        wYear: 0,
        wMonth: 0,
        wDayOfWeek: 0,
        wDay: 0,
        wHour: 0,
        wMinute: 0,
        wSecond: 0,
        wMilliseconds: 0,
    };
    // SAFETY: `t` is a SYSTEMTIME the call fills.
    unsafe { GetLocalTime(&mut t) };
    [
        t.wYear,
        t.wMonth,
        t.wDay,
        t.wHour,
        t.wMinute,
        t.wSecond,
        t.wMilliseconds,
    ]
}

/// Whether `path` (UTF-16, no NUL) names an existing file that is not a directory. A path whose attributes cannot be
/// read counts as absent.
pub fn file_exists(path: &[u16]) -> bool {
    let w: Vec<u16> = path.iter().copied().chain([0]).collect();
    // SAFETY: `w` is NUL-terminated.
    let attributes = unsafe { GetFileAttributesW(w.as_ptr()) };
    attributes != INVALID_FILE_ATTRIBUTES && attributes & FILE_ATTRIBUTE_DIRECTORY == 0
}

/// Text formatted into a fixed UTF-16 buffer, NUL-terminated; what does not fit is cut.
struct WideBuf<const N: usize> {
    buf: [u16; N],
    len: usize,
}

impl<const N: usize> WideBuf<N> {
    fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }

    /// The text with its NUL.
    fn as_nul_terminated(&self) -> &[u16] {
        &self.buf[..=self.len]
    }
}

impl<const N: usize> fmt::Write for WideBuf<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.encode_utf16() {
            // The last unit stays the NUL.
            if self.len + 1 >= N {
                break;
            }
            self.buf[self.len] = c;
            self.len += 1;
        }
        Ok(())
    }
}

fn format_wide<const N: usize>(text: impl fmt::Display) -> WideBuf<N> {
    let mut buf = WideBuf::new();
    // Writing to a WideBuf never fails; a Display impl that errors leaves what it wrote so far.
    let _ = fmt::write(&mut buf, format_args!("{text}"));
    buf
}

/// Shows a modal error box with no owner window and waits for it to be closed.
///
/// It formats into fixed buffers and allocates nothing, so a panic handler can use it. Text past 2047 UTF-16 units
/// is cut.
pub fn error_box(title: &str, text: impl fmt::Display) {
    let title = format_wide::<256>(title);
    let text = format_wide::<2048>(text);
    // The box only reports; which button closed it does not matter.
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        MessageBoxW(
            core::ptr::null_mut(),
            text.as_nul_terminated().as_ptr(),
            title.as_nul_terminated().as_ptr(),
            MB_OK | MB_ICONERROR,
        )
    };
}

/// Ends the process through `ExitProcess`: DLLs get their detach notifications.
pub fn exit_process(code: u32) -> ! {
    // SAFETY: no preconditions.
    unsafe { ExitProcess(code) }
}

/// Runs `f` inside the critical section at `section`: entered before, left after.
///
/// # Safety
/// `section` must be an initialised `CRITICAL_SECTION` that outlives the call.
pub unsafe fn in_critical_section<R>(section: *mut c_void, f: impl FnOnce() -> R) -> R {
    let section = section.cast::<CRITICAL_SECTION>();
    // SAFETY: the caller guarantees an initialised section.
    unsafe { EnterCriticalSection(section) };
    let result = f();
    // SAFETY: entered above on this thread; a panic aborts, so nothing skips this.
    unsafe { LeaveCriticalSection(section) };
    result
}

/// Ends the process at once, running no more code in it: the end of a panic.
pub fn terminate_process(code: u32) -> ! {
    // SAFETY: a pseudo-handle to our own process; no preconditions.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: terminating our own process is always allowed and does not return when it succeeds.
    unsafe { TerminateProcess(process, code) };
    // Reached only if termination failed; ExitProcess does not return either.
    exit_process(code)
}

/// `text` lowered (`upper` false) or raised in place by user32's `CharLowerA` / `CharUpperA`, which follow the
/// user's code page; the result ends at the first NUL.
pub fn change_case(text: &[u8], upper: bool) -> Vec<u8> {
    let mut buf: Vec<u8> = text.iter().copied().chain([0]).collect();
    let convert = if upper { CharUpperA } else { CharLowerA };
    // SAFETY: `buf` is NUL-terminated and writable; a pointer above 0xFFFF makes both work on the string in place.
    unsafe { convert(buf.as_mut_ptr()) };
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    buf.truncate(end);
    buf
}

/// `CF_UNICODETEXT`.
const UNICODE_TEXT: u32 = 13;

/// Puts `text` (UTF-16, no NUL) on the clipboard as Unicode text, owned by the calling thread's active window.
/// Returns false when there is no active window or the clipboard cannot be had.
pub fn set_clipboard_text(text: &[u16]) -> bool {
    let bytes = (text.len() + 1) * 2;
    // SAFETY: a moveable, zeroed block of `bytes`.
    let block = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes) };
    if block.is_null() {
        return false;
    }
    // SAFETY: `block` is a live moveable block.
    let at = unsafe { GlobalLock(block) }.cast::<u16>();
    if at.is_null() {
        // SAFETY: the block is ours and not handed on.
        unsafe { GlobalFree(block) };
        return false;
    }
    // SAFETY: the block holds `text.len() + 1` units; its last stays the zeroed NUL.
    unsafe { core::ptr::copy_nonoverlapping(text.as_ptr(), at, text.len()) };
    // SAFETY: locked above; the result only says whether it is still locked.
    unsafe { GlobalUnlock(block) };
    let open = |owner| {
        // SAFETY: a stale handle only makes the call fail.
        unsafe { OpenClipboard(owner) != 0 }
    };
    let empty = || {
        // SAFETY: called only while the clipboard is open.
        unsafe { EmptyClipboard() };
    };
    let publish = |block| {
        // SAFETY: the clipboard is open; the block is a live, unlocked moveable allocation.
        unsafe { SetClipboardData(UNICODE_TEXT, block) }
    };
    let close = || {
        // SAFETY: called only after opening the clipboard.
        unsafe { CloseClipboard() };
    };
    // SAFETY: no preconditions; a thread without an active window gets null.
    let owner = unsafe { GetActiveWindow() };
    // SAFETY: the block is ours; publication transfers ownership only on success.
    unsafe { with_clipboard_owner(owner, block, open, empty, publish, close) }
}

/// # Safety
/// `block` is an owned, unlocked GlobalAlloc block; `publish` takes ownership only when it returns non-null.
unsafe fn with_clipboard_owner(
    owner: HWND,
    block: *mut c_void,
    open: impl FnOnce(HWND) -> bool,
    empty: impl FnOnce(),
    publish: impl FnOnce(*mut c_void) -> *mut c_void,
    close: impl FnOnce(),
) -> bool {
    if owner.is_null() || !open(owner) {
        // SAFETY: the clipboard never took ownership of this unlocked block.
        unsafe { GlobalFree(block) };
        return false;
    }
    empty();
    // SAFETY: the block is ours; `publish` transfers ownership only on success.
    let set = unsafe { publish_clipboard_block(block, publish) };
    close();
    set
}

/// # Safety
/// `block` is an owned, unlocked GlobalAlloc block; `publish` takes ownership only when it returns non-null.
unsafe fn publish_clipboard_block(
    block: *mut c_void,
    publish: impl FnOnce(*mut c_void) -> *mut c_void,
) -> bool {
    let set = !publish(block).is_null();
    if !set {
        // SAFETY: the publisher refused ownership; the block is still ours and unlocked.
        unsafe { GlobalFree(block) };
    }
    set
}

/// # Safety
/// `at` is aligned for UTF-16 and readable for `bytes` bytes while this call lasts.
unsafe fn clipboard_units(at: *const u16, bytes: usize) -> Vec<u16> {
    let mut out = Vec::new();
    for i in 0..bytes / size_of::<u16>() {
        // SAFETY: only whole UTF-16 units within the readable block are visited.
        let c = unsafe { *at.wrapping_add(i) };
        if c == 0 {
            break;
        }
        out.push(c);
    }
    out
}

/// The clipboard's Unicode text (no NUL), empty when there is none or it cannot be had.
pub fn clipboard_text() -> Vec<u16> {
    // SAFETY: no owner window.
    if unsafe { OpenClipboard(core::ptr::null_mut()) } == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    // SAFETY: the clipboard is open.
    let data = unsafe { GetClipboardData(UNICODE_TEXT) };
    if !data.is_null() {
        // SAFETY: the clipboard's Unicode text is a live moveable block.
        let at = unsafe { GlobalLock(data) }.cast::<u16>();
        if !at.is_null() {
            // SAFETY: the clipboard owns this live global block.
            let bytes = unsafe { GlobalSize(data) };
            // SAFETY: the locked block is aligned for UTF-16 and readable for its GlobalSize.
            out = unsafe { clipboard_units(at, bytes) };
            // SAFETY: locked above.
            unsafe { GlobalUnlock(data) };
        }
    }
    // SAFETY: opened above.
    unsafe { CloseClipboard() };
    out
}

/// The window `hwnd` (a handle's value) as the API's handle type.
fn window(hwnd: usize) -> HWND {
    core::ptr::with_exposed_provenance_mut(hwnd)
}

/// Confines the cursor to the client area of the window `hwnd`; false when the window has no area or the call fails.
pub fn clip_cursor_to_client(hwnd: usize) -> bool {
    let mut area = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: `area` is a live RECT; a stale handle only makes the call fail.
    if unsafe { GetClientRect(window(hwnd), &mut area) } == 0 || area.right <= 0 || area.bottom <= 0
    {
        return false;
    }
    let mut corners = [
        POINT {
            x: area.left,
            y: area.top,
        },
        POINT {
            x: area.right,
            y: area.bottom,
        },
    ];
    for corner in &mut corners {
        // SAFETY: `corner` is a live POINT.
        if unsafe { ClientToScreen(window(hwnd), corner) } == 0 {
            return false;
        }
    }
    let screen = RECT {
        left: corners[0].x,
        top: corners[0].y,
        right: corners[1].x,
        bottom: corners[1].y,
    };
    // SAFETY: `screen` is a live RECT.
    unsafe { ClipCursor(&screen) != 0 }
}

/// Lets the cursor go (`ClipCursor(NULL)`).
pub fn release_cursor_clip() {
    // SAFETY: a null rectangle removes the clip.
    unsafe { ClipCursor(core::ptr::null()) };
}

/// Whether `hwnd` (a handle's value) is the foreground window.
pub fn is_foreground(hwnd: usize) -> bool {
    // SAFETY: no preconditions.
    unsafe { GetForegroundWindow() == window(hwnd) }
}

/// Writes `bytes` as they are to the process's standard output handle, as the C runtime's `printf` reaches it; false
/// when there is no such handle or the write fails (a GUI process started without redirection has none).
pub fn write_stdout(bytes: &[u8]) -> bool {
    // SAFETY: no preconditions.
    let handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return false;
    }
    let mut rest = bytes;
    while !rest.is_empty() {
        let chunk = u32::try_from(rest.len()).unwrap_or(u32::MAX);
        let mut written = 0u32;
        // SAFETY: `rest` holds at least `chunk` bytes; `written` is a valid out pointer; no overlapped I/O.
        let ok = unsafe {
            WriteFile(
                handle,
                rest.as_ptr(),
                chunk,
                &raw mut written,
                core::ptr::null_mut(),
            )
        };
        if ok == 0 || written == 0 {
            return false;
        }
        rest = &rest[written as usize..];
    }
    true
}

/// One directory entry found by [`find_files`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundFile {
    /// Its name (UTF-16, no NUL), without the folder.
    pub name: Vec<u16>,
    /// It is a directory.
    pub directory: bool,
}

/// The entries `FindFirstFileW` / `FindNextFileW` list for `pattern` (UTF-16, no NUL; wildcards allowed), in their
/// order; `None` when the first call fails (nothing matches, or the folder does not exist).
pub fn find_files(pattern: &[u16]) -> Option<Vec<FoundFile>> {
    let w: Vec<u16> = pattern.iter().copied().chain([0]).collect();
    // SAFETY: an all-zero WIN32_FIND_DATAW is a valid value of the plain struct.
    let mut data: WIN32_FIND_DATAW = unsafe { core::mem::zeroed() };
    // SAFETY: `w` is NUL-terminated; `data` is a valid out struct.
    let find = unsafe { FindFirstFileW(w.as_ptr(), &raw mut data) };
    if find == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut out = Vec::new();
    loop {
        let name = &data.cFileName;
        let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        out.push(FoundFile {
            name: name[..len].to_vec(),
            directory: data.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0,
        });
        // SAFETY: `find` is the live search handle; `data` is a valid out struct.
        if unsafe { FindNextFileW(find, &raw mut data) } == 0 {
            break;
        }
    }
    // SAFETY: the live search handle, not used afterwards.
    unsafe { FindClose(find) };
    Some(out)
}

/// Stops thread attach and detach calls to a DLL's entry point.
///
/// # Safety
/// `module` is the handle of a DLL loaded in this process (DllMain's `hinstDLL`).
pub unsafe fn disable_thread_library_calls(module: *mut c_void) {
    // It fails only for a DLL with static TLS, which then keeps getting the calls; that changes nothing for it.
    // SAFETY: the caller guarantees the handle.
    unsafe { DisableThreadLibraryCalls(module) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_text_stops_at_the_block_boundary() {
        let storage = [65, 66, 0];
        for (bytes, expected) in [
            (0, &[][..]),
            (1, &[][..]),
            (2, &[65][..]),
            (3, &[65][..]),
            (4, &[65, 66][..]),
            (6, &[65, 66][..]),
        ] {
            // SAFETY: storage is readable and NUL-terminated; bytes names the accessible prefix.
            let text = unsafe { clipboard_units(storage.as_ptr(), bytes) };
            assert_eq!(text, expected, "{bytes} bytes");
        }
        let terminated = [65, 0, 66];
        // SAFETY: all three units are readable.
        assert_eq!(unsafe { clipboard_units(terminated.as_ptr(), 6) }, [65]);
    }

    #[test]
    fn clipboard_publication_uses_the_window_and_closes_after_publication() {
        use core::cell::RefCell;

        for accepted in [true, false] {
            let calls = RefCell::new(Vec::new());
            // SAFETY: a fresh moveable allocation.
            let block = unsafe { GlobalAlloc(GMEM_MOVEABLE, 4) };
            assert!(!block.is_null());
            let open = |owner| {
                calls.borrow_mut().push(("open", owner as usize));
                true
            };
            let empty = || calls.borrow_mut().push(("empty", 0));
            let publish = |owned| {
                calls.borrow_mut().push(("publish", 0));
                assert_eq!(owned, block);
                if accepted {
                    owned
                } else {
                    core::ptr::null_mut()
                }
            };
            let close = || calls.borrow_mut().push(("close", 0));
            // SAFETY: the block is ours; the publisher retains it only on success, until freed below.
            let set =
                unsafe { with_clipboard_owner(window(42), block, open, empty, publish, close) };
            // SAFETY: GlobalSize returns zero if the refused block was freed.
            let left = unsafe { GlobalSize(block) };
            if left != 0 {
                // SAFETY: the test owns the accepted block, or a block not freed after refusal.
                unsafe { GlobalFree(block) };
            }
            assert_eq!(set, accepted);
            assert_eq!(left != 0, accepted);
            assert_eq!(
                calls.into_inner(),
                [("open", 42), ("empty", 0), ("publish", 0), ("close", 0)]
            );
        }
    }

    #[test]
    fn clipboard_publication_without_an_owner_does_not_open_or_empty() {
        use core::cell::RefCell;

        let calls = RefCell::new(Vec::new());
        // SAFETY: a fresh moveable allocation.
        let block = unsafe { GlobalAlloc(GMEM_MOVEABLE, 4) };
        assert!(!block.is_null());
        let open = |_| {
            calls.borrow_mut().push("open");
            true
        };
        let empty = || calls.borrow_mut().push("empty");
        let publish = |_| {
            calls.borrow_mut().push("publish");
            core::ptr::null_mut()
        };
        let close = || calls.borrow_mut().push("close");
        // SAFETY: the block is ours and the publisher never takes ownership.
        let set = unsafe {
            with_clipboard_owner(core::ptr::null_mut(), block, open, empty, publish, close)
        };
        // SAFETY: GlobalSize returns zero for a freed block.
        let left = unsafe { GlobalSize(block) };
        if left != 0 {
            // SAFETY: a block that was not freed remains ours.
            unsafe { GlobalFree(block) };
        }
        assert!(!set);
        assert_eq!(left, 0);
        assert!(calls.into_inner().is_empty());
    }

    #[test]
    fn refused_clipboard_blocks_are_freed() {
        // SAFETY: a fresh moveable allocation.
        let block = unsafe { GlobalAlloc(GMEM_MOVEABLE, 4) };
        assert!(!block.is_null());
        // SAFETY: the publisher refuses the block and takes no ownership.
        let set = unsafe { publish_clipboard_block(block, |_| core::ptr::null_mut()) };
        // SAFETY: GlobalSize returns zero for an invalid global handle.
        let left = unsafe { GlobalSize(block) };
        if left != 0 {
            // SAFETY: a refused block still owned here, if it was not freed.
            unsafe { GlobalFree(block) };
        }
        assert!(!set);
        assert_eq!(left, 0);

        // SAFETY: a fresh moveable allocation.
        let block = unsafe { GlobalAlloc(GMEM_MOVEABLE, 4) };
        assert!(!block.is_null());
        // SAFETY: this publisher accepts ownership and leaves the block live until freed below.
        let set = unsafe { publish_clipboard_block(block, |owned| owned) };
        // SAFETY: GlobalSize returns zero if the handle was wrongly freed.
        let left = unsafe { GlobalSize(block) };
        // SAFETY: ownership passed to this test's publisher; GlobalFree also accepts an invalid handle.
        unsafe { GlobalFree(block) };
        assert!(set);
        assert!(left >= 4);
    }

    #[test]
    fn round_trips_wide_strings() {
        let w = wide("C:\\Mods\\inv");
        assert_eq!(w.last(), Some(&0));
        assert_eq!(narrow(&w[..w.len() - 1]), "C:\\Mods\\inv");
    }

    #[test]
    fn converts_ascii_to_ansi_and_back() {
        let w: Vec<u16> = "C:\\Mods\\inv\\config.reg".encode_utf16().collect();
        let a = to_ansi(&w);
        assert_eq!(a, b"C:\\Mods\\inv\\config.reg");
        assert_eq!(from_ansi(&a), w);
        assert!(to_ansi(&[]).is_empty() && from_ansi(&[]).is_empty());
        let args: [usize; 2] = [42, c"x".as_ptr() as usize];
        // SAFETY: the format is NUL-terminated and reads an int and a string, which `args` holds in order.
        let text = unsafe { wvsprintf(c"%d-%s".as_ptr().cast(), args.as_ptr().cast()) };
        assert_eq!(text, b"42-x");
        let [year, month, day, ..] = local_time();
        assert!(year >= 2000 && (1..=12).contains(&month) && (1..=31).contains(&day));
    }

    #[test]
    fn formats_into_a_fixed_buffer_and_cuts_the_rest() {
        let short = format_wide::<8>(format_args!("{}-{}", 1, 2));
        assert_eq!(short.as_nul_terminated(), &[49, 45, 50, 0]);
        let cut = format_wide::<4>("abcdef");
        assert_eq!(cut.as_nul_terminated(), &[97, 98, 99, 0]);
    }

    #[test]
    fn tells_files_from_directories() {
        // SAFETY: null asks for the main module: the test binary.
        let exe = unsafe { crate::dll::module_path(core::ptr::null_mut()) }.unwrap();
        assert!(file_exists(&exe));
        let sep = exe.iter().rposition(|&c| c == u16::from(b'\\')).unwrap();
        assert!(!file_exists(&exe[..sep]));
        let mut missing = exe.clone();
        missing.extend(".missing".encode_utf16());
        assert!(!file_exists(&missing));
    }
}
