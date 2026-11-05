//! SendSpellEffectValueToPlayers: each effect record of a map object's network state carries its value as well, so
//! clients see the host's figure. The writer and the reader each gain a 4-byte field after the record's four.
#![expect(
    unsafe_code,
    reason = "reading the game's frames and calling its stream functions at the hooked sites"
)]

use std::ffi::c_void;
use std::mem::offset_of;

use eix_engine::game::net::DeltaStream;
use eix_engine::game::unit::MapEffect;
use eix_engine::hook::{self, Context, Mode};

use super::memory::{stack_word, word};
use super::{BuildError, Labelled, Setup};

/// The record's value, from ESI, which both loops leave at the record's ending field after its last read or write.
fn value(esi: u32) -> *mut f32 {
    let at = esi as usize - offset_of!(MapEffect, ending) + offset_of!(MapEffect, value);
    core::ptr::with_exposed_provenance_mut(at)
}

/// #0, `MapObjectWriteNetState` 0x50EC04 (EDI the stream, EBX the object): writes the value, then the replaced
/// `mov ecx, [esp+0x14]; mov eax, [ebx+0x19c]`.
extern "C" fn write_value(_: usize, ctx: &mut Context) {
    let stream = core::ptr::with_exposed_provenance_mut::<DeltaStream>(ctx.edi as usize);
    // SAFETY: at 0x50EC04 the stream is open and ESI points into the record just written.
    unsafe { DeltaStream::write_both(stream, value(ctx.esi).cast::<c_void>(), 4) };
    // The replaced instructions: the loop's offset on the stack, then the object's effect count.
    ctx.ecx = stack_word(ctx, 5);
    // SAFETY: EBX is the object whose effects the loop writes.
    ctx.eax = unsafe { word(ctx.ebx as usize + 0x19C) };
}

/// #1, `CMapObject::ReadNetState` 0x50EF8E (EBX the stream): the value is cleared, then read, then the replaced
/// `mov eax, [esp+0x1c]; mov edx, [esp+0x18]`.
extern "C" fn read_value(_: usize, ctx: &mut Context) {
    let stream = core::ptr::with_exposed_provenance_mut::<DeltaStream>(ctx.ebx as usize);
    let field = value(ctx.esi);
    // SAFETY: at 0x50EF8E ESI points into the record just read, which the game owns, on its thread.
    unsafe { field.write(0.0) };
    // SAFETY: the stream is the one the record was read from.
    unsafe { DeltaStream::read_raw(stream, field.cast::<c_void>(), 4) };
    // The replaced instructions: the loop's counter and offset on the stack.
    ctx.eax = stack_word(ctx, 7);
    ctx.edx = stack_word(ctx, 6);
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![
        Labelled {
            label: "SendSpellEffectValueToPlayers #0".to_owned(),
            write: hook::context_hook(
                eix_sites::MAP_OBJECT_WRITE_NET_STATE__114,
                &[0x8B, 0x4C, 0x24, 0x14, 0x8B, 0x83, 0x9C, 0x01, 0x00, 0x00],
                Mode::Drop,
                write_value,
                0,
            )?,
        },
        Labelled {
            label: "SendSpellEffectValueToPlayers #1".to_owned(),
            write: hook::context_hook(
                eix_sites::C_MAP_OBJECT_READ_NET_STATE__35E,
                &[0x8B, 0x44, 0x24, 0x1C, 0x8B, 0x54, 0x24, 0x18],
                Mode::Drop,
                read_value,
                0,
            )?,
        },
    ])
}
