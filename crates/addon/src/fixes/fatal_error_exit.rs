//! FatalErrorExit: a fatal error ends the game with its streams flushed and `ExitProcess`, where the C runtime's
//! `exit` 0x6E8DA7 also ran the game's static destructors, which fault in the middle of play (the crash box after the
//! error's own box). `FatalError` 0x480080 calls `exit(-1)` at 0x4800CD (AntiCrash skips it), and the HRESULT twin
//! 0x4800E0 at 0x48015C (which AntiCrash does not reach). Ours flushes what the game's streams hold (`_flushall`
//! 0x6ED5ED, which `exit` runs too, through `__endstdio`) and leaves the destructors and the heap teardown out.
#![expect(unsafe_code, reason = "calls the game's C runtime flush")]

use eix_engine::{hook, win};
use eix_sites::Site;

use crate::groups::{BuildError, Labelled};
use crate::log;
use crate::setup::Setup;

/// `call exit` in FatalError and in its HRESULT twin, each after `push -1`.
const EXIT_CALLS: [(Site, [u8; 5]); 2] = [
    (eix_sites::FATAL_ERROR__4D, [0xE8, 0xD5, 0x8C, 0x26, 0x00]),
    (
        eix_sites::FATAL_ERROR_CODE__7C,
        [0xE8, 0x46, 0x8C, 0x26, 0x00],
    ),
];
/// The game's `_flushall`.
const FLUSH_ALL: Site = eix_sites::TEXT_6ED5ED;

type FlushAll = unsafe extern "C" fn() -> i32;

extern "C" fn fatal_exit(code: i32) -> ! {
    // SAFETY: the game's own `_flushall`, of this type, on its own thread, as `exit` would call it.
    let flush = unsafe { hook::as_fn::<FlushAll>(FLUSH_ALL) };
    // SAFETY: as above.
    unsafe { flush() };
    log::info(
        "fixes",
        &format!("fatal error: the game ends with code {code}"),
        "FatalErrorExit",
    );
    // The process's exit code is the game's -1, as `exit` passes it on.
    win::exit_process(code as u32)
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    build_calls(&EXIT_CALLS)
}

fn build_calls(calls: &[(Site, [u8; 5]); 2]) -> Result<Vec<Labelled>, BuildError> {
    let to = fatal_exit as *const () as usize;
    calls
        .iter()
        .enumerate()
        // AntiCrash (DisableExitOnCrashReport, applied before the fixes) NOPs FatalError's call: nothing to redirect.
        .filter(|(i, (site, _))| *i != 0 || !crate::game::holds(*site, &[0x90; 5]))
        .map(|(i, &(site, bytes))| {
            let (write, _) = hook::redirect_call(site, bytes, to)?;
            Ok(Labelled {
                label: format!("FatalErrorExit #{i}"),
                write,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eix_engine::group::{self, Outcome};

    fn apply_calls(found: &mut [[u8; 5]; 2]) -> group::Report {
        let calls = [
            (Site::of(found[0].as_ptr() as usize), EXIT_CALLS[0].1),
            (Site::of(found[1].as_ptr() as usize), EXIT_CALLS[1].1),
        ];
        let writes = build_calls(&calls).unwrap();
        // SAFETY: both sites are live, private buffers; their bytes are never executed.
        unsafe {
            group::apply("FatalErrorExit", true, || {
                Ok(writes.into_iter().map(|l| l.write).collect())
            })
        }
    }

    #[test]
    fn anticrash_nops_skip_only_the_first_call() {
        let mut found = [[0x90; 5], EXIT_CALLS[1].1];
        assert_eq!(apply_calls(&mut found).outcome, Outcome::Applied);
        assert_eq!(found[0], [0x90; 5]);
        assert_ne!(found[1], EXIT_CALLS[1].1);
    }

    #[test]
    fn unexpected_bytes_fail_the_whole_group() {
        for (index, bytes) in [
            (0, [0xCC; 5]),
            (0, [0x90, 0x90, 0x90, 0x90, 0xCC]),
            (1, [0x90; 5]),
        ] {
            let mut found = EXIT_CALLS.map(|(_, bytes)| bytes);
            found[index] = bytes;
            let before = found;
            let report = apply_calls(&mut found);
            assert!(matches!(report.outcome, Outcome::Failed { .. }), "{report}");
            assert_eq!(found, before);
            let text = report.to_string();
            assert!(text.contains("expected E8"), "{text}");
            assert!(text.contains("but found"), "{text}");
        }
    }

    #[test]
    fn both_calls_were_exit() {
        for (site, bytes) in EXIT_CALLS {
            let rel = i32::from_le_bytes(bytes[1..].try_into().unwrap());
            assert_eq!(
                Site::new((site.offset(5)).ru().wrapping_add_signed(rel as isize)),
                eix_sites::TEXT_6E8DA7
            );
        }
    }
}
