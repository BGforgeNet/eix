//! NoAudioDevice: without an audio device the game runs silent, as with its `Audio` option at 0, where it crashed in
//! the first movie. `CSoundManager::Open` 0x6B6A30 returns 0 when `AIL_waveOutOpen` fails (0x6B6B44), with the
//! failed call's driver handle left at +0x1C; start-up (`GameMain` 0x456E98) ignores the result, and a movie
//! (`PlayMovie` 0x5DB473, `CBinkVideo::Open` 0x5DBAC9) hands that handle to Bink, which faults in
//! `AIL_allocate_sample_handle`. Ours clears the handle on a failed open and makes the readers of the `Audio` option
//! (0x79BCC0) that Open does not guard, the movies' and a zone load's (`CWorldClient::LoadZone` 0x56806D, 0x568099,
//! 0x56811A), read 0, as when the option is off. The option itself stays as set, since the game saves it.
#![expect(
    unsafe_code,
    reason = "calls the game's sound open and reads and writes its sound state"
)]

use core::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use eix_core::x86_flags;
use eix_engine::hook::{self, Context, Mode};

use super::Warnings;
use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// `call CSoundManager::Open`.
const OPEN_CALL: usize = eix_sites::GAME_MAIN__A58;
const OPEN_CALL_BYTES: [u8; 5] = [0xE8, 0x93, 0xFB, 0x25, 0x00];
/// The manager's Miles driver handle.
const DRIVER: usize = 0x1C;
/// The `Audio` option: 0 off.
const AUDIO: usize = eix_sites::DATA_79BCC0;
/// The option's readers: `cmp [0x79BCC0], <reg>` (the register in the ModRM byte) and one `mov eax, [0x79BCC0]`.
const COMPARES: [(usize, [u8; 6]); 4] = [
    (
        eix_sites::C_WORLD_CLIENT_LOAD_ZONE__5BD,
        [0x39, 0x3D, 0xC0, 0xBC, 0x79, 0x00],
    ),
    (
        eix_sites::C_WORLD_CLIENT_LOAD_ZONE__5E9,
        [0x39, 0x3D, 0xC0, 0xBC, 0x79, 0x00],
    ),
    (
        eix_sites::PLAY_MOVIE__83,
        [0x39, 0x35, 0xC0, 0xBC, 0x79, 0x00],
    ),
    (
        eix_sites::C_BINK_VIDEO_OPEN__79,
        [0x39, 0x1D, 0xC0, 0xBC, 0x79, 0x00],
    ),
];
const LOAD: usize = eix_sites::C_WORLD_CLIENT_LOAD_ZONE__66A;
const LOAD_BYTES: [u8; 5] = [0xA1, 0xC0, 0xBC, 0x79, 0x00];

type Open = unsafe extern "thiscall" fn(*mut c_void, *mut c_void, u32, i32, u32, u32) -> u32;

static OPEN: AtomicUsize = AtomicUsize::new(0);
/// Set once Open has failed.
static NO_DEVICE: AtomicBool = AtomicBool::new(false);
static WARNINGS: Warnings = Warnings::new("NoAudioDevice");

extern "thiscall" fn open(
    manager: *mut c_void,
    window: *mut c_void,
    mode: u32,
    device: i32,
    rate: u32,
    bits: u32,
) -> u32 {
    // SAFETY: OPEN holds the call's old target, `CSoundManager::Open`, of this type; the arguments are the game's.
    let original = unsafe { hook::as_fn::<Open>(OPEN.load(Ordering::Relaxed)) };
    // SAFETY: as above.
    let opened = unsafe { original(manager, window, mode, device, rate, bits) };
    if opened == 0 {
        // SAFETY: the game's live sound manager; Open is done with the handle.
        unsafe { manager.wrapping_byte_add(DRIVER).cast::<usize>().write(0) };
        NO_DEVICE.store(true, Ordering::Relaxed);
        WARNINGS.warn(|| "no audio device: the game runs silent".to_owned());
    }
    opened
}

/// The option as the readers see it.
fn audio() -> u32 {
    if NO_DEVICE.load(Ordering::Relaxed) {
        0
    } else {
        // SAFETY: the game's option word, which the main thread alone writes.
        unsafe { core::ptr::with_exposed_provenance::<u32>(eix_sites::here(AUDIO)).read() }
    }
}

/// The ModRM byte of `cmp [disp32], <reg>`: EDI, ESI and EBX.
const REG_EDI: usize = 0x3D;
const REG_ESI: usize = 0x35;
const REG_EBX: usize = 0x1D;

/// `cmp [Audio], <reg>`, `register` the ModRM byte that names the register.
extern "C" fn compare(register: usize, ctx: &mut Context) {
    let value = match register {
        REG_EDI => ctx.edi,
        REG_ESI => ctx.esi,
        _ => ctx.ebx,
    };
    ctx.eflags = x86_flags::cmp(ctx.eflags, audio(), value);
}

/// `mov eax, [Audio]`.
extern "C" fn load(_: usize, ctx: &mut Context) {
    ctx.eax = audio();
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let (open_write, old) =
        hook::redirect_call(OPEN_CALL, OPEN_CALL_BYTES, open as *const () as usize)?;
    OPEN.store(old, Ordering::Relaxed);
    let mut writes = vec![Labelled {
        label: "NoAudioDevice #0".to_owned(),
        write: open_write,
    }];
    for (i, (site, bytes)) in COMPARES.into_iter().enumerate() {
        let register = usize::from(bytes[1]);
        debug_assert!([REG_EDI, REG_ESI, REG_EBX].contains(&register));
        writes.push(Labelled {
            label: format!("NoAudioDevice #{}", i + 1),
            write: hook::context_hook(site, &bytes, Mode::Drop, compare, register)?,
        });
    }
    writes.push(Labelled {
        label: format!("NoAudioDevice #{}", COMPARES.len() + 1),
        write: hook::context_hook(LOAD, &LOAD_BYTES, Mode::Drop, load, 0)?,
    });
    Ok(writes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_call_goes_to_open_and_each_compare_names_its_register() {
        let rel = i32::from_le_bytes(OPEN_CALL_BYTES[1..].try_into().unwrap());
        assert_eq!(
            (OPEN_CALL + 5).wrapping_add_signed(rel as isize),
            eix_sites::C_SOUND_MANAGER_OPEN
        );
        let registers: Vec<usize> = COMPARES.iter().map(|(_, b)| usize::from(b[1])).collect();
        assert_eq!(registers, [REG_EDI, REG_EDI, REG_ESI, REG_EBX]);
    }
}
