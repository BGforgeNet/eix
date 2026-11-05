//! Executable memory for hook stubs and trampolines: one read-write-execute pool, never freed.
#![expect(
    unsafe_code,
    reason = "allocating executable memory and writing code into it"
)]

use alloc::vec::Vec;

use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE, VirtualAlloc,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use crate::lock::SpinLock;

/// Size of each block the pool takes from the system: VirtualAlloc's allocation granularity.
const BLOCK: usize = 0x1_0000;
/// Placed code starts at this alignment.
const ALIGN: usize = 16;

/// The block code is placed in: next free address and the block's end.
struct Pool {
    next: usize,
    end: usize,
}

/// Hooks may be installed from any thread.
static POOL: SpinLock<Pool> = SpinLock::new(Pool { next: 0, end: 0 });

/// Reserves `len` bytes of executable memory, aligned to 16 bytes.
fn reserve(len: usize) -> Result<usize, alloc::string::String> {
    POOL.with(|pool| {
        let at = pool.next.next_multiple_of(ALIGN);
        if pool.end != 0 && at + len <= pool.end {
            pool.next = at + len;
            return Ok(at);
        }
        let size = len.next_multiple_of(BLOCK);
        // SAFETY: a fresh allocation; no pointer is passed in.
        let block = unsafe {
            VirtualAlloc(
                core::ptr::null(),
                size,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_EXECUTE_READWRITE,
            )
        };
        if block.is_null() {
            return Err(crate::win::os_error("VirtualAlloc"));
        }
        let at = block as usize;
        // The rest of the old block stays unused: blocks are never freed, and stubs are small.
        pool.next = at + len;
        pool.end = at + size;
        Ok(at)
    })
}

/// Places code of length `len` in the pool. `build` gets the address the code will run at and returns its bytes.
///
/// If `build` fails, or returns a length other than `len`, the reserved bytes stay unused.
pub fn place<E: From<alloc::string::String>>(
    len: usize,
    build: impl FnOnce(usize) -> Result<Vec<u8>, E>,
) -> Result<usize, E> {
    let at = reserve(len)?;
    let code = build(at)?;
    if code.len() != len {
        return Err(alloc::format!("code for {at:#x} is {} bytes, not {len}", code.len()).into());
    }
    // SAFETY: `at..at + len` was reserved for this call alone and is writable; `code` has that length.
    unsafe { core::ptr::copy_nonoverlapping(code.as_ptr(), at as *mut u8, len) };
    // SAFETY: returns a pseudo-handle to our own process; no preconditions.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: flushing a range of our own process is always allowed.
    unsafe { FlushInstructionCache(process, at as *const _, len) };
    crate::owners::stub(at, len);
    Ok(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placed_code_runs() {
        // mov eax, 42; ret
        let at = place::<alloc::string::String>(6, |_| Ok(vec![0xB8, 42, 0, 0, 0, 0xC3])).unwrap();
        assert_eq!(at % ALIGN, 0);
        // SAFETY: `at` holds a complete cdecl function without arguments returning in EAX.
        let f: extern "C" fn() -> u32 = unsafe { core::mem::transmute(at) };
        assert_eq!(f(), 42);
    }

    #[test]
    fn large_requests_get_their_own_block() {
        let len = BLOCK + 1;
        let at = place::<alloc::string::String>(len, |_| Ok(vec![0xCC; len])).unwrap();
        // SAFETY: the whole range was just written.
        assert_eq!(unsafe { *((at + len - 1) as *const u8) }, 0xCC);
    }
}
