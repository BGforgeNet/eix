//! FixProcessLogin: `ServerBase::OnLogin` 0x443080 clears the age (+0x18) of the join record it builds on its
//! stack before passing it on (the record's address is in EDX at 0x4433D6).

use eix_engine::hook::{self, Context, Mode};

use super::{BuildError, Labelled, Setup};
use crate::log;

/// The age's offset in the join record.
const AGE: u32 = 0x18;

extern "C" fn handler(_: usize, ctx: &mut Context) {
    // The record lies on the stack at or above the site's ESP; the copied `push edx` ran first, so ESP is 4 lower.
    let index = ctx
        .edx
        .wrapping_add(AGE)
        .checked_sub(ctx.stack() as u32)
        .filter(|offset| offset % 4 == 0)
        .map(|offset| offset as usize / 4);
    if !index.is_some_and(|i| ctx.set_stack_word(i, 0)) {
        log::error(
            "groups",
            "FixProcessLogin: the join record is not on the stack",
            &format!("EDX {:#010x}", ctx.edx),
        );
    }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "FixProcessLogin".to_owned(),
        // push edx; mov [esp + 0x54], ecx
        write: hook::context_hook(
            eix_sites::SERVER_BASE_ON_LOGIN__356,
            &[0x52, 0x89, 0x4C, 0x24, 0x54],
            Mode::Before,
            handler,
            0,
        )?,
    }])
}
