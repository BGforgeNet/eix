//! HeapPadding: every block game.exe's C runtime takes from the heap is made larger, so the game's writes past the end
//! of its blocks land in bytes it owns instead of the next block's header. game.exe's import slots of `HeapAlloc` and
//! `HeapReAlloc` get stand-ins that add the padding to the size and change nothing else.
//!
//! Every path stays consistent without bookkeeping: the block's address is the heap's own, so `HeapFree` (left as it
//! is) frees padded blocks and those made before the fix alike, and a realloc pads whichever it gets. `_msize`
//! (`HeapSize`) reports the padded size; its three callers in game.exe take it as the room the block has, which it
//! is. Other modules call the heap through their own imports and are not affected.
#![expect(
    unsafe_code,
    reason = "reading game.exe's import slots and calling the heap functions they held"
)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use eix_core::heap_padding::padded;
use eix_engine::game::crt::{
    ACTIVE_HEAP, HEAP_ALLOC_SLOT, HEAP_RE_ALLOC_SLOT, HeapAlloc, HeapReAlloc, SYSTEM_HEAP,
};
use eix_engine::hook::as_fn_at;

use crate::groups::{BuildError, Labelled, pointer};
use crate::log;
use crate::setup::Setup;

/// What game.exe's `HeapAlloc` slot held: the function [`heap_alloc`] calls.
static ALLOC: AtomicUsize = AtomicUsize::new(0);
/// What game.exe's `HeapReAlloc` slot held: the function [`heap_re_alloc`] calls.
static RE_ALLOC: AtomicUsize = AtomicUsize::new(0);

/// Stands in for `HeapAlloc`: the same call with the padded size.
extern "system" fn heap_alloc(heap: *mut c_void, flags: u32, size: usize) -> *mut c_void {
    // SAFETY: ALLOC holds what the import slot held, `HeapAlloc` or a function of its type, stored before the slot
    // was pointed here.
    let f: HeapAlloc = unsafe { as_fn_at(ALLOC.load(Ordering::SeqCst)) };
    // SAFETY: the caller's arguments; a larger size asks the heap for no more than any caller may.
    unsafe { f(heap, flags, padded(size)) }
}

