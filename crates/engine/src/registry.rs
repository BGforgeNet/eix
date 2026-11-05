//! Reading string values under `HKEY_CURRENT_USER`, where the starter keeps the loader chain's settings.
#![expect(unsafe_code, reason = "Win32 registry calls")]

use alloc::vec;
use alloc::vec::Vec;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, REG_DWORD, REG_EXPAND_SZ, REG_MULTI_SZ, REG_SZ, RegCloseKey,
    RegOpenKeyExW, RegQueryValueExW,
};

use crate::win::wide;

/// The string value `name` of `HKEY_CURRENT_USER\<key>`, as UTF-16 up to its first NUL.
///
/// Empty when the key or the value is missing, cannot be read, is empty or is not a string: the starter's loader
/// treats all of these as "not set". `REG_EXPAND_SZ` is returned as stored, unexpanded, as that loader does.
pub fn hkcu_string(key: &str, name: &str) -> Vec<u16> {
    let key = wide(key);
    let name = wide(name);
    let mut hkey: HKEY = core::ptr::null_mut();
    // SAFETY: `key` is NUL-terminated; `hkey` receives the opened key.
    let opened = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_READ, &mut hkey) };
    if opened != ERROR_SUCCESS {
        return Vec::new();
    }
    let value = query_string(hkey, &name);
    // Closing a key we opened for reading cannot lose data; there is nothing to do if it fails.
    // SAFETY: `hkey` was opened above and is closed once.
    unsafe { RegCloseKey(hkey) };
    value
}

/// A value of `HKEY_CURRENT_USER\<key>` as stored: its type and its bytes. `None` when the key or the value is
/// missing or cannot be read.
pub fn hkcu_value(key: &str, name: &str) -> Option<(u32, Vec<u8>)> {
    let key = wide(key);
    let name = wide(name);
    let mut hkey: HKEY = core::ptr::null_mut();
    // SAFETY: `key` is NUL-terminated; `hkey` receives the opened key.
    let opened = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_READ, &mut hkey) };
    if opened != ERROR_SUCCESS {
        return None;
    }
    let value = query_raw(hkey, &name);
    // SAFETY: `hkey` was opened above and is closed once.
    unsafe { RegCloseKey(hkey) };
    value
}

