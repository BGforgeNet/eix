//! ExtendedServerName: longer server names on the multiplayer connect screen. In `CMpConnectScreen::Create`
//! 0x61B2C0 the name field takes 50 characters in font class 15 (was 10, class 3, font 1) and the field and the
//! selection are wider; in the list (`DrawListVanilla` 0x61D780) the columns move right, and three of its text draws
//! go through the addon: the new game's row and each server's name are drawn shortened with an ellipsis (the name
//! within 140 pixels), the selected server's name wraps over up to three lines and moves the rows below it down.
//!
//! Only what is drawn changes; nothing is sent over the network.
#![expect(
    unsafe_code,
    reason = "calling the game's text draws from the redirected calls"
)]

use eix_engine::game::ui::{DRAW_TEXT_IN, DrawTextIn, Rect, Surface};
use eix_engine::hook;

use super::{BuildError, Labelled, Setup, bytes};

/// DrawText's `DT_END_ELLIPSIS | DT_MODIFYSTRING`.
const ELLIPSIS: u32 = 0x1_8000;
/// The original copies the text into a 90-byte buffer.
const TEXT_SIZE: usize = 90;
/// DrawText may write up to four characters of the ellipsis past the text it shortens.
const ELLIPSIS_ROOM: usize = 4;
/// Right edge of a server's name in the list.
const NAME_RIGHT: i32 = 140;
/// Lines the selected server's name may take, and their height.
const NAME_LINES: i32 = 3;
const ROW_HEIGHT: i32 = 24;

/// The bytes of the C string at `text`, at most `TEXT_SIZE - 1` of them.
///
/// The original's `strcpy_s` into its buffer ends the process for a longer text; here the text is cut.
///
/// # Safety
/// `text` must be a NUL-terminated string that lives for the call.
unsafe fn fitting(text: *const u8) -> [u8; TEXT_SIZE + ELLIPSIS_ROOM] {
    let mut buffer = [0u8; TEXT_SIZE + ELLIPSIS_ROOM];
    if text.is_null() {
        return buffer;
    }
    // SAFETY: the caller guarantees a live NUL-terminated string.
    let bytes = unsafe { core::ffi::CStr::from_ptr(text.cast()) }.to_bytes();
    let n = bytes.len().min(TEXT_SIZE - 1);
    buffer[..n].copy_from_slice(&bytes[..n]);
    buffer
}

/// The text drawn shortened with an ellipsis.
extern "thiscall" fn draw_ellipsis(
    surface: *mut Surface,
    text: *const u8,
    rect: *mut Rect,
    font: i32,
    colour: u32,
    format: u32,
    flag: i32,
) -> i32 {
    // SAFETY: the list passes its label, a live C string.
    let mut buffer = unsafe { fitting(text) };
    // SAFETY: the slot is the game's own DrawTextIn, which these three call sites called.
    let draw: DrawTextIn = unsafe { hook::as_fn(DRAW_TEXT_IN) };
    // SAFETY: the game's arguments, with a writable copy of the text DrawText may shorten in place.
    unsafe {
        draw(
            surface,
            buffer.as_mut_ptr(),
            rect,
            font,
            colour,
            format | ELLIPSIS,
            flag,
        )
    }
}

/// A server's name: within [`NAME_RIGHT`], with an ellipsis.
extern "thiscall" fn draw_name(
    surface: *mut Surface,
    text: *const u8,
    rect: *mut Rect,
    font: i32,
    colour: u32,
    format: u32,
    flag: i32,
) -> i32 {
    // SAFETY: the list passes its row rectangle, live for the call.
    let mut copy = unsafe { *rect };
    copy.right = NAME_RIGHT;
    draw_ellipsis(surface, text, &mut copy, font, colour, format, flag)
}

/// The selected server's name, wrapped; the rectangle moves down by the lines past the first.
extern "thiscall" fn draw_wrapped(
    surface: *mut Surface,
    text: *const u8,
    rect: *mut Rect,
    font: i32,
    colour: u32,
    format: u32,
    flag: i32,
) -> i32 {
    // SAFETY: the game's surface, text and rectangle, passed on to its own wrapped draw.
    let lines = unsafe {
        Surface::draw_wrapped_text(surface, text, rect, NAME_LINES, font, colour, format, flag)
    };
    let extra = lines.wrapping_mul(ROW_HEIGHT).wrapping_sub(ROW_HEIGHT);
    // SAFETY: as above; the list reads the moved rectangle for its next rows.
    let r = unsafe { &mut *rect };
    r.top = r.top.wrapping_add(extra);
    r.bottom = r.bottom.wrapping_add(extra);
    extra
}

