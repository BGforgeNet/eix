//! EmptyItemStrip: an item strip whose list comes from a missing constructor record shows no items, where the game read
//! the list's count at address 0x44. The camp and shop screens set up their strips (`CUiItemStrip::Setup` 0x621160,
//! from 0x60D428, 0x60D6C1, 0x60D890 and 0x62C600) with a constructor record's address plus 0x40, its item list; the
//! record of `i.onspellconstr` or `i.onitemconstr` (script variables) can be missing, and the strip's list is then
//! 0x40, which `CUiItemStrip::RebuildList` 0x6212F0 reads (0x621323). Ours gives such a strip an empty list.
#![expect(
    unsafe_code,
    reason = "calls the game's strip setup through its trampoline"
)]

use core::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use eix_engine::hook;
use eix_sites::Site;

use super::Warnings;
use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// `mov eax, fs:[0]`: the start of its exception frame.
const SETUP: Site = eix_sites::C_UI_ITEM_STRIP_SETUP;
const PROLOGUE: [u8; 6] = [0x64, 0xA1, 0x00, 0x00, 0x00, 0x00];
/// Windows maps nothing in the first 64 KiB, so a list below is a missing record's.
const NULL_PAGES: usize = 0x1_0000;

/// The list a missing record's strip gets: no items (+0x00), a count of 0 (+0x04). The strip only reads its list.
static EMPTY: [u32; 2] = [0, 0];

type StripSetup = unsafe extern "thiscall" fn(*mut c_void, *const c_void, u32);

static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static WARNINGS: Warnings = Warnings::new("EmptyItemStrip");

extern "thiscall" fn setup(strip: *mut c_void, list: *const c_void, flag: u32) {
    let list = if list.addr() < NULL_PAGES {
        WARNINGS.warn(|| format!("item strip with list {list:p} (a missing record): no items"));
        EMPTY.as_ptr().cast()
    } else {
        list
    };
    // SAFETY: ORIGINAL holds the trampoline `build` made, of this type; the arguments are the game's, or the empty
    // list, which outlives every strip.
    let original = unsafe { hook::as_fn_at::<StripSetup>(ORIGINAL.load(Ordering::Relaxed)) };
    // SAFETY: as above.
    unsafe { original(strip, list, flag) };
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let (write, trampoline) =
        hook::replace_function(SETUP, &PROLOGUE, setup as *const () as usize)?;
    ORIGINAL.store(trampoline, Ordering::Relaxed);
    Ok(vec![Labelled {
        label: "EmptyItemStrip #0".to_owned(),
        write,
    }])
}
