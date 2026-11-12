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

fn compose(camp: Handler, strip: usize, other: Option<Handler>) -> Result<SiteWrite, HookError> {
    let handlers = Box::leak(Box::new(Dispatch { camp, strip, other }));
    let arg = handlers as *const Dispatch as usize;
    hook::context_hook(
        eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__7F,
        &[0x83, 0xC4, 0x08, 0x85, 0xC0],
        Mode::Before,
        dispatch,
        arg,
    )
}

pub(crate) fn camp_hook(camp: Handler, strip: usize) -> Result<SiteWrite, HookError> {
    compose(camp, strip, STRING_EVENTS.get().copied())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    fn event(ctx: &Context) -> &[u8] {
        // SAFETY: the synthetic caller passes a static NUL-terminated event name in EAX.
        unsafe { CStr::from_ptr(ctx.eax as *const core::ffi::c_char) }.to_bytes()
    }

    extern "C" fn camp(strip: usize, ctx: &mut Context) {
        if event(ctx) == b"constr" {
            ctx.edx += strip as u32;
            ctx.resume = eix_sites::Site::of(ctx.esi as usize);
        }
    }

    extern "C" fn game_over(_: usize, ctx: &mut Context) {
        if event(ctx) == b"gameover" {
            ctx.edx += 10;
            ctx.resume = eix_sites::Site::of(ctx.esi as usize);
        }
    }

    #[test]
    fn both_builds_leave_a_handler_that_dispatches_both_events() {
        let first = string_event_hook(game_over).unwrap();
        let last = camp_hook(camp, 3).unwrap();
        assert_eq!(first.site(), last.site());
        assert_eq!(first.expected(), last.expected());
        // Enter the final jump's actual stub with the two words its copied `add esp, 8` consumes.
        let displacement = u32::from_le_bytes(last.new_bytes()[1..5].try_into().unwrap());
        let stub = last
            .site()
            .ru32()
            .wrapping_add(5)
            .wrapping_add(displacement);
        let wrapper = eix_engine::exec::place::<String>(31, |at| {
            // mov eax,[esp+4]; push esi; mov esi,cleanup; xor edx,edx; push 0; push 0; jmp stub
            let mut code = vec![0x8B, 0x44, 0x24, 4, 0x56, 0xBE];
            code.extend_from_slice(&((at + 26) as u32).to_le_bytes());
            code.extend_from_slice(&[0x31, 0xD2, 0x6A, 0, 0x6A, 0]);
            code.extend_from_slice(&eix_engine::patch::jmp_rel32(
                eix_sites::Site::new(at + 16),
                stub as usize,
            ));
            code.extend_from_slice(&[0x90; 5]);
            // cleanup: mov eax,edx; pop esi; ret; nop
            code.extend_from_slice(&[0x89, 0xD0, 0x5E, 0xC3, 0x90]);
            Ok(code)
        })
        .unwrap();
        // SAFETY: the wrapper takes one event pointer and returns its handler's result, preserving callee registers.
        let run: extern "C" fn(*const core::ffi::c_char) -> u32 =
            unsafe { hook::as_fn_at(wrapper) };
        assert_eq!(run(c"constr".as_ptr()), 3);
        assert_eq!(run(c"gameover".as_ptr()), 10);
    }
}
