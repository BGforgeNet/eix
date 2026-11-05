//! Loaded DLLs and their exports: the system's own copy of a DLL we stand in for (dinput.dll), and addon.dll.
#![expect(
    unsafe_code,
    reason = "Win32 calls and casting a resolved export to its function type"
)]

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::CStr;

use windows_sys::Win32::Foundation::{GetLastError, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleFileNameW, GetModuleHandleExW, GetModuleHandleW, GetProcAddress, LoadLibraryW,
};

use crate::win::{narrow, os_error, wide};

/// A loaded DLL. It is never freed: the exports resolved from it stay valid for the process's life.
#[derive(Debug)]
pub struct Dll {
    module: HMODULE,
    path: String,
}

impl Dll {
    /// The system's `name`: the copy in kernel32.dll's directory, already loaded or loaded now.
    ///
    /// kernel32.dll's directory is the system directory of the process's own bitness (SysWOW64 for a 32-bit
    /// process on 64-bit Windows), so no file system redirection is involved.
    pub fn system(name: &str) -> Result<Self, String> {
        let kernel32 = wide("kernel32.dll");
        // SAFETY: `kernel32` is NUL-terminated.
        let module = unsafe { GetModuleHandleW(kernel32.as_ptr()) };
        if module.is_null() {
            return Err(os_error("kernel32.dll is not loaded"));
        }
        // SAFETY: `module` is kernel32.dll's handle, which stays loaded.
        let mut path = unsafe { module_path(module) }?;
        let Some(sep) = path.iter().rposition(|&c| c == u16::from(b'\\')) else {
            return Err(format!("{} has no directory", narrow(&path)));
        };
        path.truncate(sep + 1);
        path.extend(name.encode_utf16());
        let w = with_nul(&path);
        // SAFETY: `w` is NUL-terminated.
        let loaded = unsafe { GetModuleHandleW(w.as_ptr()) };
        let module = if loaded.is_null() {
            // SAFETY: as above.
            unsafe { LoadLibraryW(w.as_ptr()) }
        } else {
            loaded
        };
        if module.is_null() {
            return Err(os_error(&format!("cannot load {}", narrow(&path))));
        }
        Ok(Self {
            module,
            path: narrow(&path),
        })
    }

    /// Loads the DLL at `path` (UTF-16, no NUL). The error is the system's error code.
    pub fn load(path: &[u16]) -> Result<Self, u32> {
        let w = with_nul(path);
        // SAFETY: `w` is NUL-terminated.
        let module = unsafe { LoadLibraryW(w.as_ptr()) };
        if module.is_null() {
            // SAFETY: no preconditions.
            return Err(unsafe { GetLastError() });
        }
        Ok(Self {
            module,
            path: narrow(path),
        })
    }

    /// The DLL at `path` (UTF-16, no NUL): the module already loaded under that name, else loaded now. The error is
    /// the system's error code.
    pub fn open(path: &[u16]) -> Result<Self, u32> {
        let w = with_nul(path);
        // SAFETY: `w` is NUL-terminated.
        let loaded = unsafe { GetModuleHandleW(w.as_ptr()) };
        if loaded.is_null() {
            return Self::load(path);
        }
        Ok(Self {
            module: loaded,
            path: narrow(path),
        })
    }

    /// The module's handle (its load address): two `Dll`s with one handle are the same module.
    pub fn handle(&self) -> usize {
        self.module as usize
    }

    /// The file the DLL was loaded from, as it was asked for.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The address of an export, if the DLL has it.
    pub fn address(&self, export: &CStr) -> Option<usize> {
        // SAFETY: the module is loaded and never freed; `export` is NUL-terminated.
        let found = unsafe { GetProcAddress(self.module, export.as_ptr().cast()) };
        found.map(|f| f as usize)
    }

    /// An export as `F`, if the DLL has it.
    ///
    /// # Safety
    /// `F` must be the export's real function pointer type.
    pub unsafe fn proc<F: Copy>(&self, export: &CStr) -> Option<F> {
        const { assert!(size_of::<F>() == size_of::<usize>()) };
        let found = self.address(export)?;
        // SAFETY: the caller guarantees `F` is this export's type; both are pointer-sized.
        Some(unsafe { core::mem::transmute_copy(&found) })
    }
}

fn with_nul(w: &[u16]) -> Vec<u16> {
    w.iter().copied().chain([0]).collect()
}

/// The file a loaded module was loaded from, as UTF-16 without a NUL.
///
/// # Safety
/// `module` is the handle of a module loaded in this process (DllMain's `hinstDLL`), or null for the main module.
pub unsafe fn module_path(module: HMODULE) -> Result<Vec<u16>, String> {
    let mut buf = vec![0u16; 1024];
    let capacity = u32::try_from(buf.len()).map_err(|_| String::from("buffer too large"))?;
    // SAFETY: the buffer and its length match; the caller guarantees the handle.
    let len = unsafe { GetModuleFileNameW(module, buf.as_mut_ptr(), capacity) } as usize;
    if len == 0 || len >= buf.len() {
        return Err(os_error("GetModuleFileNameW"));
    }
    buf.truncate(len);
    Ok(buf)
}

/// The file of the module this code is linked into (addon.dll for the addon), as UTF-16 without a NUL.
pub fn own_path() -> Result<Vec<u16>, String> {
    // The address of this function lies in the module that holds this code.
    let module = module_holding(own_path as *const () as usize)?;
    // SAFETY: the handle of a module loaded in this process.
    unsafe { module_path(module) }
}

/// The module whose image holds `address`: its file (UTF-16, no NUL) and its base address; none for an address
/// outside every module.
pub fn module_at(address: usize) -> Option<(Vec<u16>, usize)> {
    let module = module_holding(address).ok()?;
    // SAFETY: the handle of a module loaded in this process.
    let path = unsafe { module_path(module) }.ok()?;
    Some((path, module as usize))
}

/// The handle of the module whose image holds `address`, its count unchanged.
fn module_holding(address: usize) -> Result<HMODULE, String> {
    let mut module: HMODULE = core::ptr::null_mut();
    // SAFETY: FROM_ADDRESS takes `address` as an address, inside a module or not; UNCHANGED_REFCOUNT keeps the
    // module's count; `module` is a valid out pointer.
    let found = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            core::ptr::with_exposed_provenance::<u16>(address),
            &raw mut module,
        )
    };
    if found == 0 {
        return Err(os_error("GetModuleHandleExW"));
    }
    Ok(module)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_system_dinput() {
        let dll = Dll::system("dinput.dll").unwrap();
        assert!(dll.path().to_ascii_lowercase().ends_with("\\dinput.dll"));
        assert!(dll.address(c"DirectInputCreateA").is_some());
        assert!(dll.address(c"NoSuchExport").is_none());
    }

    #[test]
    fn reports_the_error_code_of_a_missing_dll() {
        let path: Vec<u16> = "C:\\no\\such\\addon.dll".encode_utf16().collect();
        let code = Dll::load(&path).unwrap_err();
        assert_ne!(code, 0);
    }
}
