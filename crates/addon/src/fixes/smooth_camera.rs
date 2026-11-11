//! SmoothCamera: the camera covers the same distance, spread over the frames, instead of a lump at each game tick.
//! `GameMain` 0x456440 runs `CCamera::Update` 0x40FB30 (thiscall, a time step in ticks: elapsed milliseconds times
//! 1/55) once per frame for the frame's time (0x45756D for each whole tick of a long frame, 0x457595 for the rest)
//! and again after each game tick for that tick (0x4573A9), so a frame that holds a tick moves the camera by a whole
//! tick's step on top of its own. Ours holds each tick's step back and lets the frame updates take it along
//! (core's [`camera_release`]).
#![expect(unsafe_code, reason = "calls CCamera::Update on the game's camera")]

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use core::ffi::c_void;

use eix_core::engine_fixes::camera_release;
use eix_engine::hook;
use eix_sites::Site;

use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// After the game tick: `call CCamera::Update` with the tick's step.
const TICK_UPDATE: Site = eix_sites::GAME_MAIN__F69;
const TICK_UPDATE_BYTES: [u8; 5] = [0xE8, 0x82, 0x87, 0xFB, 0xFF];
/// The frame's updates: one per whole tick of a long frame, then the rest.
const FRAME_TICK_UPDATE: Site = eix_sites::GAME_MAIN__112D;
const FRAME_TICK_UPDATE_BYTES: [u8; 5] = [0xE8, 0xBE, 0x85, 0xFB, 0xFF];
const FRAME_UPDATE: Site = eix_sites::GAME_MAIN__1155;
const FRAME_UPDATE_BYTES: [u8; 5] = [0xE8, 0x96, 0x85, 0xFB, 0xFF];

type Update = unsafe extern "thiscall" fn(*mut c_void, f32);

/// The tick steps held back, as `f32` bits; the game's thread alone updates the camera.
static PENDING: AtomicU32 = AtomicU32::new(0);
/// `CCamera::Update`, the calls' old target.
static UPDATE: AtomicUsize = AtomicUsize::new(0);

/// The tick's update: its step is held back for the frames.
extern "thiscall" fn tick_update(_camera: *mut c_void, step: f32) {
    let pending = f32::from_bits(PENDING.load(Ordering::Relaxed));
    PENDING.store((pending + step).to_bits(), Ordering::Relaxed);
}

/// A frame's update, with as much of the held-back steps as [`camera_release`] gives it.
extern "thiscall" fn frame_update(camera: *mut c_void, step: f32) {
    let (release, left) = camera_release(f32::from_bits(PENDING.load(Ordering::Relaxed)), step);
    PENDING.store(left.to_bits(), Ordering::Relaxed);
    // SAFETY: UPDATE holds CCamera::Update, of this type, once `build` ran; the calls are redirected only after.
    let update = unsafe { hook::as_fn_at::<Update>(UPDATE.load(Ordering::Relaxed)) };
    // SAFETY: `camera` is the game's camera, as the game passed it, on its own thread.
    unsafe { update(camera, step + release) };
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let (tick, update) = hook::redirect_call(
        TICK_UPDATE,
        TICK_UPDATE_BYTES,
        tick_update as *const () as usize,
    )?;
    let frame = frame_update as *const () as usize;
    let (frame_tick, _) = hook::redirect_call(FRAME_TICK_UPDATE, FRAME_TICK_UPDATE_BYTES, frame)?;
    let (frame_rest, _) = hook::redirect_call(FRAME_UPDATE, FRAME_UPDATE_BYTES, frame)?;
    UPDATE.store(update.here(), Ordering::Relaxed);
    Ok(vec![
        Labelled {
            label: "SmoothCamera #0".to_owned(),
            write: tick,
        },
        Labelled {
            label: "SmoothCamera #1".to_owned(),
            write: frame_tick,
        },
        Labelled {
            label: "SmoothCamera #2".to_owned(),
            write: frame_rest,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_three_calls_go_to_ccamera_update() {
        for (site, bytes) in [
            (TICK_UPDATE, TICK_UPDATE_BYTES),
            (FRAME_TICK_UPDATE, FRAME_TICK_UPDATE_BYTES),
            (FRAME_UPDATE, FRAME_UPDATE_BYTES),
        ] {
            let rel = i32::from_le_bytes(bytes[1..].try_into().unwrap());
            assert_eq!(
                Site::new((site.offset(5)).ru().wrapping_add_signed(rel as isize)),
                eix_sites::C_CAMERA_UPDATE
            );
        }
    }
}
