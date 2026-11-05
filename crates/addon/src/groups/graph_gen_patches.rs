//! GraphGenPatches (`game.exe addon aigraph ...`): what the AI graph's generation needs from game.exe. The loading
//! movie never plays (#0), `FatalError` prints its text to standard output instead of its message box (#1) and
//! ends the game only at its 127th error (#2), `CWorldServer::NewZone` does not load the zone's existing graph (#3),
//! and each block's edges are measured by the addon's own rule (#4).
#![expect(
    unsafe_code,
    reason = "the block measure reads the game's region map and calls its path search"
)]

use eix_core::cli::{BLOCK_STEPS, edge_word};
use eix_engine::game::ai::{
    AiMap, MEASURE_BLOCK_EDGES_CALL, MeasureBlockEdges, RegionMap, SearchGrid,
};
use eix_engine::game::mfc::PtrList;
use eix_engine::group::SiteWrite;
use eix_engine::hook;

use super::{BuildError, Labelled, Setup, bytes};
use crate::cli;

/// The size classes of the region map.
const CLASSES: usize = 8;

/// The block a path list allocates its nodes in, as the addon's measure makes its list.
const PATH_BLOCK: i32 = 0x80;

/// Stands in for `FatalError`'s message box (0x4560D0, fastcall: the text, the box flags): the text and a line end
/// go to standard output.
extern "fastcall" fn print_fatal(text: *const u8, _flags: u32) -> u32 {
    if !text.is_null() {
        // SAFETY: FatalError passes its NUL-terminated message buffer (0x4800A3).
        let text = unsafe { std::ffi::CStr::from_ptr(text.cast()) };
        cli::print(&[text.to_bytes(), b"\n"].concat());
    }
    0
}

/// One representative cell of block (`x`, `y`) for a size class: the low nibble is its x and the high one its y
/// within the block's 8 x 8 cells.
fn cell_of(grid_cell: u8, x: i32, y: i32) -> (i32, i32) {
    (
        i32::from(grid_cell & 0xF) + x * 8,
        i32::from(grid_cell >> 4) + y * 8,
    )
}

/// The element (`x`, `y`) of `grid`.
///
/// # Safety
/// (`x`, `y`) must lie inside the grid, whose rows the game allocated.
unsafe fn element<T>(grid: &SearchGrid<T>, x: i32, y: i32) -> *mut T {
    // SAFETY: the caller guarantees row `y` exists.
    let row = unsafe { grid.rows.wrapping_add(y as usize).read() };
    row.wrapping_add(x as usize)
}

/// The addon's `CRegionMap::MeasureBlockEdges`: for each neighbour and size class, the edge word from
/// the path search between the two blocks' representative cells over the whole AI map, where the game's own measure
/// searches only the 3 x 3 blocks around (0x5AFAE0).
extern "thiscall" fn measure_block_edges(this: *mut RegionMap, map: *mut AiMap, x: i32, y: i32) {
    // SAFETY: the game calls this in place of MeasureBlockEdges, with its region map, which outlives the call.
    let regions = unsafe { &*this };
    let width = regions.edges[0].width;
    let height = regions.edges[0].height;
    for (dir, (dx, dy)) in BLOCK_STEPS.iter().enumerate() {
        let (nx, ny) = (x + dx, y + dy);
        let inside = nx >= 0 && ny >= 0 && nx < width && ny < height;
        for class in 0..CLASSES {
            let word = if inside {
                // SAFETY: the game measures block (x, y) of its grid.
                let own = unsafe { element(&regions.cells[class], x, y) };
                // SAFETY: as above, inside the grid.
                let own = unsafe { own.read() };
                // SAFETY: (nx, ny) was checked to lie inside the grid.
                let other = unsafe { element(&regions.cells[class], nx, ny) };
                // SAFETY: as above.
                let other = unsafe { other.read() };
                let (sx, sy) = cell_of(own, x, y);
                let (tx, ty) = cell_of(other, nx, ny);
                let mut path = PtrList::new();
                // SAFETY: `path` is writable list memory, constructed here.
                unsafe { PtrList::construct(&raw mut path, PATH_BLOCK) };
                let mut cost = 0i32;
                // SAFETY: the game's AI map, on its thread; `path` and `cost` live through the call.
                let found = unsafe {
                    AiMap::path_cost(
                        map,
                        class as i32,
                        sx,
                        sy,
                        tx,
                        ty,
                        &raw mut path,
                        &raw mut cost,
                    )
                };
                // SAFETY: the list constructed above; as in the original, its memory is not destroyed further.
                unsafe { PtrList::remove_all(&raw mut path) };
                edge_word(found.then_some(cost))
            } else {
                -1
            };
            // SAFETY: the edge grid has 8 words per block, so word `dir` of block (x, y) is element (8x + dir, y).
            let at = unsafe { element(&regions.edges[class], x * 8 + dir as i32, y) };
            // SAFETY: as above, inside the grid.
            unsafe { at.write(word) };
        }
    }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let (fatal, _) = hook::redirect_call(
        eix_sites::FATAL_ERROR__27,
        [0xE8, 0x24, 0x60, 0xFD, 0xFF],
        print_fatal as *const () as usize,
    )?;
    let measure: MeasureBlockEdges = measure_block_edges;
    let (edges, _) = hook::redirect_call(
        MEASURE_BLOCK_EDGES_CALL,
        [0xE8, 0xB6, 0xFD, 0xFF, 0xFF],
        measure as usize,
    )?;
    Ok(vec![
        // push ebp, the first byte of ShowLoadingMovie 0x5DA400, becomes ret.
        bytes(
            "GraphGenPatches #0",
            eix_sites::SHOW_LOADING_MOVIE,
            &[0x55],
            &[0xC3],
        )?,
        Labelled {
            label: "GraphGenPatches #1".to_owned(),
            // call 0x4560D0, FatalError's message box
            write: fatal,
        },
        // cmp eax, 1: the error count at which FatalError exits becomes 127.
        bytes(
            "GraphGenPatches #2",
            eix_sites::FATAL_ERROR__43,
            &[0x01],
            &[0x7F],
        )?,
        Labelled {
            label: "GraphGenPatches #3".to_owned(),
            // lea edx, [esp+0x24]; mov ecx, esi; push edx; push ebp; push edi; mov [esp+0x60], 2;
            // call 0x596140 (the zone's map and graph); test eax, eax; jnz 0x5955ED; push 0x790AD8; jmp 0x59563D
            write: SiteWrite::nop(
                eix_sites::C_WORLD_SERVER_NEW_ZONE__14C,
                &[
                    0x8D, 0x54, 0x24, 0x24, 0x8B, 0xCE, 0x52, 0x55, 0x57, 0xC7, 0x44, 0x24, 0x60,
                    0x02, 0x00, 0x00, 0x00, 0xE8, 0x5E, 0x0B, 0x00, 0x00, 0x85, 0xC0, 0x75, 0x07,
                    0x68, 0xD8, 0x0A, 0x79, 0x00, 0xEB, 0x50,
                ],
            ),
        },
        Labelled {
            label: "GraphGenPatches #4".to_owned(),
            // call 0x5AFAE0, CRegionMap::MeasureBlockEdges
            write: edges,
        },
    ])
}
