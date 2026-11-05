//! What a bug report needs to know about the process's surroundings: the Windows version (and Wine's, under Wine)
//! and the loaded modules, each with the size and link time stamp that tell its build.
#![expect(
    unsafe_code,
    reason = "Win32 calls and reading the loaded modules' PE headers"
)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{CStr, c_char};

use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW, TH32CS_SNAPMODULE,
    TH32CS_SNAPMODULE32,
};
use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;

use crate::dll::Dll;
use crate::win::os_error;

/// A loaded module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// Its file, UTF-16 without a NUL.
    pub path: Vec<u16>,
    /// Where it is mapped.
    pub base: usize,
    /// Its image size.
    pub size: usize,
    /// The link time stamp of its PE header; none when the header cannot be read.
    pub time_stamp: Option<u32>,
}

/// The Windows version as the system reports it, unaffected by compatibility settings (`RtlGetVersion`), and Wine's
/// version under Wine: `Windows 10.0.19045` or `Windows 10.0.19045, Wine 10.0`.
pub fn system() -> String {
    type RtlGetVersion = unsafe extern "system" fn(*mut OSVERSIONINFOW) -> i32;
    type WineGetVersion = unsafe extern "C" fn() -> *const c_char;
    let Ok(ntdll) = Dll::system("ntdll.dll") else {
        return os_error("ntdll.dll");
    };
    // SAFETY: ntdll's export of this type.
    let rtl = unsafe { ntdll.proc::<RtlGetVersion>(c"RtlGetVersion") };
    // SAFETY: Wine's ntdll export of this type; Windows' ntdll has none.
    let wine = unsafe { ntdll.proc::<WineGetVersion>(c"wine_get_version") };
    let mut text = match rtl {
        Some(get) => {
            // SAFETY: a plain-data struct, valid when zeroed.
            let mut info: OSVERSIONINFOW = unsafe { core::mem::zeroed() };
            info.dwOSVersionInfoSize = size_of::<OSVERSIONINFOW>() as u32;
            // SAFETY: `info` is a valid OSVERSIONINFOW with its size set.
            match unsafe { get(&raw mut info) } {
                0 => format!(
                    "Windows {}.{}.{}",
                    info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber
                ),
                status => format!("Windows, version unknown: RtlGetVersion 0x{status:08X}"),
            }
        }
        None => String::from("Windows, version unknown: no RtlGetVersion"),
    };
    if let Some(get) = wine {
        // SAFETY: Wine's export returns its static, NUL-terminated version text.
        let version = unsafe { get() };
        if !version.is_null() {
            // SAFETY: as above.
            let version = unsafe { CStr::from_ptr(version) };
            text.push_str(&format!(", Wine {}", version.to_string_lossy()));
        }
    }
    text
}

/// The modules loaded in this process, in the system's order.
pub fn modules() -> Result<Vec<Module>, String> {
    // SAFETY: a snapshot of this process (0); no pointer is passed in.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(os_error("CreateToolhelp32Snapshot"));
    }
    let mut out = Vec::new();
    // SAFETY: a plain-data struct, valid when zeroed.
    let mut entry: MODULEENTRY32W = unsafe { core::mem::zeroed() };
    entry.dwSize = size_of::<MODULEENTRY32W>() as u32;
    // SAFETY: `snapshot` is open and `entry` has its size set.
    let mut more = unsafe { Module32FirstW(snapshot, &raw mut entry) } != 0;
    while more {
        let path = &entry.szExePath;
        let len = path.iter().position(|&c| c == 0).unwrap_or(path.len());
        let base = entry.modBaseAddr as usize;
        let size = entry.modBaseSize as usize;
        out.push(Module {
            path: path[..len].to_vec(),
            base,
            size,
            // SAFETY: the module is loaded at `base` with `size` bytes mapped, as the snapshot reports.
            time_stamp: unsafe { time_stamp(base, size) },
        });
        // SAFETY: as for the first entry.
        more = unsafe { Module32NextW(snapshot, &raw mut entry) } != 0;
    }
    // SAFETY: the snapshot is ours and closed once.
    unsafe { CloseHandle(snapshot) };
    Ok(out)
}

/// The `TimeDateStamp` of the PE image at `base`; none when its headers do not fit `size` or lack the signatures.
///
/// # Safety
/// `base..base + size` must be mapped and readable.
unsafe fn time_stamp(base: usize, size: usize) -> Option<u32> {
    let read = |offset: usize| -> Option<u32> {
        (offset.checked_add(4)? <= size).then(|| {
            // SAFETY: the 4 bytes lie inside the mapped range, as the caller guarantees.
            unsafe { core::ptr::read_unaligned((base + offset) as *const u32) }
        })
    };
    // "MZ", then `e_lfanew` at 0x3C pointing at "PE\0\0"; `TimeDateStamp` follows 8 bytes into the PE header.
    if read(0)? & 0xFFFF != 0x5A4D {
        return None;
    }
    let pe = read(0x3C)? as usize;
    if read(pe)? != 0x0000_4550 {
        return None;
    }
    read(pe.checked_add(8)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_names_its_version() {
        assert!(system().starts_with("Windows "), "{}", system());
        assert!(!system().contains("unknown"), "{}", system());
    }

    #[test]
    fn the_modules_include_kernel32_with_its_time_stamp() {
        let modules = modules().unwrap();
        let kernel32 = modules
            .iter()
            .find(|m| {
                crate::win::narrow(&m.path)
                    .to_ascii_lowercase()
                    .ends_with("kernel32.dll")
            })
            .unwrap();
        assert!(kernel32.size > 0);
        assert!(kernel32.time_stamp.is_some());
    }
}
