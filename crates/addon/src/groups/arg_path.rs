//! A call whose first argument is a file path the game passes: a rule may give the call another path instead.
#![expect(
    unsafe_code,
    reason = "reading the path the game passes and the hook's own state from the handler's argument"
)]

use std::ffi::{CStr, CString};
use std::sync::Mutex;

use eix_engine::hook::{self, Context};

use super::{BuildError, Labelled};
use crate::log;

/// The path the call gets instead of `game`'s, or `None` to keep it.
pub(crate) type Rule = fn(game: &[u8]) -> Option<Vec<u8>>;

/// One hook's rule and the path it last handed out, which stays alive until its next call.
struct Rewrite {
    label: &'static str,
    rule: Rule,
    last: Mutex<Option<CString>>,
}

/// The NUL-terminated string at `address`, or `None` for null.
///
/// # Safety
/// `address` is null or points at a NUL-terminated string that lives for the call.
pub(crate) unsafe fn c_str<'a>(address: u32) -> Option<&'a CStr> {
    let ptr = core::ptr::with_exposed_provenance::<core::ffi::c_char>(address as usize);
    // SAFETY: the caller guarantees null or a live NUL-terminated string.
    (!ptr.is_null()).then(|| unsafe { CStr::from_ptr(ptr) })
}

extern "C" fn handler(arg: usize, ctx: &mut Context) {
    // SAFETY: `arg` is the address of the `Rewrite` leaked in `rewrite_first_arg`, which lives for the process.
    let rewrite = unsafe { &*core::ptr::with_exposed_provenance::<Rewrite>(arg) };
    let Some(word) = ctx.stack_word(1) else {
        return;
    };
    // SAFETY: at each site using this hook the first argument is the file name, a NUL-terminated string that
    // lives for the call (checked against the game's code at the site).
    let Some(game) = (unsafe { c_str(word) }) else {
        return;
    };
    let Some(path) = (rewrite.rule)(game.to_bytes()) else {
        return;
    };
    let Ok(path) = CString::new(path) else {
        log::error("groups", "a mod path holds a NUL", rewrite.label);
        return;
    };
    let mut last = rewrite
        .last
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let address = path.as_ptr().expose_provenance() as u32;
    // The game copies the path during the call; the string stays alive until the hook's next call replaces it.
    *last = Some(path);
    ctx.set_stack_word(1, address);
}

/// The call at `site` (bytes `original`) gets `rule`'s path in place of its first argument when the rule gives one.
pub(crate) fn rewrite_first_arg(
    label: &'static str,
    site: usize,
    original: [u8; 5],
    rule: Rule,
) -> Result<Labelled, BuildError> {
    let rewrite: &'static Rewrite = Box::leak(Box::new(Rewrite {
        label,
        rule,
        last: Mutex::new(None),
    }));
    let arg = core::ptr::from_ref(rewrite).expose_provenance();
    let (write, _) = hook::call_hook(site, original, handler, arg)?;
    Ok(Labelled {
        label: label.to_owned(),
        write,
    })
}
