//! StreamFadeLock: the music fades write the fader under the stream lock, which the stream thread holds while it
//! reads them. `CSoundManager::FadeOutStream` 0x6BAFA0 and `FadeInStream` 0x6BAFE0 store the fader's state, elapsed
//! time and rate (+0x150..+0x15C) from the main thread with no lock, while `StreamThread` 0x6BB910 runs
//! `SoundFader::Update` inside the manager's lock (+0x1B0, 0x6BB966); a read between the stores took one step at a
//! zero or stale rate. The main thread already takes that lock for its other stream calls (0x6BB05E, 0x6BB25E).
#![expect(
    unsafe_code,
    reason = "calls the game's fades inside the game's stream lock"
)]

use core::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use eix_engine::{hook, win};

use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// Both start with `mov edx, [ecx+0x898]`, the manager's opened flag.
const FADE_OUT: usize = eix_sites::C_SOUND_MANAGER_FADE_OUT_STREAM;
const FADE_IN: usize = eix_sites::C_SOUND_MANAGER_FADE_IN_STREAM;
const PROLOGUE: [u8; 6] = [0x8B, 0x91, 0x98, 0x08, 0x00, 0x00];
/// The manager's stream lock, which its constructor initialises (0x6B614D).
const LOCK: usize = 0x1B0;

type Fade = unsafe extern "thiscall" fn(*mut c_void);

static FADE_OUT_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static FADE_IN_ORIGINAL: AtomicUsize = AtomicUsize::new(0);

/// Runs the fade at `original` on `manager` inside the manager's lock.
fn locked(original: &AtomicUsize, manager: *mut c_void) {
    // SAFETY: `original` holds a trampoline `build` made, of this type.
    let fade = unsafe { hook::as_fn::<Fade>(original.load(Ordering::Relaxed)) };
    let run = || {
        // SAFETY: the game's fade on its manager.
        unsafe { fade(manager) }
    };
    // SAFETY: `manager` is the game's live sound manager, whose lock lives as long as it.
    unsafe { win::in_critical_section(manager.wrapping_byte_add(LOCK), run) };
}

extern "thiscall" fn fade_out(manager: *mut c_void) {
    locked(&FADE_OUT_ORIGINAL, manager);
}

extern "thiscall" fn fade_in(manager: *mut c_void) {
    locked(&FADE_IN_ORIGINAL, manager);
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let (out_write, out_trampoline) =
        hook::replace_function(FADE_OUT, &PROLOGUE, fade_out as *const () as usize)?;
    let (in_write, in_trampoline) =
        hook::replace_function(FADE_IN, &PROLOGUE, fade_in as *const () as usize)?;
    FADE_OUT_ORIGINAL.store(out_trampoline, Ordering::Relaxed);
    FADE_IN_ORIGINAL.store(in_trampoline, Ordering::Relaxed);
    Ok(vec![
        Labelled {
            label: "StreamFadeLock #0".to_owned(),
            write: out_write,
        },
        Labelled {
            label: "StreamFadeLock #1".to_owned(),
            write: in_write,
        },
    ])
}