/// The three calls of DrawTextIn the list makes through the addon, with their bytes.
const CALLS: [(&str, usize, [u8; 5]); 3] = [
    (
        "ExtendedServerName #9",
        eix_sites::C_MP_CONNECT_SCREEN_DRAW_LIST_VANILLA__51E,
        [0xE8, 0x7D, 0x01, 0xFE, 0xFF],
    ),
    (
        "ExtendedServerName #10",
        eix_sites::C_MP_CONNECT_SCREEN_DRAW_LIST_VANILLA__4FF,
        [0xE8, 0x9C, 0x01, 0xFE, 0xFF],
    ),
    (
        "ExtendedServerName #11",
        eix_sites::C_MP_CONNECT_SCREEN_DRAW_LIST_VANILLA__889,
        [0xE8, 0x12, 0xFE, 0xFD, 0xFF],
    ),
];

type Draw = extern "thiscall" fn(*mut Surface, *const u8, *mut Rect, i32, u32, u32, i32) -> i32;

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let mut writes = vec![
        // mov edi, 0x28A: the name field's right edge, 650 to 680
        bytes(
            "ExtendedServerName #0",
            eix_sites::C_MP_CONNECT_SCREEN_CREATE__891,
            &0x28Au32.to_le_bytes(),
            &0x2A8u32.to_le_bytes(),
        )?,
        // push 3: the field's font class
        bytes(
            "ExtendedServerName #1",
            eix_sites::C_MP_CONNECT_SCREEN_CREATE__94E,
            &[0x03],
            &[0x0F],
        )?,
        // push 0xA: its length
        bytes(
            "ExtendedServerName #2",
            eix_sites::C_MP_CONNECT_SCREEN_CREATE__949,
            &[0x0A],
            &[0x32],
        )?,
        // push 1: its font
        bytes(
            "ExtendedServerName #3",
            eix_sites::C_MP_CONNECT_SCREEN_CREATE__94B,
            &[0x01],
            &[0x00],
        )?,
        // the right end of the line under the titles, 270 to 283
        bytes(
            "ExtendedServerName #4",
            eix_sites::C_MP_CONNECT_SCREEN_DRAW_LIST_VANILLA__3A3,
            &0x10Eu32.to_le_bytes(),
            &0x11Bu32.to_le_bytes(),
        )?,
        // the selection's right edge, 370.0 to 381.5
        bytes(
            "ExtendedServerName #5",
            eix_sites::C_MP_CONNECT_SCREEN_CREATE__30E,
            &370.0f32.to_le_bytes(),
            &381.5f32.to_le_bytes(),
        )?,
        // the selection's place in the list, 239.0 to 244.75
        bytes(
            "ExtendedServerName #6",
            eix_sites::C_MP_CONNECT_SCREEN_DRAW_LIST_VANILLA__46F,
            &239.0f32.to_le_bytes(),
            &244.75f32.to_le_bytes(),
        )?,
        // the ping column's left edge, 220 to 233
        bytes(
            "ExtendedServerName #7",
            eix_sites::C_MP_CONNECT_SCREEN_DRAW_LIST_VANILLA__5C0,
            &0xDCu32.to_le_bytes(),
            &0xE9u32.to_le_bytes(),
        )?,
        // the rows' right edge, 265 to 278
        bytes(
            "ExtendedServerName #8",
            eix_sites::C_MP_CONNECT_SCREEN_DRAW_LIST_VANILLA__27C,
            &0x109u32.to_le_bytes(),
            &0x116u32.to_le_bytes(),
        )?,
    ];
    let handlers: [Draw; 3] = [draw_name, draw_ellipsis, draw_wrapped];
    for ((label, site, call), handler) in CALLS.into_iter().zip(handlers) {
        let (write, _) = hook::redirect_call(site, call, handler as *const () as usize)?;
        writes.push(Labelled {
            label: label.to_owned(),
            write,
        });
    }
    Ok(writes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_texts_are_cut_to_the_originals_buffer() {
        let long = [b'a'; 200].iter().copied().chain([0]).collect::<Vec<u8>>();
        // SAFETY: a NUL-terminated local string.
        let b = unsafe { fitting(long.as_ptr()) };
        assert_eq!(b.iter().position(|&c| c == 0), Some(TEXT_SIZE - 1));
        // SAFETY: as above.
        let short = unsafe { fitting(c"abc".as_ptr().cast()) };
        assert_eq!(&short[..4], b"abc\0");
    }
}
