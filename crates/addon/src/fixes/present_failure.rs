//! PresentFailure: a frame the device cannot draw or show is dropped, where the game ended. `DevicePresent` 0x4EF610
//! flips with `DDFLIP_WAIT`, so a flip still pending from the last frame waits instead of failing
//! (DDERR_WASSTILLDRAWING, then `FatalError` "Can't flip render surface" in `FrameStep` 0x4580F5). A failed
//! `BeginScene` in `CSceneRenderer::BeginScene` 0x503F40 answers "not ready", which `FrameStep` already takes as
//! "skip this frame" (0x458084), where it ended the process through the HRESULT `FatalError` 0x4800E0, which
//! AntiCrash does not reach.

use std::sync::atomic::{AtomicU32, Ordering};

use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use crate::groups::{BuildError, Labelled, bytes};
use crate::log;
use crate::setup::Setup;

/// The flags argument of `IDirectDrawSurface::Flip`: `push 0`.
const FLIP_FLAGS: Site = eix_sites::DEVICE_PRESENT__1F;
/// `DDFLIP_WAIT`.
const DDFLIP_WAIT: u8 = 1;

/// BeginScene's error branch (an HRESULT in EAX that is neither 0 nor D3DERR_SCENE_IN_SCENE): `push "Can't begin
/// scene."`, which the hook replaces.
const SCENE_ERROR: Site = eix_sites::C_SCENE_RENDERER_BEGIN_SCENE__13D;
const SCENE_ERROR_BYTES: [u8; 5] = [0x68, 0xAC, 0xEE, 0x78, 0x00];
/// After it, `push eax; call 0x4800E0; add esp, 8`, then the function's `return 1` at 0x50408B.
const REPORT: Site = eix_sites::C_SCENE_RENDERER_BEGIN_SCENE__142;
const REPORT_BYTES: [u8; 9] = [0x50, 0xE8, 0x58, 0xC0, 0xF7, 0xFF, 0x83, 0xC4, 0x08];
/// `pop edi; pop ebp; pop ebx; xor eax, eax; pop esi; ret`: the epilogue of the `return 1` with 0 instead, the
/// answer of the function's own "not ready" exit (0x503F49).
const RETURN_NOT_READY: [u8; 9] = [0x5F, 0x5D, 0x5B, 0x33, 0xC0, 0x5E, 0xC3, 0xCC, 0xCC];

/// The last failure logged: a device that stays lost fails every frame, and one line per HRESULT is enough.
static LAST_LOGGED: AtomicU32 = AtomicU32::new(0);

extern "C" fn handler(_: usize, ctx: &mut Context) {
    if LAST_LOGGED.swap(ctx.eax, Ordering::Relaxed) != ctx.eax {
        log::warn(
            "fixes",
            &format!("BeginScene failed with 0x{:08X}: frames dropped", ctx.eax),
            "PresentFailure",
        );
    }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![
        bytes(
            "PresentFailure #0",
            FLIP_FLAGS,
            &[0x6A, 0x00],
            &[0x6A, DDFLIP_WAIT],
        )?,
        Labelled {
            label: "PresentFailure #1".to_owned(),
            write: hook::context_hook(SCENE_ERROR, &SCENE_ERROR_BYTES, Mode::Drop, handler, 0)?,
        },
        bytes(
            "PresentFailure #2",
            REPORT,
            &REPORT_BYTES,
            &RETURN_NOT_READY,
        )?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hook_falls_through_to_the_new_epilogue() {
        let w = build(&Setup::default()).unwrap();
        assert_eq!(w.len(), 3);
        // The hook's default resume is the byte after the five it covers: the start of the epilogue.
        assert_eq!(w[1].write.site().offset(w[1].write.size()), REPORT);
        assert_eq!(w[2].write.new_bytes(), &RETURN_NOT_READY);
    }

    #[test]
    fn the_report_call_is_the_hresult_fatal_error() {
        let rel = i32::from_le_bytes(REPORT_BYTES[2..6].try_into().unwrap());
        let target = Site::new((REPORT.offset(6)).ru().wrapping_add_signed(rel as isize));
        assert_eq!(target, eix_sites::FATAL_ERROR_CODE);
    }
}
