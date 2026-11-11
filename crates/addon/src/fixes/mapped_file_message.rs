//! MappedFileMessage: `CMappedFile::Open` 0x675770 formats a failed open's message with `sprintf` (0x67585B) into a
//! 256-byte stack buffer that ends at the return address, so a path over 210 bytes overran the frame. The format it
//! passes becomes the game's own text with the path's precision bounded, so the message fits and a long path is cut.

use std::ffi::CStr;

use eix_sites::Site;

use crate::groups::{BuildError, Labelled, pointer};
use crate::setup::Setup;

/// The operand of the `push 0x795314` that passes the format to `sprintf`.
const FORMAT_OPERAND: Site = eix_sites::C_MAPPED_FILE_OPEN__E6;
/// The game's format, in its read-only data.
const GAME_FORMAT: Site = eix_sites::DATA_795314;

/// The game's format at 0x795314, with `%s` bounded to the 210 bytes that fit (the tests derive them).
const FORMAT: &CStr = c"CMappedFile::Open().\nUnable to open file \"%.210s\".\n";

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![pointer(
        "MappedFileMessage #0",
        FORMAT_OPERAND,
        GAME_FORMAT.ru(),
        FORMAT.as_ptr() as usize,
    )])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The game's format, as read at 0x795314.
    const ORIGINAL: &[u8] = b"CMappedFile::Open().\nUnable to open file \"%s\".\n";
    /// The buffer: `[esp+0x48]` of a frame whose return address is at `[esp+0x148]` (0x138 bytes of locals and
    /// four saved registers).
    const BUFFER: usize = 0x100;
    /// The longest path that fits: the buffer less the format's 45 other characters and the NUL.
    const MAX_PATH_SHOWN: usize = 210;

    #[test]
    fn the_format_is_the_games_with_a_bounded_path() {
        let precision = format!("%.{MAX_PATH_SHOWN}s");
        let ours = String::from_utf8(ORIGINAL.to_vec())
            .unwrap()
            .replace("%s", &precision);
        assert_eq!(ours.as_bytes(), FORMAT.to_bytes());
    }

    #[test]
    fn the_longest_message_fills_the_buffer_with_its_nul() {
        let other = ORIGINAL.len() - "%s".len();
        assert_eq!(other + MAX_PATH_SHOWN + 1, BUFFER);
    }

    #[test]
    fn only_the_push_operand_changes() {
        let w = build(&Setup::default()).unwrap();
        assert_eq!(w[0].write.site(), FORMAT_OPERAND);
        assert_eq!(
            w[0].write.expected(),
            &eix_sites::DATA_795314.ru32().to_le_bytes()
        );
    }
}
