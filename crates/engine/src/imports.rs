//! Import address table slots of a loaded module: where it calls a function of another DLL through.
#![expect(
    unsafe_code,
    reason = "reading a loaded module's PE headers and import directory"
)]

use alloc::format;
use alloc::string::String;
use core::ffi::CStr;

use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

use crate::win::wide;

/// Reads a 32-bit little-endian word of a loaded image.
///
/// # Safety
/// `base + offset .. + 4` must lie in the mapped image.
unsafe fn word(base: usize, offset: usize) -> u32 {
    // SAFETY: the caller guarantees the four bytes are mapped; the read is unaligned-safe.
    unsafe { core::ptr::read_unaligned((base + offset) as *const u32) }
}

/// The import slot through which the loaded module `module` calls `function` of `dll` (`dll` compared without regard
/// to ASCII case, `function` exactly), and the address the slot holds. `None` when the module imports no such function by name.
pub fn slot(module: &str, dll: &str, function: &CStr) -> Result<Option<(usize, usize)>, String> {
    let name = wide(module);
    // SAFETY: `name` is NUL-terminated.
    let handle = unsafe { GetModuleHandleW(name.as_ptr()) };
    if handle.is_null() {
        return Err(format!("{module} is not loaded"));
    }
    let base = handle as usize;
    // SAFETY: a loaded module's image starts with its DOS header, whose `e_lfanew` (offset 0x3C) points at the NT
    // headers inside the image; everything below is read through the headers' own offsets and sizes.
    let nt = unsafe { word(base, 0x3C) } as usize;
    // SAFETY: as above.
    if unsafe { word(base, nt) } != 0x0000_4550 {
        return Err(format!("{module} has no PE signature"));
    }
    // The optional header follows the 4-byte signature and the 20-byte file header; for PE32 its data directories
    // start at 0x60, the import directory being the second.
    // SAFETY: as above.
    let imports = unsafe { word(base, nt + 24 + 0x60 + 8) } as usize;
    if imports == 0 {
        return Ok(None);
    }
    let mut descriptor = base + imports;
    loop {
        // SAFETY: the import directory is an array of 20-byte descriptors ended by a zeroed one.
        let lookup = unsafe { word(descriptor, 0) } as usize;
        // SAFETY: as above.
        let name_rva = unsafe { word(descriptor, 12) } as usize;
        // SAFETY: as above.
        let iat = unsafe { word(descriptor, 16) } as usize;
        if name_rva == 0 {
            return Ok(None);
        }
        // SAFETY: a descriptor's name is a NUL-terminated string in the image.
        let imported = unsafe { CStr::from_ptr((base + name_rva) as *const core::ffi::c_char) };
        if imported.to_bytes().eq_ignore_ascii_case(dll.as_bytes()) {
            // Bound images may lack the lookup table; the IAT then still holds the names before binding.
            let table = if lookup == 0 { iat } else { lookup };
            let mut i = 0;
            loop {
                // SAFETY: the lookup table is an array of 32-bit entries ended by a zero.
                let entry = unsafe { word(base, table + 4 * i) } as usize;
                if entry == 0 {
                    break;
                }
                // Entries with the top bit set import by ordinal; the others point at a hint and a name.
                if entry & 0x8000_0000 == 0 {
                    // SAFETY: a by-name entry points at a 2-byte hint followed by a NUL-terminated name.
                    let f =
                        unsafe { CStr::from_ptr((base + entry + 2) as *const core::ffi::c_char) };
                    if f == function {
                        let at = base + iat + 4 * i;
                        // SAFETY: the IAT has as many slots as the lookup table has entries.
                        let value = unsafe { word(at, 0) } as usize;
                        return Ok(Some((at, value)));
                    }
                }
                i += 1;
            }
        }
        descriptor += 20;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_test_binary_s_import_of_kernel32() {
        // win.rs calls GetLastError, so the test binary imports it; the loader filled its slot with the export.
        let kernel32 = crate::dll::Dll::system("kernel32.dll").unwrap();
        let get_last_error = kernel32.address(c"GetLastError").unwrap();
        // SAFETY: null asks for the main module: the test binary.
        let path = unsafe { crate::dll::module_path(core::ptr::null_mut()) }.unwrap();
        let name = crate::win::narrow(&path);
        let file = name.rsplit('\\').next().unwrap();
        let (at, value) = slot(file, "KERNEL32.dll", c"GetLastError")
            .unwrap()
            .unwrap();
        assert_eq!(value, get_last_error);
        // SAFETY: `at` is the slot just found, in the mapped image.
        assert_eq!(unsafe { word(at, 0) } as usize, value);
        assert_eq!(slot(file, "KERNEL32.dll", c"NoSuchFunction"), Ok(None));
        assert_eq!(slot(file, "no-such.dll", c"GetLastError"), Ok(None));
        assert!(slot("no-such-module.dll", "x.dll", c"f").is_err());
    }
}
