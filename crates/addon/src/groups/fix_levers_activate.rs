//! FixLeversActivate: `LockKey::Opens` 0x522A80 compares the key's and the lock's levels signed, and a level-0 lock
//! answers the key's level + 1 (core's [`lock_opens`]).

use eix_core::fixes::lock_opens;
use eix_engine::hook::{self, Context, Mode};

use super::{BuildError, Labelled, Setup};

/// The compare and the return: `cmp eax, edx; pop esi; sbb eax, eax; inc eax; ret 4`, with the key's level in EAX
/// and the lock's in EDX.
const SITE: usize = eix_sites::LOCK_KEY_OPENS__36;
const ORIGINAL: [u8; 9] = [0x3B, 0xC2, 0x5E, 0x1B, 0xC0, 0x40, 0xC2, 0x04, 0x00];
/// `pop esi; ret 4`, the function's other exit, which returns EAX as it is.
const EXIT: usize = eix_sites::LOCK_KEY_OPENS__28;

extern "C" fn handler(_: usize, ctx: &mut Context) {
    ctx.eax = lock_opens(ctx.eax as i32, ctx.edx as i32);
    ctx.resume = EXIT;
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "FixLeversActivate #0".to_owned(),
        write: hook::context_hook(SITE, &ORIGINAL, Mode::Drop, handler, 0)?,
    }])
}
