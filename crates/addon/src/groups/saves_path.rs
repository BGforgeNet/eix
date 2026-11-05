//! SavesPath (`[MOD] GameSaves`, default 1): the saves, the multiplayer heroes and their folders go under the mod
//! folder. Where the game builds those paths from its base folder `CString` 0x7AF460 (`mov eax, [0x7AF460]`), the
//! operand points at a `CString` holding the mod folder; where it asks `GameFlow` for the folder (`call 0x578DB0`,
//! which returns the `CString*`), the call becomes `mov eax, <that CString>`.

use super::game_text::game_string;
use super::{BuildError, Labelled, Setup, bytes, pointer};

/// The game's base folder `CString`.
const BASE_FOLDER: usize = eix_sites::DATA_7AF460;

/// The operands that read [`BASE_FOLDER`], with the original's labels, in its order.
const OPERANDS: [(&str, usize); 13] = [
    (
        "Set Addon path for: saves\\ (CreateDir)",
        eix_sites::TEXT_456C5F,
    ),
    (
        "Set Addon path for: mp\\ (CreateDir)",
        eix_sites::TEXT_456C8C,
    ),
    (
        "Set Addon path for: saves\\current\\ #1",
        eix_sites::ENTER_ZONE__230,
    ),
    (
        "Set Addon path for: saves\\current\\ #2",
        eix_sites::LEAVE_ZONE__93,
    ),
    (
        "Set Addon path for: saves\\current\\ #3",
        eix_sites::GAME_FLOW_LOAD_GAME__CC,
    ),
    (
        "Set Addon path for: saves\\current\\ #4",
        eix_sites::GAME_FLOW_SAVE_GAME__FE,
    ),
    (
        "Set Addon path for: saves\\current\\ #5",
        eix_sites::GAME_FLOW_START_NEW_GAME__41,
    ),
    (
        "Set Addon path for: saves\\current\\ #6",
        eix_sites::START_NETWORK_ZONE__A0,
    ),
    (
        "Set Addon path for: saves\\current\\ #7",
        eix_sites::GAME_FLOW_PROCESS_REQUEST__2C4,
    ),
    (
        "Set Addon path for: saves\\current\\ #8",
        eix_sites::GAME_FLOW_SET_NETWORK_ZONE__25,
    ),
    (
        "Set Addon path for: saves\\*\\ #1",
        eix_sites::GAME_FLOW_LOAD_GAME__AE,
    ),
    (
        "Set Addon path for: saves\\*\\ #2",
        eix_sites::GAME_FLOW_SAVE_GAME__E8,
    ),
    (
        "Set Addon path for: saves\\current\\*.*",
        eix_sites::CURRENT_SAVE_SIZE__35,
    ),
];

/// The calls of `GameFlow`'s folder getter 0x578DB0, with their bytes.
const CALLS: [(&str, usize, [u8; 5]); 7] = [
    (
        "Set Addon path for: saves\\*\\ #3",
        eix_sites::C_SAVE_LOAD_WINDOW_SAVE_GAME__127,
        [0xE8, 0x34, 0x37, 0xF2, 0xFF],
    ),
    (
        "Set Addon path for: saves\\*\\ #4",
        eix_sites::C_SAVE_LOAD_WINDOW_SHOW_PICTURE__E6,
        [0xE8, 0xB5, 0x1E, 0xF2, 0xFF],
    ),
    (
        "Set Addon path for: saves\\*\\ #5",
        eix_sites::QUICK_SAVE__20D,
        [0xE8, 0xBE, 0x1A, 0xF2, 0xFF],
    ),
    (
        "Set Addon path for: saves\\*\\ #6",
        eix_sites::QUICK_LOAD__44,
        [0xE8, 0xB7, 0x17, 0xF2, 0xFF],
    ),
    (
        "Set Addon path for: saves\\*",
        eix_sites::C_SAVE_LOAD_WINDOW_DELETE_SAVE__77,
        [0xE8, 0x14, 0x32, 0xF2, 0xFF],
    ),
    (
        "Set Addon path for: saves\\save*",
        eix_sites::C_SAVE_LOAD_WINDOW_FILL_ENTRIES__3E,
        [0xE8, 0x2D, 0x30, 0xF2, 0xFF],
    ),
    (
        "Set Addon path for: mp\\*",
        eix_sites::C_MP_HERO_SCREEN_CREATE__1782,
        [0xE8, 0x89, 0x37, 0xF6, 0xFF],
    ),
];

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let folder = game_string(setup.mod_dir.clone())?;
    let mut writes: Vec<Labelled> = OPERANDS
        .iter()
        .map(|&(label, site)| pointer(label, site, BASE_FOLDER, folder))
        .collect();
    let mut mov = [0xB8, 0, 0, 0, 0];
    mov[1..].copy_from_slice(&(folder as u32).to_le_bytes());
    for (label, site, call) in CALLS {
        writes.push(bytes(label, site, &call, &mov)?);
    }
    Ok(writes)
}
