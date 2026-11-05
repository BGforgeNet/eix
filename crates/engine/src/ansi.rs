//! Win32 calls with byte strings in the ANSI code page, for code that must see what an ANSI caller of game.exe's era
//! sees: INI reads through `GetPrivateProfile*A`, `MessageBoxA` and `AddFontResourceExA`.
#![expect(
    unsafe_code,
    reason = "GetPrivateProfileIntA, GetPrivateProfileStringA, MessageBoxA and AddFontResourceExA"
)]

use alloc::vec;
use alloc::vec::Vec;

use windows_sys::Win32::Graphics::Gdi::{AddFontResourceExA, FR_PRIVATE};
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntA, GetPrivateProfileStringA,
};
use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxA;

/// `s` with a NUL appended, cut at an inner NUL as the API would read it.
fn c(s: &[u8]) -> Vec<u8> {
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    let mut v = Vec::with_capacity(end + 1);
    v.extend_from_slice(&s[..end]);
    v.push(0);
    v
}

/// `GetPrivateProfileIntA(section, key, default, file)`, its unsigned result as the signed value it holds.
pub fn profile_int(section: &[u8], key: &[u8], default: i32, file: &[u8]) -> i32 {
    let (section, key, file) = (c(section), c(key), c(file));
    // SAFETY: the three strings are NUL-terminated and outlive the call.
    let v =
        unsafe { GetPrivateProfileIntA(section.as_ptr(), key.as_ptr(), default, file.as_ptr()) };
    v as i32
}

/// `GetPrivateProfileStringA(section, key, default, buffer, size, file)` into a buffer of `size` bytes filled with
/// `fill` first: the whole buffer afterwards, so the bytes after the string's NUL are `fill` as the caller set them.
pub fn profile_string(
    section: &[u8],
    key: &[u8],
    default: &[u8],
    size: usize,
    fill: u8,
    file: &[u8],
) -> Vec<u8> {
    let (section, key, default, file) = (c(section), c(key), c(default), c(file));
    let mut buf = vec![fill; size];
    let Ok(len) = u32::try_from(size) else {
        return buf;
    };
    // SAFETY: the strings are NUL-terminated, `buf` has `len` bytes, and all outlive the call.
    unsafe {
        GetPrivateProfileStringA(
            section.as_ptr(),
            key.as_ptr(),
            default.as_ptr(),
            buf.as_mut_ptr(),
            len,
            file.as_ptr(),
        )
    };
    buf
}

/// `MessageBoxA(NULL, text, caption, style)`: a modal box with no owner window; returns the button that closed it.
pub fn message_box(text: &[u8], caption: &[u8], style: u32) -> i32 {
    let (text, caption) = (c(text), c(caption));
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        MessageBoxA(
            core::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            style,
        )
    }
}

/// `AddFontResourceExA(path, FR_PRIVATE, NULL)`: makes the font file at `path` available to this process only;
/// returns the number of fonts added, 0 on failure.
pub fn add_private_font(path: &[u8]) -> i32 {
    let path = c(path);
    // SAFETY: the path is NUL-terminated and outlives the call; the reserved argument must be null.
    unsafe { AddFontResourceExA(path.as_ptr(), FR_PRIVATE, core::ptr::null()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_an_ini_file() {
        let dir = std::env::temp_dir().join(format!("eix-ansi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("TEST.INI");
        std::fs::write(&path, "[gameplay]\r\nflag=256\r\nneg=-3\r\nname=abc\r\n").unwrap();
        let file = path.to_str().unwrap().as_bytes();
        assert_eq!(profile_int(b"gameplay", b"flag", 7, file), 256);
        assert_eq!(profile_int(b"GAMEPLAY", b"neg", 7, file), -3);
        assert_eq!(profile_int(b"gameplay", b"missing", 7, file), 7);
        let buf = profile_string(b"gameplay", b"name", b"dflt", 8, 0xCC, file);
        assert_eq!(buf, b"abc\0\xCC\xCC\xCC\xCC");
        let buf = profile_string(b"gameplay", b"missing", b"dflt", 8, 0xCC, file);
        assert_eq!(&buf[..5], b"dflt\0");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
