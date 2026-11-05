//! ImproveConsole's edits of the console's input line: select all, cut, paste. The line is the game's ANSI bytes;
//! positions are byte offsets, as the console window keeps its caret and anchor.

/// MFC's `CString::Left(n)`: the first `n` bytes, all of them past the end, none for a negative `n`.
pub fn left(text: &[u8], n: i32) -> &[u8] {
    &text[..usize::try_from(n).unwrap_or(0).min(text.len())]
}

/// MFC's `CString::Mid(n)`: the bytes from `n` on, none past the end, all for a negative `n`.
pub fn mid(text: &[u8], n: i32) -> &[u8] {
    &text[usize::try_from(n).unwrap_or(0).min(text.len())..]
}

/// The selection's ends, lower first.
pub fn selection(caret: i32, anchor: i32) -> (i32, i32) {
    (caret.min(anchor), caret.max(anchor))
}

/// The input without the selection: the bytes before `low`, then those from `high` on.
pub fn cut(input: &[u8], low: i32, high: i32) -> Vec<u8> {
    [left(input, low), mid(input, high)].concat()
}

/// The clipboard text's first line: up to its first CR or LF.
pub fn first_line(clip: &[u8]) -> &[u8] {
    let end = clip
        .iter()
        .position(|&c| c == b'\r' || c == b'\n')
        .unwrap_or(clip.len());
    &clip[..end]
}

/// Pastes `clip` over the selection `low..high`: the new input and the caret (= anchor) position, which is the
/// paste's length past `high`, less one when there was a selection (the original's arithmetic).
pub fn paste(input: &[u8], low: i32, high: i32, clip: &[u8], selected: bool) -> (Vec<u8>, i32) {
    let text = [left(input, low), clip, mid(input, high)].concat();
    let len = i32::try_from(clip.len()).unwrap_or(i32::MAX);
    let position = len.wrapping_sub(i32::from(selected)).wrapping_add(high);
    (text, position)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_and_mid_clamp_as_mfc_does() {
        assert_eq!(left(b"abc", 2), b"ab");
        assert_eq!(left(b"abc", 9), b"abc");
        assert_eq!(left(b"abc", -1), b"");
        assert_eq!(mid(b"abc", 1), b"bc");
        assert_eq!(mid(b"abc", 9), b"");
        assert_eq!(mid(b"abc", -3), b"abc");
    }

    #[test]
    fn cutting_and_pasting_follow_the_original() {
        assert_eq!(selection(4, 1), (1, 4));
        assert_eq!(cut(b"abcdef", 1, 4), b"aef");
        assert_eq!(first_line(b"one\r\ntwo"), b"one");
        assert_eq!(first_line(b"one"), b"one");
        // A selection: the caret lands one byte short of the paste's end.
        assert_eq!(paste(b"abcdef", 1, 4, b"XY", true), (b"aXYef".to_vec(), 5));
        // No selection: right after the pasted text.
        assert_eq!(paste(b"abc", 1, 1, b"XY", false), (b"aXYbc".to_vec(), 3));
    }
}