/// The `REG_DWORD` value `name` of `HKEY_CURRENT_USER\<key>`; `None` when missing or of another type.
pub fn hkcu_dword(key: &str, name: &str) -> Option<u32> {
    match hkcu_value(key, name)? {
        (REG_DWORD, bytes) if bytes.len() >= 4 => {
            Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
        _ => None,
    }
}

/// The `REG_MULTI_SZ` value `name` of `HKEY_CURRENT_USER\<key>`, one UTF-16 string per entry (the empty entry
/// that ends the list is not one); `None` when missing or of another type.
pub fn hkcu_multi_string(key: &str, name: &str) -> Option<Vec<Vec<u16>>> {
    let (ty, bytes) = hkcu_value(key, name)?;
    if ty != REG_MULTI_SZ {
        return None;
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    let mut out = Vec::new();
    for s in units.split(|&c| c == 0) {
        if s.is_empty() {
            break;
        }
        out.push(s.to_vec());
    }
    Some(out)
}

fn query_raw(hkey: HKEY, name: &[u16]) -> Option<(u32, Vec<u8>)> {
    let mut ty = 0;
    let mut size = 0u32;
    // SAFETY: `name` is NUL-terminated; a null data pointer asks for the size only.
    let sized = unsafe {
        RegQueryValueExW(
            hkey,
            name.as_ptr(),
            core::ptr::null(),
            &mut ty,
            core::ptr::null_mut(),
            &mut size,
        )
    };
    if sized != ERROR_SUCCESS {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    // SAFETY: `buf` holds `size` bytes.
    let read = unsafe {
        RegQueryValueExW(
            hkey,
            name.as_ptr(),
            core::ptr::null(),
            &mut ty,
            buf.as_mut_ptr(),
            &mut size,
        )
    };
    if read != ERROR_SUCCESS {
        return None;
    }
    buf.truncate(size as usize);
    Some((ty, buf))
}

fn query_string(hkey: HKEY, name: &[u16]) -> Vec<u16> {
    let mut ty = 0;
    let mut size = 0u32;
    // SAFETY: `name` is NUL-terminated; a null data pointer asks for the size only.
    let sized = unsafe {
        RegQueryValueExW(
            hkey,
            name.as_ptr(),
            core::ptr::null(),
            &mut ty,
            core::ptr::null_mut(),
            &mut size,
        )
    };
    if sized != ERROR_SUCCESS || size == 0 {
        return Vec::new();
    }
    // Whole UTF-16 units; an odd byte count gets a spare byte that stays zero.
    let mut buf = vec![0u16; (size as usize).div_ceil(2)];
    // SAFETY: `buf` holds at least `size` bytes.
    let read = unsafe {
        RegQueryValueExW(
            hkey,
            name.as_ptr(),
            core::ptr::null(),
            &mut ty,
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if read != ERROR_SUCCESS || (ty != REG_SZ && ty != REG_EXPAND_SZ) {
        return Vec::new();
    }
    buf.truncate((size as usize).div_ceil(2));
    if let Some(nul) = buf.iter().position(|&c| c == 0) {
        buf.truncate(nul);
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_key_reads_as_empty() {
        assert!(hkcu_string("Software\\eix\\no such key", "AddonPath").is_empty());
    }

    #[test]
    fn a_missing_value_reads_as_empty() {
        // HKCU\Software exists in every profile.
        assert!(hkcu_string("Software", "eix: no such value").is_empty());
    }

    #[test]
    fn reads_strings_and_skips_other_types() {
        use windows_sys::Win32::System::Registry::{
            REG_DWORD, RegCreateKeyW, RegDeleteKeyW, RegSetValueExW,
        };
        const KEY: &str = "Software\\eix-test-registry";
        let key = wide(KEY);
        let mut hkey: HKEY = core::ptr::null_mut();
        // SAFETY: `key` is NUL-terminated; `hkey` receives the created key.
        let created = unsafe { RegCreateKeyW(HKEY_CURRENT_USER, key.as_ptr(), &mut hkey) };
        assert_eq!(created, ERROR_SUCCESS);
        let set = |name: &str, ty, data: &[u8]| {
            let name = wide(name);
            let len = u32::try_from(data.len()).unwrap();
            // SAFETY: `name` is NUL-terminated; `data` has `len` bytes.
            let r = unsafe { RegSetValueExW(hkey, name.as_ptr(), 0, ty, data.as_ptr(), len) };
            assert_eq!(r, ERROR_SUCCESS);
        };
        let bytes = |w: &[u16]| w.iter().flat_map(|c| c.to_le_bytes()).collect::<Vec<u8>>();
        set("sz", REG_SZ, &bytes(&wide("C:\\Mods\\inv")));
        // Stored without its terminating NUL, as some writers do.
        set(
            "bare",
            REG_SZ,
            &bytes(&"C:\\x".encode_utf16().collect::<Vec<_>>()),
        );
        set("expand", REG_EXPAND_SZ, &bytes(&wide("%TEMP%\\m")));
        set("dword", REG_DWORD, &1u32.to_le_bytes());
        let read = |name| alloc::string::String::from_utf16(&hkcu_string(KEY, name)).unwrap();
        assert_eq!(read("sz"), "C:\\Mods\\inv");
        assert_eq!(read("bare"), "C:\\x");
        assert_eq!(read("expand"), "%TEMP%\\m");
        assert_eq!(read("dword"), "");
        assert_eq!(hkcu_dword(KEY, "dword"), Some(1));
        assert_eq!(hkcu_dword(KEY, "sz"), None);
        set("multi", REG_MULTI_SZ, &bytes(&wide("a\0bc\0")));
        let multi = hkcu_multi_string(KEY, "multi").unwrap();
        assert_eq!(multi, [wide("a")[..1].to_vec(), wide("bc")[..2].to_vec()]);
        assert_eq!(hkcu_multi_string(KEY, "dword"), None);
        assert_eq!(hkcu_value(KEY, "no such value"), None);
        // SAFETY: `hkey` was opened above and is closed once.
        unsafe { RegCloseKey(hkey) };
        // SAFETY: `key` is NUL-terminated.
        unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, key.as_ptr()) };
    }
}
