//! Movies: game.exe's imports from Bink's `binkw32.dll`, and the loading screen's movie.

use core::ffi::{CStr, c_void};

use super::Global;

/// `binkw32!_BinkOpen@8`: stdcall `(file name, flags)`, the opened movie or null.
pub type BinkOpen = unsafe extern "system" fn(*const u8, u32) -> *mut c_void;

/// game.exe's import slot for `_BinkOpen@8` 0x73B720, which every movie open calls through (`call [0x73B720]`).
pub const BINK_OPEN_SLOT: Global<usize> = Global::at(eix_sites::RDATA_73B720);

/// The loading screen's movie, a file name: `SelectLoadingMovie` 0x595080 stores one of its static names
/// (0x595130 "progres.bik") and the loading screen reads it (0x5DA474).
pub const LOADING_MOVIE: Global<*const u8> = Global::at(eix_sites::G_LOADING_MOVIE_NAME);

/// `_BinkSetVolume@12` of later Bink builds: stdcall `(movie, track, volume)`; the game's own Bink has
/// `_BinkSetVolume@8`, `(movie, volume)`.
pub type BinkSetVolume12 = unsafe extern "system" fn(*mut c_void, u32, i32);

/// game.exe's imports from `binkw32.dll`, all by name: each name and its import slot, read from the exe's import
/// directory (first thunk 0x73B714, names in the original thunk list), in slot order.
pub const BINK_IMPORTS: [(&CStr, Global<usize>); 12] = [
    (c"_BinkGoto@12", Global::at(eix_sites::RDATA_73B714)),
    (c"_BinkDoFrame@4", Global::at(eix_sites::RDATA_73B718)),
    (c"_BinkDDSurfaceType@4", Global::at(eix_sites::RDATA_73B71C)),
    (c"_BinkOpen@8", Global::at(eix_sites::RDATA_73B720)),
    (c"_BinkNextFrame@4", Global::at(eix_sites::RDATA_73B724)),
    (c"_BinkWait@4", Global::at(eix_sites::RDATA_73B728)),
    (c"_BinkSetFrameRate@8", Global::at(eix_sites::RDATA_73B72C)),
    (c"_BinkSetVolume@8", Global::at(eix_sites::RDATA_73B730)),
    (
        c"_BinkSetSoundSystem@8",
        Global::at(eix_sites::RDATA_73B734),
    ),
    (c"_BinkOpenMiles@4", Global::at(eix_sites::RDATA_73B738)),
    (c"_BinkClose@4", Global::at(eix_sites::RDATA_73B73C)),
    (c"_BinkCopyToBuffer@28", Global::at(eix_sites::RDATA_73B740)),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bink_slots_are_one_table_holding_bink_open() {
        for (i, (_, slot)) in BINK_IMPORTS.iter().enumerate() {
            assert_eq!(slot.address(), eix_sites::RDATA_73B714 + 4 * i);
        }
        assert!(
            BINK_IMPORTS
                .iter()
                .any(|(n, s)| *n == c"_BinkOpen@8" && s.address() == BINK_OPEN_SLOT.address())
        );
    }
}
