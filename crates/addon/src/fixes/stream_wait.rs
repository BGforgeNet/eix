//! StreamWait: the main thread sleeps while the stream thread takes a music request, where it spun. A track change
//! (`CMusicSystem::UpdateMusic` 0x6B5350, loop 0x6B5669-0x6B5673) and a jingle (`CMusicSystem::PlayJingle` 0x6B5270,
//! loop 0x6B5310-0x6B5321) wait for the stream manager's state (+0x100) to leave 2 with `Sleep(0)`, while the
//! stream thread (`CSoundManager::StreamThread` 0x6BB910) looks for requests once every 100 ms or so. Ours waits the
//! same way with `Sleep(1)`.
#![expect(
    unsafe_code,
    reason = "reads the stream manager's state, which the stream thread writes"
)]

use std::time::Duration;

use eix_engine::hook::{self, Context, Mode};

use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// `mov edi, [Sleep]` before UpdateMusic's loop; EBP is the stream manager.
const UPDATE_WAIT: usize = eix_sites::C_MUSIC_SYSTEM_UPDATE_MUSIC__313;
const UPDATE_WAIT_BYTES: [u8; 6] = [0x8B, 0x3D, 0xA4, 0xB1, 0x73, 0x00];
const UPDATE_AFTER: usize = eix_sites::C_MUSIC_SYSTEM_UPDATE_MUSIC__325;
/// `mov esi, [Sleep]` before PlayJingle's loop, which reloads the stream manager from the global each pass.
const JINGLE_WAIT: usize = eix_sites::C_MUSIC_SYSTEM_PLAY_JINGLE__9A;
const JINGLE_WAIT_BYTES: [u8; 6] = [0x8B, 0x35, 0xA4, 0xB1, 0x73, 0x00];
const JINGLE_AFTER: usize = eix_sites::C_MUSIC_SYSTEM_PLAY_JINGLE__B3;
/// The stream manager, as PlayJingle reads it (0x6B5314).
const MANAGER: usize = eix_sites::DATA_7AF784;
/// The manager's request state, and its value while a request waits for the stream thread.
const STATE: u32 = 0x100;
const REQUESTED: u32 = 2;

/// Whether the stream manager at `manager` still holds a request the stream thread has not taken.
fn requested(manager: u32) -> bool {
    let state = core::ptr::with_exposed_provenance::<u32>(manager.wrapping_add(STATE) as usize);
    // SAFETY: `manager` is the game's live stream manager; the stream thread writes the word, so it is read volatile.
    unsafe { state.read_volatile() == REQUESTED }
}

fn sleep() {
    std::thread::sleep(Duration::from_millis(1));
}

extern "C" fn update_wait(_: usize, ctx: &mut Context) {
    while requested(ctx.ebp) {
        sleep();
    }
    ctx.resume = UPDATE_AFTER;
}

extern "C" fn jingle_wait(_: usize, ctx: &mut Context) {
    let global = core::ptr::with_exposed_provenance::<u32>(MANAGER);
    loop {
        // SAFETY: the global holds the stream manager while music plays; read each pass as the game's loop does.
        let manager = unsafe { global.read_volatile() };
        // The game's loop leaves ECX on the manager it read last.
        ctx.ecx = manager;
        if !requested(manager) {
            break;
        }
        sleep();
    }
    ctx.resume = JINGLE_AFTER;
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![
        Labelled {
            label: "StreamWait #0".to_owned(),
            write: hook::context_hook(
                UPDATE_WAIT,
                &UPDATE_WAIT_BYTES,
                Mode::Before,
                update_wait,
                0,
            )?,
        },
        Labelled {
            label: "StreamWait #1".to_owned(),
            write: hook::context_hook(
                JINGLE_WAIT,
                &JINGLE_WAIT_BYTES,
                Mode::Before,
                jingle_wait,
                0,
            )?,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_wait_resumes_right_after_its_loop() {
        // UpdateMusic: the load, `push ebx; call edi` (3), `cmp [ebp+0x100], 2` (7), `je` (2).
        assert_eq!(UPDATE_AFTER, UPDATE_WAIT + 6 + 3 + 7 + 2);
        // PlayJingle: the load, `push 0; call esi` (4), `mov ecx, [manager]` (6), `cmp [ecx+0x100], 2` (7), `je` (2).
        assert_eq!(JINGLE_AFTER, JINGLE_WAIT + 6 + 4 + 6 + 7 + 2);
    }

    #[test]
    fn the_wait_ends_when_the_state_leaves_two() {
        let mut manager = [0u32; 0x41];
        let base = manager.as_mut_ptr();
        let at = base.expose_provenance() as u32;
        let state = base.wrapping_add(0x40);
        // SAFETY: `state` is the array's last word, which only this test uses.
        unsafe { state.write(REQUESTED) };
        assert!(requested(at));
        // SAFETY: as above.
        unsafe { state.write(3) };
        assert!(!requested(at));
    }
}
