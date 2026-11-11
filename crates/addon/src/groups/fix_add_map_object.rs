//! FixAddMapObject: `CWorldServer::CreateObject` 0x5927C0 reports a unit whose prototype has no record and takes
//! index 0 instead of reading the record's +8 through null.
#![expect(
    unsafe_code,
    reason = "reading the prototype record and the unit's name the game holds"
)]

use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use super::arg_path::c_str;
use super::{BuildError, Labelled, Setup};
use crate::report;

/// `mov esi, [esi + 8]; add edx, ecx`: ESI is the prototype's record (null when there is none), EDX:ECX the start
/// of the division by the record size that follows.
const SITE: Site = eix_sites::C_WORLD_SERVER_CREATE_OBJECT__7A8;
const ORIGINAL: [u8; 5] = [0x8B, 0x76, 0x08, 0x03, 0xD1];
/// EBX + 0x20C: the prototype's name. EBX is the function's first argument, the description of the object to
/// create; 0x592EFE takes the name's length to look the prototype up.
const PROTOTYPE_NAME: u32 = 0x20C;

extern "C" fn handler(_: usize, ctx: &mut Context) {
    if ctx.esi == 0 {
        let at = core::ptr::with_exposed_provenance::<u32>(
            ctx.ebx.wrapping_add(PROTOTYPE_NAME) as usize
        );
        // SAFETY: at 0x592F68 EBX is the description the game passed, whose +0x20C holds the name's address.
        let name = unsafe { at.read_unaligned() };
        // SAFETY: the word is the name, a NUL-terminated string the description keeps.
        let name = unsafe { c_str(name) }.map_or(&b""[..], |n| n.to_bytes());
        report::error(&[&b"Invalid unit's prototype: \""[..], name, b"\""].concat());
    } else {
        let at = core::ptr::with_exposed_provenance::<u32>(ctx.esi.wrapping_add(8) as usize);
        // SAFETY: a non-null ESI is the prototype's record, whose +8 is its index.
        ctx.esi = unsafe { at.read_unaligned() };
    }
    ctx.edx = ctx.edx.wrapping_add(ctx.ecx);
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    report::init(&setup.options);
    Ok(vec![Labelled {
        label: "FixAddMapObject #0".to_owned(),
        write: hook::context_hook(SITE, &ORIGINAL, Mode::Drop, handler, 0)?,
    }])
}
