//! What WinMain loads before the game's first zone (0x4568E9-0x45705C): the folder strings, the options from the
//! registry, the logic settings and the databases. The addon's `aigraph` command runs these steps itself, since it
//! works before WinMain does.
//!
//! Names follow ei-rebuild.
#![expect(unsafe_code, reason = "calls into game.exe's start-up code")]

use core::ffi::{CStr, c_void};

use super::mfc::GameString;
use super::{Global, code};

/// The registry key the game reads its options from, a game string WinMain assigns `Software\Nival
/// Interactive\EvilIslands` (0x4568E9-0x4568F3) and then passes to the options' reader (0x456925).
pub const REGISTRY_KEY: Global<GameString> = Global::at(eix_sites::DATA_7AF450);

/// The game's start folder, a game string [`store_start_folder`] fills; WinMain joins it with `config\ai.reg` for
/// the logic settings (0x456905).
pub const START_FOLDER: Global<GameString> = Global::at(eix_sites::DATA_7AF454);

/// The options (`g_options`), whose registry reader WinMain calls with [`REGISTRY_KEY`] (0x45692B).
pub const OPTIONS: Global<c_void> = Global::at(eix_sites::DATA_79BC70);

/// The options' word at +0x24 that WinMain passes to every database load (0x45700F-0x45705B).
pub const DATABASE_SOURCE: Global<usize> = Global::at(eix_sites::DATA_79BC94);

/// The logic settings (`g_logicSettings`), loaded from the start folder's `config\ai.reg` (0x456912).
pub const LOGIC_SETTINGS: Global<c_void> = Global::at(eix_sites::DATA_7C53F8);

/// The unit database (WinMain loads it at 0x457020).
pub const UNIT_DATABASE: Global<c_void> = Global::at(eix_sites::DATA_7AF358);

/// The skill database.
pub const SKILL_DATABASE: Global<c_void> = Global::at(eix_sites::DATA_7AF268);

/// The spell database.
pub const SPELL_DATABASE: Global<c_void> = Global::at(eix_sites::DATA_7C8510);

/// The item database (WinMain loads it at 0x45704B).
pub const ITEM_DATABASE: Global<c_void> = Global::at(eix_sites::DATA_7C86D8);

/// The lever records (WinMain loads them at 0x45705C).
pub const LEVER_TABLE: Global<c_void> = Global::at(eix_sites::DATA_79C1A0);

/// Stores the process's working folder as [`START_FOLDER`]: `StoreStartFolder` 0x55EA10 (cdecl, no arguments),
/// which WinMain calls right after assigning [`REGISTRY_KEY`] (0x4568F8).
///
/// # Safety
/// The process must be a supported game.exe, on the game's thread.
pub unsafe fn store_start_folder() {
    // SAFETY: 0x55EA10 is `void __cdecl ()`.
    let f = unsafe { code::<unsafe extern "C" fn()>(eix_sites::STORE_START_FOLDER) };
    // SAFETY: the caller guarantees the build and the thread.
    unsafe { f() }
}

/// Loads the logic settings from `path`: `LogicSettings::Load` 0x5CE0F0 (thiscall on [`LOGIC_SETTINGS`], `ret 4`).
///
/// # Safety
/// As for [`store_start_folder`].
pub unsafe fn load_logic_settings(path: &CStr) {
    // SAFETY: 0x5CE0F0 is `void __thiscall (const char*)`; its result is not used.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void, *const u8)>(eix_sites::LOGIC_SETTINGS_LOAD)
    };
    // SAFETY: the caller guarantees the build and the thread; `path` is NUL-terminated.
    unsafe { f(LOGIC_SETTINGS.as_ptr(), path.as_ptr().cast()) }
}

/// Reads the options from the registry key `key`: `GameOptions::LoadFromRegistry` 0x45B210 (thiscall on
/// [`OPTIONS`], `ret 4`).
///
/// # Safety
/// As for [`store_start_folder`]; `key` must point at a NUL-terminated string.
pub unsafe fn load_options(key: *const u8) {
    // SAFETY: 0x45B210 is `void __thiscall (const char*)`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void, *const u8)>(
            eix_sites::GAME_OPTIONS_LOAD_FROM_REGISTRY,
        )
    };
    // SAFETY: the caller guarantees the build, the thread and the key.
    unsafe { f(OPTIONS.as_ptr(), key) }
}

/// A database's `Load`: thiscall, the [`DATABASE_SOURCE`] word and the archive name on the stack, `ret 8`.
type DatabaseLoad = unsafe extern "thiscall" fn(*mut c_void, usize, *const u8) -> i32;

/// Loads the unit, skill, spell and item databases from archive `res` (`database.res`, `databaselmp.res`), then
/// the lever records: `UnitDatabase::Load` 0x53A200, `SkillDatabase::Load` 0x520220, `SpellDatabase::Load`
/// 0x684550, `ItemDatabase::Load` 0x6A67A0 (each `ret 8`), `LeverRecordTable::Load` 0x463390 (`ret 4`), all with
/// the [`DATABASE_SOURCE`] word, in this order.
///
/// # Safety
/// As for [`store_start_folder`]; the options must be loaded.
pub unsafe fn load_databases(res: &CStr) {
    // SAFETY: the caller guarantees the build and that the options are loaded.
    let source = unsafe { DATABASE_SOURCE.read() };
    for (database, address) in [
        (UNIT_DATABASE, eix_sites::UNIT_DATABASE_LOAD),
        (SKILL_DATABASE, eix_sites::SKILL_DATABASE_LOAD),
        (SPELL_DATABASE, eix_sites::SPELL_DATABASE_LOAD),
        (ITEM_DATABASE, eix_sites::ITEM_DATABASE_LOAD),
    ] {
        // SAFETY: each address is a database's `Load` of type `DatabaseLoad` (the `ret 8` of each body).
        let f = unsafe { code::<DatabaseLoad>(address) };
        // SAFETY: the caller guarantees the build and the thread; `res` is NUL-terminated.
        unsafe { f(database.as_ptr(), source, res.as_ptr().cast()) };
    }
    // SAFETY: 0x463390 is `int __thiscall (word)`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void, usize) -> i32>(
            eix_sites::LEVER_RECORD_TABLE_LOAD,
        )
    };
    // SAFETY: as above.
    unsafe { f(LEVER_TABLE.as_ptr(), source) };
}