/// Stands in for `HeapReAlloc`: the same call with the padded size.
extern "system" fn heap_re_alloc(
    heap: *mut c_void,
    flags: u32,
    block: *mut c_void,
    size: usize,
) -> *mut c_void {
    // SAFETY: as in `heap_alloc`, for RE_ALLOC and `HeapReAlloc`.
    let f: HeapReAlloc = unsafe { as_fn_at(RE_ALLOC.load(Ordering::SeqCst)) };
    // SAFETY: as in `heap_alloc`; the block is the caller's.
    unsafe { f(heap, flags, block, padded(size)) }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    // SAFETY: the build is a supported one, whose import slots the Windows loader filled and whose C runtime chose its heap
    // before WinMain; this runs on the game's thread before WinMain.
    let alloc = unsafe { HEAP_ALLOC_SLOT.read() };
    // SAFETY: as above.
    let re_alloc = unsafe { HEAP_RE_ALLOC_SLOT.read() };
    // SAFETY: as above.
    let mode = unsafe { ACTIVE_HEAP.read() };
    if mode != SYSTEM_HEAP {
        // The C runtime makes this choice when the system reports a version before Windows 2000.
        log::warn(
            "fixes",
            &format!(
                "the C runtime uses its own small-block heap (mode {mode}); its blocks get no padding"
            ),
            "HeapPadding",
        );
    }
    ALLOC.store(alloc, Ordering::SeqCst);
    RE_ALLOC.store(re_alloc, Ordering::SeqCst);
    Ok(vec![
        pointer(
            "HeapPadding HeapAlloc",
            HEAP_ALLOC_SLOT.site(),
            alloc,
            heap_alloc as *const () as usize,
        ),
        pointer(
            "HeapPadding HeapReAlloc",
            HEAP_RE_ALLOC_SLOT.site(),
            re_alloc,
            heap_re_alloc as *const () as usize,
        ),
    ])
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use std::ptr::null_mut;

    use eix_core::heap_padding::padding;
    use eix_engine::dll::Dll;

    type HeapCreate = unsafe extern "system" fn(u32, usize, usize) -> *mut c_void;
    type HeapDestroy = unsafe extern "system" fn(*mut c_void) -> i32;
    type HeapFree = unsafe extern "system" fn(*mut c_void, u32, *mut c_void) -> i32;
    type HeapSize = unsafe extern "system" fn(*mut c_void, u32, *const c_void) -> usize;
    type HeapValidate = unsafe extern "system" fn(*mut c_void, u32, *const c_void) -> i32;

    const HEAP_ZERO_MEMORY: u32 = 0x08;

    /// Held while a test points [`ALLOC`] and [`RE_ALLOC`] at kernel32's functions and calls through them: a build of
    /// the fix, in another test, stores what this process holds at the game's import slots there.
    pub(crate) static SLOTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// kernel32's export `name`, as the function type `F` the caller names for it.
    fn function<F: Copy>(k: &Dll, name: &std::ffi::CStr) -> F {
        // SAFETY: every caller names a kernel32 heap function with its stdcall signature.
        unsafe { as_fn_at(k.address(name).unwrap()) }
    }

    /// kernel32's heap functions, and the stand-ins pointed at the real ones as the fix points them.
    struct Heap {
        _slots: std::sync::MutexGuard<'static, ()>,
        heap: *mut c_void,
        alloc: HeapAlloc,
        free: HeapFree,
        size: HeapSize,
        validate: HeapValidate,
        destroy: HeapDestroy,
    }

    impl Heap {
        fn new() -> Self {
            let slots = SLOTS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let k = Dll::system("kernel32.dll").unwrap();
            let get = |name| k.address(name).unwrap();
            ALLOC.store(get(c"HeapAlloc"), Ordering::SeqCst);
            RE_ALLOC.store(get(c"HeapReAlloc"), Ordering::SeqCst);
            let create: HeapCreate = function(&k, c"HeapCreate");
            Self {
                _slots: slots,
                // A private growable heap, as the C runtime's `_crtheap`.
                // SAFETY: no preconditions.
                heap: unsafe { create(0, 0x1000, 0) },
                alloc: function(&k, c"HeapAlloc"),
                free: function(&k, c"HeapFree"),
                size: function(&k, c"HeapSize"),
                validate: function(&k, c"HeapValidate"),
                destroy: function(&k, c"HeapDestroy"),
            }
        }

        fn size(&self, block: *mut c_void) -> usize {
            // SAFETY: a live block of this heap.
            unsafe { (self.size)(self.heap, 0, block) }
        }

        fn free(&self, block: *mut c_void) {
            // SAFETY: a live block of this heap.
            assert_ne!(unsafe { (self.free)(self.heap, 0, block) }, 0);
        }

        fn valid(&self) -> bool {
            // SAFETY: the heap is live; null checks all of it.
            unsafe { (self.validate)(self.heap, 0, null_mut()) != 0 }
        }

        /// Fills `len` bytes of `block` with `byte`.
        fn fill(block: *mut c_void, len: usize, byte: u8) {
            // SAFETY: the caller passes a block of at least `len` bytes.
            unsafe { block.cast::<u8>().write_bytes(byte, len) };
        }

        fn holds(block: *mut c_void, len: usize, byte: u8) -> bool {
            // SAFETY: as in `fill`.
            unsafe { std::slice::from_raw_parts(block.cast::<u8>(), len) }
                .iter()
                .all(|&b| b == byte)
        }
    }

    impl Drop for Heap {
        fn drop(&mut self) {
            // SAFETY: the heap was created by `new` and is not used afterwards.
            unsafe { (self.destroy)(self.heap) };
        }
    }

    #[test]
    fn blocks_are_padded_through_alloc_realloc_and_free() {
        let h = Heap::new();
        // A figure object's block, written up to its padding's end as an overrun would.
        let small = heap_alloc(h.heap, 0, 656);
        assert!(!small.is_null());
        assert_eq!(h.size(small), 656 + 64);
        Heap::fill(small, 656 + 64, 0x11);
        assert!(h.valid());
        // Grown past 1 KB: the contents move along, the padding follows the new size.
        let large = heap_re_alloc(h.heap, 0, small, 4096);
        assert!(!large.is_null());
        assert_eq!(h.size(large), 4096 + 512);
        assert!(Heap::holds(large, 656 + 64, 0x11));
        Heap::fill(large, 4096 + 512, 0x22);
        assert!(h.valid());
        // Shrunk again.
        let shrunk = heap_re_alloc(h.heap, 0, large, 1024);
        assert_eq!(h.size(shrunk), 1024 + 256);
        assert!(Heap::holds(shrunk, 1024 + 256, 0x22));
        h.free(shrunk);
        assert!(h.valid());
    }

    #[test]
    fn a_block_made_before_the_fix_reallocs_and_frees() {
        let h = Heap::new();
        // SAFETY: the real HeapAlloc on a live heap.
        let old = unsafe { (h.alloc)(h.heap, 0, 100) };
        assert_eq!(h.size(old), 100);
        Heap::fill(old, 100, 0x33);
        let moved = heap_re_alloc(h.heap, 0, old, 200);
        assert_eq!(h.size(moved), 200 + 64);
        assert!(Heap::holds(moved, 100, 0x33));
        h.free(moved);
        // SAFETY: as above.
        let kept = unsafe { (h.alloc)(h.heap, 0, 100) };
        // Freed as made, unpadded.
        h.free(kept);
        assert!(h.valid());
    }

    #[test]
    fn zeroed_blocks_are_zeroed_with_their_padding() {
        let h = Heap::new();
        let block = heap_alloc(h.heap, HEAP_ZERO_MEMORY, 2000);
        assert_eq!(h.size(block), 2000 + padding(2000));
        assert!(Heap::holds(block, 2000 + padding(2000), 0));
        h.free(block);
    }

    #[test]
    fn a_size_no_heap_can_give_fails() {
        let h = Heap::new();
        assert!(heap_alloc(h.heap, 0, usize::MAX - 8).is_null());
        let block = heap_alloc(h.heap, 0, 16);
        assert!(heap_re_alloc(h.heap, 0, block, usize::MAX - 8).is_null());
        // The block is left as it was.
        assert_eq!(h.size(block), 16 + 64);
        h.free(block);
        assert!(h.valid());
    }

    #[test]
    fn threads_share_the_padded_heap() {
        let h = Heap::new();
        let heap = h.heap as usize;
        let threads: Vec<_> = (0..4)
            .map(|t| {
                std::thread::spawn(move || {
                    let heap = heap as *mut c_void;
                    let mut blocks = Vec::new();
                    for i in 0..500usize {
                        let size = 16 * (1 + (i * 7 + t) % 200);
                        let b = heap_alloc(heap, 0, size);
                        assert!(!b.is_null());
                        Heap::fill(b, padded(size), t as u8);
                        blocks.push((b, size));
                        if i % 3 == 0 {
                            let (b, size) = blocks.swap_remove(i % blocks.len());
                            let r = heap_re_alloc(heap, 0, b, size * 2);
                            assert!(!r.is_null());
                            assert!(Heap::holds(r, padded(size), t as u8));
                            Heap::fill(r, padded(size * 2), t as u8);
                            blocks.push((r, size * 2));
                        }
                    }
                    blocks
                        .into_iter()
                        .map(|(b, _)| b as usize)
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for t in threads {
            for b in t.join().unwrap() {
                h.free(b as *mut c_void);
            }
        }
        assert!(h.valid());
    }
}
