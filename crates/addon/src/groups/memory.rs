//! Reads of the game's memory at a hooked site, for the words the replaced instructions read.
#![expect(unsafe_code, reason = "reading game memory the hooked code addresses")]

use eix_engine::hook::Context;

use crate::log;

/// The 4-byte word at `address`.
///
/// # Safety
/// `address` must be a readable word of the game's, as the code at the hooked site reads it, on the game's thread.
pub(crate) unsafe fn word(address: usize) -> u32 {
    // SAFETY: the caller's guarantee.
    unsafe { core::ptr::with_exposed_provenance::<u32>(address).read_unaligned() }
}

/// Word `index` of the stack at the hooked site, which a replaced `mov reg, [esp+4*index]` read.
pub(crate) fn stack_word(ctx: &Context, index: usize) -> u32 {
    ctx.stack_word(index).unwrap_or_else(|| {
        log::error(
            "groups",
            "a hooked site's stack word is off the stack",
            &index.to_string(),
        );
        0
    })
}
