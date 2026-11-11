//! ViewDepthInit: `CObject3D::Construct` 0x515BD0 zeroes the object's view position (+0x28, +0x2C) and depth
//! (+0x30), which it left as the heap gave them. An object listed in the draw list only for its shadow, before it was
//! ever in view, sorts on that depth; a leftover NaN makes `UnitOrderLess::Less` 0x574300 answer "less" for the
//! object against itself, and the sort's unguarded scans run off the vector.
#![expect(
    unsafe_code,
    reason = "clearing the three fields of the object the game constructed"
)]

use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// The constructor's tail, `mov eax, esi; pop esi; mov [ecx + 8], edx`: it runs first, so EAX holds the object.
const TAIL: Site = eix_sites::C_OBJECT_3D_CONSTRUCT__154;
const TAIL_BYTES: [u8; 6] = [0x8B, 0xC6, 0x5E, 0x89, 0x51, 0x08];

/// The view x, y and depth, three floats from +0x28; the constructor writes none of +0x1C..+0x34.
const VIEW_FIELDS: usize = 0x28;

extern "C" fn handler(_: usize, ctx: &mut Context) {
    let fields = core::ptr::with_exposed_provenance_mut::<[u32; 3]>(ctx.eax as usize + VIEW_FIELDS);
    // SAFETY: EAX is the object the constructor just built: a `CObject3D` of at least 0x16C bytes (its last store is
    // to +0x16C), whose fields +0x28..+0x34 nothing has read yet. Zero bits are 0.0 in each float.
    unsafe { fields.write_unaligned([0; 3]) };
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "ViewDepthInit #0".to_owned(),
        write: hook::context_hook(TAIL, &TAIL_BYTES, Mode::Before, handler, 0)?,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tail_becomes_a_jump_to_the_stub() {
        let w = build(&Setup::default()).unwrap();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].write.site(), TAIL);
        assert_eq!(w[0].write.expected(), &TAIL_BYTES);
        assert_eq!(w[0].write.new_bytes()[0], 0xE9);
    }
}
