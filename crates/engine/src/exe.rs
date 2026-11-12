//! Which game.exe build this process runs, read from its PE header; the same fields of any loaded module.
#![expect(unsafe_code, reason = "reading a loaded module's PE header")]

use alloc::format;
use alloc::string::String;

use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

use crate::win::os_error;

/// The PE header fields that tell game.exe builds apart.
///
/// A file hash would not do: the starter's exe and GOG RU differ only in the CD-check byte and share one address
/// table, and a hash of the loaded image changes with every patch applied before us.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExeId {
    /// Load address of the image.
    pub base: usize,
    /// `AddressOfEntryPoint` (an RVA).
    pub entry_rva: u32,
    /// `SizeOfImage`.
    pub size_of_image: u32,
    /// The linker's `TimeDateStamp`.
    pub time_date_stamp: u32,
}

impl ExeId {
    /// The entry point's address in memory.
    pub fn entry(&self) -> usize {
        self.base + self.entry_rva as usize
    }
}

pub use eix_sites::Build;

/// The build whose header fields `id` has. GOG's exe and the official 1.06 patch's differ only in a code byte, not
/// in these fields.
pub fn identify(id: &ExeId) -> Option<Build> {
    if id.base != eix_sites::IMAGE_BASE.ru() {
        return None;
    }
    match (id.entry_rva, id.size_of_image, id.time_date_stamp) {
        (0x2E_C226, 0x3D_7000, 0x3ABF_0DB3) => Some(Build::Ru106),
        (0x2E_C1E6, 0x3D_7000, 0x3ABB_4A08) => Some(Build::En106),
        _ => None,
    }
}

/// Header fields of the process's main module.
pub fn current() -> Result<ExeId, String> {
    // SAFETY: a null name asks for the main module, which is always loaded.
    let base = unsafe { GetModuleHandleW(core::ptr::null()) } as usize;
    if base == 0 {
        return Err(os_error("no main module"));
    }
    // SAFETY: the main module is loaded at `base`.
    unsafe { module(base) }
}

/// Header fields of the module loaded at `base` (a module handle).
///
/// # Safety
/// `base` must be the base of a module loaded in this process.
pub unsafe fn module(base: usize) -> Result<ExeId, String> {
    let read = |offset: usize| -> u32 {
        // SAFETY: every offset read below lies inside the mapped headers of the module (DOS header, PE signature,
        // file header, optional header), which the loader maps readable.
        unsafe { core::ptr::read_unaligned((base + offset) as *const u32) }
    };
    let pe = read(0x3C) as usize;
    if read(pe) != 0x0000_4550 {
        return Err(format!("the module at {base:#x} has no PE signature"));
    }
    // File header follows the 4-byte signature; the optional header follows its 20 bytes.
    let optional = pe + 4 + 20;
    Ok(ExeId {
        base,
        entry_rva: read(optional + 16),
        size_of_image: read(optional + 56),
        time_date_stamp: read(pe + 4 + 4),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_ru_106() {
        let id = ExeId {
            base: eix_sites::IMAGE_BASE.ru(),
            entry_rva: 0x2E_C226,
            size_of_image: 0x3D_7000,
            time_date_stamp: 0x3ABF_0DB3,
        };
        assert_eq!(identify(&id), Some(Build::Ru106));
        assert_eq!(id.entry(), eix_sites::ENTRY.ru());
        let gog_en = ExeId {
            entry_rva: 0x2E_C1E6,
            time_date_stamp: 0x3ABB_4A08,
            ..id
        };
        assert_eq!(identify(&gog_en), Some(Build::En106));
        let unknown = ExeId {
            entry_rva: 0x2E_C1E6,
            ..id
        };
        assert_eq!(identify(&unknown), None);
    }

    #[test]
    fn reads_the_test_binarys_own_header() {
        let id = current().unwrap();
        assert_ne!(id.entry_rva, 0);
        assert!(id.size_of_image > id.entry_rva);
    }
}
