//! AllowScriptArgsInLoops: the body of a `For` / `ForIf` loop sees the enclosing script's locals.
//!
//! `CScript::LoadCommands` 0x4D1330 loads a loop's body by calling itself (0x4D161B) with 0 as the locals argument;
//! the hook at 0x4D1614 puts the caller's own locals (its stack word +0x19C) in that slot (+0xC), and leaves them in
//! EAX as the original's stub does.

use eix_engine::hook::{self, Context, Mode};

use super::{BuildError, Labelled, Setup};
use crate::log;

/// `mov [esp+0xDC], eax`, before the recursive call's `call 0x4D1330`.
const SITE: usize = eix_sites::C_SCRIPT_LOAD_COMMANDS__2E4;
const ORIGINAL: [u8; 7] = [0x89, 0x84, 0x24, 0xDC, 0x00, 0x00, 0x00];

/// The caller's locals, as a stack word index at the site.
const LOCALS: usize = 0x19C / 4;
/// The locals argument of the recursive call (its fourth word).
const ARGUMENT: usize = 0xC / 4;

extern "C" fn pass_locals(_: usize, ctx: &mut Context) {
    let Some(locals) = ctx.stack_word(LOCALS) else {
        log::error(
            "scripts",
            "the loop's locals are off the stack",
            "0x004D1614",
        );
        return;
    };
    ctx.set_stack_word(ARGUMENT, locals);
    ctx.eax = locals;
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "AllowScriptArgsInLoops #0".to_owned(),
        write: hook::context_hook(SITE, &ORIGINAL, Mode::Before, pass_locals, 0)?,
    }])
}
