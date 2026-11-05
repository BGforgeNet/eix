//! LowercaseTable: game.exe's C runtime, once the game has set its LC_CTYPE locale, lowers each character by an
//! `LCMapStringA` round trip in `_tolower_lk` 0x6EBAC2, which `tolower`, `_stricmp`, `_strnicmp` and `_memicmp` call
//! per character (the script tokenizer's keyword scan and the archive name lookups among their callers). The fix
//! answers a byte from the 256 results of that same call under the locale in effect, made on the first call.
#![expect(unsafe_code, reason = "calling the original `_tolower_lk` it replaces")]

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use eix_core::engine_fixes::{self, LowerTable};
use eix_engine::game::crt::{self, CtypeLocale, TOLOWER_LK};
use eix_engine::hook;

use crate::groups::{BuildError, Labelled};
use crate::log;
use crate::setup::Setup;

/// `_tolower_lk`'s first instructions: `push ebp; mov ebp, esp; push ecx; cmp dword [__lc_handle + 8], 0`.
const ORIGINAL: [u8; 11] = [
    0x55, 0x8B, 0xEC, 0x51, 0x83, 0x3D, 0xF0, 0xAB, 0x7C, 0x00, 0x00,
];

type ToLowerLk = unsafe extern "C" fn(i32) -> i32;

/// The trampoline that runs the original `_tolower_lk`.
static ORIGINAL_FN: AtomicUsize = AtomicUsize::new(0);
/// The table of the first locale the game set; another locale after it gets the original's answers.
static TABLE: OnceLock<LowerTable<CtypeLocale>> = OnceLock::new();

fn original(c: i32) -> i32 {
    // SAFETY: the trampoline runs the original, which has this type.
    let f: ToLowerLk = unsafe { hook::as_fn(ORIGINAL_FN.load(Ordering::SeqCst)) };
    // SAFETY: called as the game calls it, inside its caller's locale guard.
    unsafe { f(c) }
}

extern "C" fn tolower_lk(c: i32) -> i32 {
    // SAFETY: the fix applies to RU 1.06 only, and every caller of `_tolower_lk` holds the locale guard.
    let locale = unsafe { crt::ctype_locale() };
    // The "C" locale's path is a range check, as cheap as the table.
    if locale.handle == 0 {
        return original(c);
    }
    let lowered = engine_fixes::lower(&TABLE, locale, c, original);
    if !LOGGED.load(Ordering::Relaxed)
        && let Some(table) = TABLE.get()
        && !LOGGED.swap(true, Ordering::Relaxed)
    {
        let built = table.locale();
        log::info(
            "fixes",
            &format!(
                "LowercaseTable: table built for locale {:#06x}, code page {}",
                built.handle, built.code_page
            ),
            "",
        );
    }
    lowered
}

/// Set once the built table is logged.
static LOGGED: AtomicBool = AtomicBool::new(false);

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let (write, trampoline) =
        hook::replace_function(TOLOWER_LK, &ORIGINAL, tolower_lk as *const () as usize)?;
    ORIGINAL_FN.store(trampoline, Ordering::SeqCst);
    Ok(vec![Labelled {
        label: "LowercaseTable".to_owned(),
        write,
    }])
}
