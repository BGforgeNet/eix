//! ExitIndex: an exit the zone does not have reads as an empty exit record, where the game read past its exits array
//! or through its null data pointer. `ExitGetAt` 0x587DD0 copies record `index` (0x3C bytes, a `CString` at +0x28)
//! from the array's data (+0x04) with no test of the index against the count (+0x08); a zone whose exits did not load
//! (missing map files) faults at 0x587DEC as soon as a briefing screen updates (`CBriefingScreen::Update` through
//! `GameExitTarget` and `CWorldClient::ExitTarget`).
#![expect(
    unsafe_code,
    reason = "reads the game's exits array and builds the record in the caller's buffer"
)]

use core::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use eix_engine::game::mfc::{self, GameString};
use eix_engine::hook;
use eix_sites::Site;

use super::Warnings;
use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// `push ecx; mov eax, [esp+0xc]`, the function's first two instructions.
const EXIT_GET_AT: Site = eix_sites::EXIT_GET_AT;
const PROLOGUE: [u8; 5] = [0x51, 0x8B, 0x44, 0x24, 0x0C];
/// An exit record's size and its name's offset.
const RECORD_SIZE: usize = 0x3C;
const RECORD_NAME: usize = 0x28;

/// The game's exits array: a data pointer and a count after its first word.
#[repr(C)]
struct ExitArray {
    _unknown0: u32,
    data: *mut c_void,
    count: i32,
}

const _: () = {
    assert!(core::mem::offset_of!(ExitArray, _unknown0) == 0x0);
    assert!(core::mem::offset_of!(ExitArray, data) == 0x4);
    assert!(core::mem::offset_of!(ExitArray, count) == 0x8);
};

type GetAt = unsafe extern "thiscall" fn(*mut ExitArray, *mut u8, i32) -> *mut u8;

/// The original function, run through its trampoline.
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
/// A briefing screen asks every frame.
static WARNINGS: Warnings = Warnings::new("ExitIndex");

extern "thiscall" fn exit_get_at(array: *mut ExitArray, out: *mut u8, index: i32) -> *mut u8 {
    // SAFETY: the game passes its live exits array.
    let exits = unsafe { &*array };
    let (data, count) = (exits.data, exits.count);
    if !data.is_null() && (0..count).contains(&index) {
        // SAFETY: ORIGINAL holds the trampoline `build` made, of this type; the arguments are the game's.
        let original = unsafe { hook::as_fn_at::<GetAt>(ORIGINAL.load(Ordering::Relaxed)) };
        // SAFETY: as above.
        return unsafe { original(array, out, index) };
    }
    WARNINGS.warn(|| {
        format!("exit {index} asked of a zone with {count} (data {data:p}): an empty exit")
    });
    // SAFETY: `out` is the caller's buffer for one record, which the original fills whole.
    unsafe { core::ptr::write_bytes(out, 0, RECORD_SIZE) };
    let name = out.wrapping_add(RECORD_NAME).cast::<GameString>();
    // SAFETY: the name's place in that buffer, now zero, gets the game's empty string, which the caller destroys.
    unsafe { mfc::construct_empty(name) };
    out
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let (write, trampoline) =
        hook::replace_function(EXIT_GET_AT, &PROLOGUE, exit_get_at as *const () as usize)?;
    ORIGINAL.store(trampoline, Ordering::Relaxed);
    Ok(vec![Labelled {
        label: "ExitIndex #0".to_owned(),
        write,
    }])
}
