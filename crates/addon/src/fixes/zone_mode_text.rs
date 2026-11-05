//! ZoneModeText: the error for a zone theme missing from the music list shows its text as text. When
//! `CMusicSystem::SetZoneMode` 0x6B4E30 finds no theme of the zone's name, it builds a message from that name and
//! passes it to `FatalError` 0x480080 as the format (0x6B4EEC), so a `%` in a mod's theme name read arguments that
//! were never passed. Ours passes `"%s"` and the message.
#![expect(unsafe_code, reason = "calls the game's FatalError")]

use eix_engine::hook;

use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// `call FatalError` with the message on the stack.
const CALL: usize = eix_sites::C_MUSIC_SYSTEM_SET_ZONE_MODE__BC;
const CALL_BYTES: [u8; 5] = [0xE8, 0x8F, 0xB1, 0xDC, 0xFF];

/// `FatalError(format, ...)`: cdecl, its result in EAX.
type FatalError = unsafe extern "C" fn(*const u8, ...) -> u32;

extern "C" fn fatal_error(text: *const u8) -> u32 {
    // SAFETY: the call's target is `FatalError`, of this type.
    let fatal = unsafe { hook::as_fn::<FatalError>(eix_sites::FATAL_ERROR) };
    // SAFETY: `text` is the game's message, a C string, for the one `%s`.
    unsafe { fatal(c"%s".as_ptr().cast(), text) }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let (write, _) = hook::redirect_call(CALL, CALL_BYTES, fatal_error as *const () as usize)?;
    Ok(vec![Labelled {
        label: "ZoneModeText #0".to_owned(),
        write,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_call_went_to_fatal_error() {
        let rel = i32::from_le_bytes(CALL_BYTES[1..].try_into().unwrap());
        assert_eq!(
            (CALL + 5).wrapping_add_signed(rel as isize),
            eix_sites::FATAL_ERROR
        );
    }
}
