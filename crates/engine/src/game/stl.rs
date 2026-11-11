//! STLport containers as game.exe lays them out.
#![expect(unsafe_code, reason = "walking game.exe's hash tables")]

use core::marker::PhantomData;
use core::mem::offset_of;

/// A node of a [`HashTable`] bucket chain: its first word is the next node of the bucket.
///
/// # Safety
/// The implementing type is `#[repr(C)]` and starts with that pointer.
pub unsafe trait HashNode {}

/// An STLport `hash_map` / `hash_set`: the buckets vector at +4..+8, each bucket a chain of nodes. The game's begin
/// iterator 0x455830 walks the buckets from +4 to +8; `NamedFloatTable::Set` 0x460BC0 hashes into them.
#[repr(C)]
#[derive(Debug)]
pub struct HashTable<N> {
    _unknown0: [u8; 4],
    /// The first bucket (0x455830).
    pub buckets: *mut *mut N,
    /// One past the last bucket (0x455830).
    pub buckets_end: *mut *mut N,
    _unknownc: [u8; 0x14 - 0xC],
    _node: PhantomData<N>,
}

const _: () = {
    assert!(offset_of!(HashTable<u8>, buckets) == 0x4);
    assert!(offset_of!(HashTable<u8>, buckets_end) == 0x8);
    assert!(size_of::<HashTable<u8>>() == 0x14);
};

impl<N: HashNode> HashTable<N> {
    /// Calls `f` with each node, bucket by bucket and down each chain: the order of the table's own iterator and of
    /// addon.dll 0.12.4.2's walk.
    ///
    /// # Safety
    /// `this` must be a live table of a supported game.exe, on the game's thread; `f` must not change it.
    pub unsafe fn for_each(this: *const Self, mut f: impl FnMut(*mut N)) {
        // SAFETY: the caller guarantees a live table.
        let table = unsafe { &*this };
        let (mut bucket, end) = (table.buckets, table.buckets_end);
        while bucket < end {
            // SAFETY: `bucket` lies in the buckets vector.
            let mut node = unsafe { *bucket };
            while !node.is_null() {
                f(node);
                // SAFETY: a chain node of a live table starts with its successor (`HashNode`).
                node = unsafe { node.cast::<*mut N>().read() };
            }
            // SAFETY: stays within or one past the vector.
            bucket = unsafe { bucket.add(1) };
        }
    }
}
