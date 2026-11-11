//! KillScriptOutside: `ScriptRun::Execute` 0x4B6760 skips `KillScript`'s store of 1 when the pointer it stores
//! through (`[ebx + 0x10]`) is null: the command ran outside a script instance. The game reports the case through
//! `FatalError` 0x480080, which ends the game unless AntiCrash (DisableExitOnCrashReport) makes it return; then the
//! store went through the null pointer (0x4B6949). The report stays.

use eix_sites::Site;

use crate::groups::{BuildError, Labelled, bytes};
use crate::setup::Setup;

/// The `add esp, 4` after the `FatalError` call, which only the null case runs: it fell into the store at 0x4B6946,
/// the target of the set case's `jne`. `pop ecx` drops the argument as well (ECX is dead: `FatalError` clobbered it
/// and the shared tail 0x4B9BC5 sets it before reading it), and `jmp 0x4B694F` goes to the tail's jump.
const AFTER_REPORT: Site = eix_sites::SCRIPT_RUN_EXECUTE__1E3;

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    writes()
}

fn writes() -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![bytes(
        "KillScriptOutside #0",
        AFTER_REPORT,
        &[0x83, 0xC4, 0x04],
        // pop ecx; jmp 0x4B694F
        &[0x59, 0xEB, 0x09],
    )?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_null_case_jumps_past_the_store() {
        let w = writes().unwrap();
        let new = w[0].write.new_bytes();
        // The short jump at AFTER_REPORT + 1 lands on the `jmp 0x4B9BC5` after the 9-byte store at 0x4B6946.
        assert_eq!(
            AFTER_REPORT.offset(3).offset(new[2] as usize),
            eix_sites::SCRIPT_RUN_EXECUTE__1E6.offset(9)
        );
    }
}
