//! CastTargetGone: `CUnitServer::ActionInReach` 0x549D40 answers "not in reach" for a cast order (kind 5) whose
//! target is gone, where a spell castable at a point sent it on to the distance test, which reads the null target's
//! vtable (0x549E2D). Every other missing-target case of the function already answers "not in reach".

use eix_engine::group::SiteWrite;

use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// `jne 0x549E27` after the spell's point-cast flag test (`test [eax + 0x3C], 0x20000000`). Only a null target
/// reaches it (0x549DC7 sends a set one to 0x549DF0); without the jump the code goes on to the function's
/// `return false` at 0x549DE8.
const POINT_CAST_JUMP: usize = eix_sites::C_UNIT_SERVER_ACTION_IN_REACH__A6;

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    writes()
}

fn writes() -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "CastTargetGone #0".to_owned(),
        write: SiteWrite::nop(POINT_CAST_JUMP, &[0x75, 0x3F]),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_jump_becomes_two_nops() {
        let w = writes().unwrap();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].write.site(), POINT_CAST_JUMP);
        assert_eq!(w[0].write.expected(), &[0x75, 0x3F]);
        assert_eq!(w[0].write.new_bytes(), &[0x90, 0x90]);
    }
}
