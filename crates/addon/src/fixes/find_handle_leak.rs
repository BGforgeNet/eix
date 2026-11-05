//! FindHandleLeak: opening a loose sound or music file closes the find handle it opens. `CResourceStream::OpenFile`
//! 0x6744C0 calls `_findfirst` (0x674506) for the file's size, drops the handle it returns and never calls
//! `_findclose`, so each stream opened from the `Stream` folder (`CSoundManager::ReadStreamVanilla` 0x6BB3A0) kept one
//! open. Ours closes it at once.
#![expect(unsafe_code, reason = "calls the game's C runtime find functions")]

use core::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use eix_engine::hook;

use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// `call _findfirst`.
const FIND_FIRST_CALL: usize = eix_sites::C_RESOURCE_STREAM_OPEN_FILE__46;
const FIND_FIRST_CALL_BYTES: [u8; 5] = [0xE8, 0x27, 0x73, 0x07, 0x00];
/// The C runtime's `_findclose` (cdecl `int (intptr_t)`: `FindClose`, -1 with `errno` EINVAL on a failure), which
/// nothing in game.exe calls.
const FIND_CLOSE: usize = eix_sites::TEXT_6EB9C7;
/// `_findfirst`'s failure.
const NOT_FOUND: isize = -1;

type FindFirst = unsafe extern "C" fn(*const u8, *mut c_void) -> isize;
type FindClose = unsafe extern "C" fn(isize) -> i32;

static FIND_FIRST: AtomicUsize = AtomicUsize::new(0);

extern "C" fn find_first(pattern: *const u8, data: *mut c_void) -> isize {
    // SAFETY: FIND_FIRST holds the call's old target, `_findfirst`, of this type; the arguments are the game's.
    let find_first = unsafe { hook::as_fn::<FindFirst>(FIND_FIRST.load(Ordering::Relaxed)) };
    // SAFETY: as above.
    let handle = unsafe { find_first(pattern, data) };
    if handle != NOT_FOUND {
        // SAFETY: the address is the C runtime's `_findclose`, of this type; the handle is `_findfirst`'s.
        let find_close = unsafe { hook::as_fn::<FindClose>(FIND_CLOSE) };
        // SAFETY: as above; the caller never uses the handle.
        unsafe { find_close(handle) };
    }
    handle
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let (write, old) = hook::redirect_call(
        FIND_FIRST_CALL,
        FIND_FIRST_CALL_BYTES,
        find_first as *const () as usize,
    )?;
    FIND_FIRST.store(old, Ordering::Relaxed);
    Ok(vec![Labelled {
        label: "FindHandleLeak #0".to_owned(),
        write,
    }])
}
