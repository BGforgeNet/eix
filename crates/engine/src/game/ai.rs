//! The server world's AI map and its region map: the AI graph that the addon's `aigraph` command builds and saves.
//!
//! Names follow ei-rebuild.
#![expect(unsafe_code, reason = "calls into game.exe's AI map code")]

use core::ffi::c_void;
use core::mem::offset_of;

use super::mfc::PtrList;
use super::{Global, code};

/// The server world's AI map (opaque): its +0x170, which the world's constructor builds at 0x589F4F.
#[derive(Debug)]
pub enum AiMap {}

/// The server world's AI map.
pub const AI_MAP: Global<AiMap> = Global::at(eix_sites::DATA_7B00D0);

/// A grid of per-block values: row pointers, then the width and the height in blocks.
#[repr(C)]
#[derive(Debug)]
pub struct SearchGrid<T> {
    /// One pointer per row.
    pub rows: *mut *mut T,
    /// Blocks per row.
    pub width: i32,
    /// Rows.
    pub height: i32,
    _unknown_c: u32,
}

/// The block layer of the AI map: per size class (8) the edge costs of every block and its representative cell.
///
/// `CRegionMap::MeasureBlockEdges` 0x5AFAE0 reads a class's representative cells at +0x1A0 + 0x10 * class (0x5AFB17)
/// and writes its edge words 0x80 bytes lower, 8 per block (0x5AFBF7); it bounds the neighbour by the first edge
/// grid's width (+0x124, 0x5AFBA2) and height (+0x128, 0x5AFBAA).
#[repr(C)]
#[derive(Debug)]
pub struct RegionMap {
    _unknown0: [u8; 0x120],
    /// Per size class: 8 edge words per block (the cost to each neighbour / 128, -1 for none).
    pub edges: [SearchGrid<i16>; 8],
    /// Per size class: the representative cell of each block, x in the low nibble and y in the high one.
    pub cells: [SearchGrid<u8>; 8],
}

const _: () = {
    assert!(offset_of!(SearchGrid<u8>, width) == 0x4);
    assert!(offset_of!(SearchGrid<u8>, height) == 0x8);
    assert!(size_of::<SearchGrid<u8>>() == 0x10);
    assert!(offset_of!(RegionMap, edges) == 0x120);
    assert!(offset_of!(RegionMap, cells) == 0x1A0);
};

/// The AI map's region map: its +0x22F8.
pub const REGION_MAP: Global<RegionMap> = Global::at(eix_sites::DATA_7B23C8);

/// The `call` in `CRegionMap::UpdateBlocks` 0x5AFC70 that measures each block's edges (0x5AFD25), and its callee
/// `CRegionMap::MeasureBlockEdges` 0x5AFAE0 (thiscall: an AI map, the block's x and y; `ret 0xC`).
pub const MEASURE_BLOCK_EDGES_CALL: usize = eix_sites::C_REGION_MAP_UPDATE_BLOCKS__B5;

/// The type of `CRegionMap::MeasureBlockEdges` and of a function standing in for it at
/// [`MEASURE_BLOCK_EDGES_CALL`].
pub type MeasureBlockEdges = unsafe extern "thiscall" fn(*mut RegionMap, *mut AiMap, i32, i32);

impl RegionMap {
    /// Builds the whole block layer for `map`: 0x5AFDA0 (thiscall, `ret 4`), which nothing in game.exe calls; the
    /// addon's `aigraph` does.
    ///
    /// # Safety
    /// The process must be game.exe RU 1.06, on the game's thread, with a zone loaded into `map`.
    pub unsafe fn build(this: *mut Self, map: *mut AiMap) {
        // SAFETY: 0x5AFDA0 is `void __thiscall (CAIMap*)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *mut AiMap)>(
                eix_sites::C_REGION_MAP_UPDATE_BLOCKS__130,
            )
        };
        // SAFETY: the caller guarantees the build, the thread and the zone.
        unsafe { f(this, map) }
    }

    /// Writes the block layer to `stream` as the AI graph file holds it: `CRegionMap::SaveBlocks` 0x5B1380
    /// (thiscall, `ret 4`).
    ///
    /// # Safety
    /// As for [`build`](Self::build); `stream` must be a constructed stream.
    pub unsafe fn save_blocks(this: *mut Self, stream: *mut c_void) {
        // SAFETY: 0x5B1380 is `void __thiscall (ByteStream*)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *mut c_void)>(
                eix_sites::C_REGION_MAP_SAVE_BLOCKS,
            )
        };
        // SAFETY: the caller guarantees the build, the thread and the stream.
        unsafe { f(this, stream) }
    }
}

impl AiMap {
    /// The cost of the cheapest way for size class `class` from cell (`from_x`, `from_y`) to cell (`to_x`, `to_y`)
    /// into `cost`, and its cells into `path`; false when there is none or both cells are the same: 0x5B9950
    /// (thiscall, seven stack arguments, `ret 0x1C`; it empties `path` first, 0x5B995B-0x5B997B).
    ///
    /// # Safety
    /// As for [`RegionMap::build`]; `path` must be a constructed list and `cost` writable.
    #[expect(
        clippy::too_many_arguments,
        reason = "the game function's own seven arguments"
    )]
    pub unsafe fn path_cost(
        this: *mut Self,
        class: i32,
        from_x: i32,
        from_y: i32,
        to_x: i32,
        to_y: i32,
        path: *mut PtrList,
        cost: *mut i32,
    ) -> bool {
        type PathCost = unsafe extern "thiscall" fn(
            *mut AiMap,
            i32,
            i32,
            i32,
            i32,
            i32,
            *mut PtrList,
            *mut i32,
        ) -> u8;
        // SAFETY: 0x5B9950 is of type `PathCost`; its result is the low byte of EAX.
        let f = unsafe { code::<PathCost>(eix_sites::MEET_LOCAL_SEARCHES__2C0) };
        // SAFETY: the caller guarantees the build, the thread, the list and the cost.
        unsafe { f(this, class, from_x, from_y, to_x, to_y, path, cost) != 0 }
    }
}
