//! The string functions and the two text commands. Texts are the game's ANSI bytes; lengths and positions count bytes.
#![expect(
    unsafe_code,
    reason = "printing to the game's console, announcing to its players and drawing from its generator"
)]

use std::ffi::CString;

use eix_core::script;
use eix_engine::float::ftol;
use eix_engine::game::mfc::{self, FormatArg};
use eix_engine::game::{console, crt, flow, net};
use eix_engine::win::change_case;

use super::{Call, invalid_arguments};

/// Every string argument appended in order (each up to its NUL).
fn joined(call: &Call) -> Vec<u8> {
    call.strings()
        .iter()
        .flat_map(|s| s.to_bytes())
        .copied()
        .collect()
}

/// The object's id in decimal.
pub(super) fn get_object_string_id(call: &Call) -> bool {
    let object = call.object(0);
    if object.is_null() {
        return invalid_arguments("GetObjectStringID");
    }
    // SAFETY: a bound object argument holds a live object.
    let id = unsafe { (*object).id };
    call.set_text(id.to_string().as_bytes());
    true
}

/// `%.<n>f` (n from the second argument, 3 by default, clamped to 0..8), then the original's trailing-zero cut. The
/// game's printf formats it, the original's own (static VS2010) runtime did: the same legacy MSVC algorithm.
pub(super) fn float_to_str(call: &Call) -> bool {
    let decimals = script::float_to_str_decimals((call.floats().len() >= 2).then(|| call.float(1)));
    let Ok(format) = CString::new(format!("%.{decimals}f")) else {
        return true;
    };
    let value = FormatArg::Double(f64::from(call.float(0)));
    // SAFETY: script functions run on the game's thread; the format reads the one double given.
    let mut text = unsafe { mfc::format_bytes(&format, &[value]) };
    script::trim_float_text(&mut text);
    call.set_text(&text);
    true
}

/// The C runtime's `atof` of the text (the game's; the original's own runtime has the same).
pub(super) fn str_to_float(call: &Call) -> bool {
    // SAFETY: script functions run on the game's thread.
    call.set_float(unsafe { crt::atof(call.string(0)) } as f32);
    true
}

pub(super) fn str_cat(call: &Call) -> bool {
    call.set_text(&joined(call));
    true
}

/// The length in bytes, from the string's header.
pub(super) fn str_len(call: &Call) -> bool {
    call.set_float(call.string_bytes(0).len() as f32);
    true
}

/// One of the strings, by the game generator's next value modulo their count.
pub(super) fn rand_str(call: &Call) -> bool {
    let strings = call.strings();
    let Ok(count) = u32::try_from(strings.len()) else {
        return true;
    };
    // The signature asks for two strings at least; the original divides by zero without any.
    if count == 0 {
        return true;
    }
    // SAFETY: script functions run on the game's thread, with the generator constructed.
    let index = unsafe { flow::random_next() } % count;
    if let Some(s) = strings.get(index as usize) {
        call.set_text(s.to_bytes());
    }
    true
}

/// 1 when the second text occurs in the first.
pub(super) fn str_has(call: &Call) -> bool {
    let (text, part) = (call.string(0).to_bytes(), call.string(1).to_bytes());
    let found = part.is_empty() || text.windows(part.len()).any(|w| w == part);
    call.set_float(if found { 1.0 } else { 0.0 });
    true
}

/// `StrMid(text, start, count)`: see [`script::str_mid`]; a negative count leaves the result as it was.
pub(super) fn str_mid(call: &Call) -> bool {
    let text = call.string_bytes(0);
    let len = i32::try_from(text.len()).unwrap_or(i32::MAX);
    let start = ftol(f64::from(call.float(0)));
    let count = ftol(f64::from(call.float(1)));
    if let Some((first, last)) = script::str_mid(len, start, count) {
        call.set_text(text.get(first..last).unwrap_or_default());
    }
    true
}

/// Lowered by user32's `CharLowerA`, which follows the user's code page.
pub(super) fn str_lower(call: &Call) -> bool {
    call.set_text(&change_case(call.string(0).to_bytes(), false));
    true
}

/// Raised by user32's `CharUpperA`.
pub(super) fn str_upper(call: &Call) -> bool {
    call.set_text(&change_case(call.string(0).to_bytes(), true));
    true
}

/// `Say(text, ...)`: in a network game, the joined texts go to every player as a chat line from the server.
pub(super) fn say(call: &Call) -> bool {
    // SAFETY: script commands run on the game's thread.
    if !unsafe { net::is_multiplayer_client() } {
        return true;
    }
    // SAFETY: as above.
    let Some(server) = (unsafe { net::live_server() }) else {
        return true;
    };
    if let Ok(text) = CString::new(joined(call)) {
        // SAFETY: the live game server, on the game's thread.
        unsafe { net::announce(server, &text) };
    }
    true
}

/// `ConsoleString(text, ...)` (extends the game's one-text command): the joined texts as a console line.
pub(super) fn console_string(call: &Call) -> bool {
    if let Ok(text) = CString::new(joined(call)) {
        // SAFETY: script commands run on the game's thread, with the console built.
        unsafe { console::print_line(&text) };
    }
    true
}
