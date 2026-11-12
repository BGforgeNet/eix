//! PersistentGlobalVars: where the game empties a party's variable table, the `gvar.` variables survive.
//!
//! The three places are network paths: the party outside a game (`GameFlow::AfterServerStart` 0x57EB25), the
//! client's update from the server (`GameFlow::UpdateFromServer` 0x580177) and each saved party the list of saved
//! parties reloads (`SavedParties::Reload` 0x662FB0, whose inlined clear is skipped and replaced). Each keeps the
//! variables whose name starts with `gvar.` (with case), lets the table be cleared, then sets them again in the order
//! of the original's `std::unordered_map`, which decides their order in the table's chains.
#![expect(
    unsafe_code,
    reason = "reading and refilling the game's variable tables"
)]

use std::ffi::{CStr, CString};

use eix_core::msvc_map::StringFloatMap;
use eix_engine::game::player::NamedFloatTable;
use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, Mode};
use eix_engine::win::ansi_text;
use eix_sites::Site;

use super::{BuildError, Labelled, Setup};
use crate::log;

/// `SavedParties::Reload`'s `jne` around the inlined clear: its target moves from 0x663129 to 0x66316C.
const RELOAD_BRANCH: Site = eix_sites::SAVED_PARTIES_RELOAD__D3;
const RELOAD_BRANCH_BYTES: [u8; 6] = [0x0F, 0x85, 0xA0, 0x00, 0x00, 0x00];
const RELOAD_BRANCH_NEW: [u8; 6] = [0x0F, 0x85, 0xE3, 0x00, 0x00, 0x00];
/// The clear's last step, `mov [esp+0x80], ebx` (the count), dropped for the hook.
const RELOAD_CLEAR: Site = eix_sites::SAVED_PARTIES_RELOAD__1C0;
const RELOAD_CLEAR_BYTES: [u8; 7] = [0x89, 0x9C, 0x24, 0x80, 0x00, 0x00, 0x00];
/// The table at the site's stack +0x70.
const RELOAD_TABLE: usize = 0x70;
/// The two calls of `CStrFloat::Clear` 0x461540.
const CLEAR_CALLS: [(&str, Site, [u8; 5]); 2] = [
    (
        "PersistentGlobalVars #2",
        eix_sites::GAME_FLOW_AFTER_SERVER_START__15,
        [0xE8, 0x16, 0x2A, 0xEE, 0xFF],
    ),
    (
        "PersistentGlobalVars #3",
        eix_sites::GAME_FLOW_UPDATE_FROM_SERVER__B7,
        [0xE8, 0xC4, 0x13, 0xEE, 0xFF],
    ),
];

/// Empties `table` but its `gvar.` variables.
///
/// # Safety
/// `table` must be a live named float table of a supported game.exe, on the game's thread.
unsafe fn clear_keeping_globals(table: *mut NamedFloatTable) {
    let mut kept = StringFloatMap::default();
    let keep = |name: &CStr, value: f32| {
        let name = name.to_bytes();
        if name.starts_with(b"gvar.") {
            *kept.entry(name) = value;
            log::debug(
                "persistent",
                &format!("keeping {} = {value}", ansi_text(name)),
                "",
            );
        }
    };
    // SAFETY: the caller guarantees the table; keeping a name does not change it.
    unsafe { NamedFloatTable::for_each_variable(table, keep) };
    // SAFETY: as above.
    unsafe { NamedFloatTable::clear(table) };
    for (name, value) in kept.elements() {
        // Names come from the table's C strings, so they hold no NUL.
        let Ok(name) = CString::new(name) else {
            continue;
        };
        // SAFETY: as above.
        unsafe { NamedFloatTable::set(table, &name, value) };
    }
}

/// Stands in for `CStrFloat::Clear` at its two calls (thiscall, the table in ECX).
extern "thiscall" fn clear(table: *mut NamedFloatTable) {
    // SAFETY: the game calls Clear with a live table on its thread.
    unsafe { clear_keeping_globals(table) }
}

extern "C" fn clear_reloaded(_: usize, ctx: &mut Context) {
    let table = (ctx.stack() + RELOAD_TABLE) as *mut NamedFloatTable;
    // SAFETY: at the site the reloaded party's table lies at the stack +0x70, untouched: the moved branch skipped the
    // inlined clear.
    unsafe { clear_keeping_globals(table) }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let mut out = vec![
        Labelled {
            label: "PersistentGlobalVars #0".to_owned(),
            write: SiteWrite::bytes(RELOAD_BRANCH, &RELOAD_BRANCH_BYTES, &RELOAD_BRANCH_NEW)?,
        },
        Labelled {
            label: "PersistentGlobalVars #1".to_owned(),
            write: hook::context_hook(
                RELOAD_CLEAR,
                &RELOAD_CLEAR_BYTES,
                Mode::Drop,
                clear_reloaded,
                0,
            )?,
        },
    ];
    for (label, site, original) in CLEAR_CALLS {
        let (write, _) = hook::redirect_call(site, original, clear as *const () as usize)?;
        out.push(Labelled {
            label: label.to_owned(),
            write,
        });
    }
    Ok(out)
}
