//! Composition of handlers at the interface's string-event site.
#![expect(
    unsafe_code,
    reason = "reading the immutable handler pair kept by a hook stub"
)]

use std::sync::OnceLock;

use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, Handler, HookError, Mode};

static STRING_EVENTS: OnceLock<Handler> = OnceLock::new();

/// Builds the addon's string-event hook and keeps its handler available to the camp.
pub fn string_event_hook(handler: Handler) -> Result<SiteWrite, HookError> {
    let _ = STRING_EVENTS.set(handler);
    hook::context_hook(
        eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__7F,
        &[0x83, 0xC4, 0x08, 0x85, 0xC0],
        Mode::Before,
        handler,
        0,
    )
}

struct Dispatch {
    camp: Handler,
    strip: usize,
    other: Option<Handler>,
}

extern "C" fn dispatch(arg: usize, ctx: &mut Context) {
    // SAFETY: the stub keeps the immutable pair allocated by `compose` for the process lifetime.
    let handlers = unsafe { &*(arg as *const Dispatch) };
    let resume = ctx.resume;
    (handlers.camp)(handlers.strip, ctx);
    if ctx.resume == resume
        && let Some(other) = handlers.other
    {
        other(0, ctx);
    }
}

fn compose(
    camp: Handler,
    strip: usize,
    other: Option<Handler>,
) -> Result<(SiteWrite, usize), HookError> {
    let handlers = Box::leak(Box::new(Dispatch { camp, strip, other }));
    let arg = handlers as *const Dispatch as usize;
    Ok((
        hook::context_hook(
            eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__7F,
            &[0x83, 0xC4, 0x08, 0x85, 0xC0],
            Mode::Before,
            dispatch,
            arg,
        )?,
        arg,
    ))
}

pub(crate) fn camp_hook(camp: Handler, strip: usize) -> Result<SiteWrite, HookError> {
    compose(camp, strip, STRING_EVENTS.get().copied()).map(|(write, _)| write)
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn camp(strip: usize, ctx: &mut Context) {
        if ctx.eax == 1 {
            ctx.edx += strip as u32;
            ctx.resume = eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__1E9;
        }
    }

    extern "C" fn game_over(_: usize, ctx: &mut Context) {
        if ctx.eax == 2 {
            ctx.edx += 10;
            ctx.resume = eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__1E9;
        }
    }

    #[test]
    fn both_builds_leave_a_handler_that_dispatches_both_events() {
        let first = string_event_hook(game_over).unwrap();
        let (last, arg) = compose(camp, 3, STRING_EVENTS.get().copied()).unwrap();
        assert_eq!(first.site(), last.site());
        assert_eq!(first.expected(), last.expected());
        // SAFETY: the context consists only of integer registers.
        let mut ctx: Context = unsafe { core::mem::zeroed() };
        ctx.eax = 1;
        dispatch(arg, &mut ctx);
        assert_eq!(ctx.edx, 3);
        ctx.resume = eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__7F.offset(5);
        ctx.eax = 2;
        dispatch(arg, &mut ctx);
        assert_eq!(ctx.edx, 13);
        assert_eq!(ctx.resume, eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__1E9);
    }
}
