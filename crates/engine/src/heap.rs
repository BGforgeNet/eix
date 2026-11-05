//! A global allocator over the process heap, for a DLL built without std (the loader).
#![expect(unsafe_code, reason = "implementing GlobalAlloc over HeapAlloc")]

use core::alloc::{GlobalAlloc, Layout};
use core::ffi::c_void;

use windows_sys::Win32::System::Memory::{GetProcessHeap, HEAP_ZERO_MEMORY, HeapAlloc, HeapFree};

/// HeapAlloc's guaranteed alignment on 32-bit Windows.
const HEAP_ALIGN: usize = 8;
const WORD: usize = size_of::<usize>();

/// Allocates from the process heap. Alignments above 8 over-allocate and keep the heap block's address in the word
/// before the aligned pointer.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessHeap;

fn heap() -> *mut c_void {
    // SAFETY: no preconditions; the process heap lives as long as the process.
    unsafe { GetProcessHeap() }
}

/// Where an over-aligned block keeps its heap address: the word before the aligned pointer.
fn slot(aligned: *mut u8) -> *mut *mut u8 {
    (aligned as usize - WORD) as *mut *mut u8
}

// SAFETY: blocks come from HeapAlloc and go back to HeapFree on the same heap; over-aligned blocks are padded so the
// aligned pointer, the stored block address before it and `layout.size()` bytes after it all fit in the block.
unsafe impl GlobalAlloc for ProcessHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        alloc_with(layout, 0)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        alloc_with(layout, HEAP_ZERO_MEMORY)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let block = if layout.align() <= HEAP_ALIGN {
            ptr
        } else {
            // SAFETY: an over-aligned block stores its heap address in the word before `ptr` (alloc_with).
            unsafe { slot(ptr).read() }
        };
        // SAFETY: `block` came from HeapAlloc on this heap.
        unsafe { HeapFree(heap(), 0, block.cast()) };
    }
}

fn alloc_with(layout: Layout, flags: u32) -> *mut u8 {
    if layout.align() <= HEAP_ALIGN {
        // SAFETY: no preconditions beyond a valid heap handle.
        return unsafe { HeapAlloc(heap(), flags, layout.size()) }.cast();
    }
    // The aligned pointer lies at most `WORD + align - 1` bytes into the block.
    let Some(size) = layout
        .size()
        .checked_add(layout.align())
        .and_then(|s| s.checked_add(WORD))
    else {
        return core::ptr::null_mut();
    };
    // SAFETY: as above.
    let block = unsafe { HeapAlloc(heap(), flags, size) }.cast::<u8>();
    if block.is_null() {
        return block;
    }
    let aligned = (block as usize + WORD).next_multiple_of(layout.align()) as *mut u8;
    // SAFETY: the slot word and `layout.size()` bytes from `aligned` lie inside the `size`-byte block.
    unsafe { slot(aligned).write(block) };
    aligned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_aligned_blocks() {
        let heap = ProcessHeap;
        for align in [1, 8, 16, 64, 4096] {
            let layout = Layout::from_size_align(100, align).unwrap();
            // SAFETY: a non-zero layout; the block is freed with the same layout.
            let p = unsafe { heap.alloc_zeroed(layout) };
            assert!(!p.is_null());
            assert_eq!(p as usize % align, 0);
            // SAFETY: the block holds 100 zeroed bytes.
            let bytes = unsafe { core::slice::from_raw_parts(p, 100) };
            assert!(bytes.iter().all(|&b| b == 0));
            // SAFETY: same layout as allocated.
            unsafe { heap.dealloc(p, layout) };
        }
    }
}
