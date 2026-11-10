//! Reading values from the INI files of the mod folder for scripts: which paths a script may name, and how a value
//! reads as a number.

use crate::spelladdon::number::digits;

/// The longest text the number reader looks at: the window of the digit parser it shares with SpellAddon.
const MAX_NUMBER_BYTES: usize = 20;

/// A script's `file` as a path below the mod folder, with `\` separators; `None` when it is not one: empty, rooted
/// (`\x`, `/x`), with a drive or stream colon, with a NUL, or with a component that names a parent folder. Windows
/// reads a component of dots and spaces only (`..`, `...`, `.. `) as the parent folder, so each of those is refused;
/// `.` alone stays.
pub fn mod_relative_path(file: &[u8]) -> Option<Vec<u8>> {
    if file.is_empty() || file.iter().any(|&c| c == 0 || c == b':') {
        return None;
    }
    let normalized: Vec<u8> = file
        .iter()
        .map(|&c| if c == b'/' { b'\\' } else { c })
        .collect();
    if normalized[0] == b'\\' {
        return None;
    }
    let climbs = |part: &[u8]| part != b"." && part.iter().all(|&c| c == b'.' || c == b' ');
    if normalized.split(|&c| c == b'\\').any(climbs) {
        return None;
    }
    Some(normalized)
}

/// An INI value as a number: spaces around it, an optional sign, then digits with at most one `.` or `,` as the
/// decimal point, read as SpellAddon reads its numbers. `None` for anything else, an empty value included.
pub fn parse_number(value: &[u8]) -> Option<f64> {
    let text = value.trim_ascii();
    let unsigned = text
        .strip_prefix(b"-")
        .or_else(|| text.strip_prefix(b"+"))
        .unwrap_or(text);
    let points = unsigned.iter().filter(|&&c| c == b'.' || c == b',').count();
    let plain = unsigned
        .iter()
        .all(|&c| c.is_ascii_digit() || c == b'.' || c == b',');
    let has_digit = unsigned.iter().any(u8::is_ascii_digit);
    if text.len() > MAX_NUMBER_BYTES || points > 1 || !plain || !has_digit {
        return None;
    }
    // The digit parser negates on a leading `-` and skips a `+`.
    Some(digits(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_below_the_mod_folder_is_kept() {
        assert_eq!(mod_relative_path(b"a.ini").as_deref(), Some(&b"a.ini"[..]));
        assert_eq!(
            mod_relative_path(b"cfg/sub\\a.ini").as_deref(),
            Some(&b"cfg\\sub\\a.ini"[..])
        );
        assert_eq!(
            mod_relative_path(b"./a.ini").as_deref(),
            Some(&b".\\a.ini"[..])
        );
        assert_eq!(
            mod_relative_path(b"a..b/c.ini").as_deref(),
            Some(&b"a..b\\c.ini"[..])
        );
        assert_eq!(
            mod_relative_path(b"..a.ini").as_deref(),
            Some(&b"..a.ini"[..])
        );
    }

    #[test]
    fn a_path_outside_the_mod_folder_is_refused() {
        for path in [
            &b""[..],
            b"..",
            b"../a.ini",
            b"..\\a.ini",
            b"a/../b.ini",
            b"a\\..\\b.ini",
            b"a/b/..",
            b"a/..\\b",
            b"...\\a.ini",
            b".. /a.ini",
            b"C:\\x",
            b"c:x",
            b"\\x",
            b"/x",
            b"\\\\host\\share\\x",
            b"a.ini:stream",
            b"a\0/../b",
        ] {
            assert_eq!(
                mod_relative_path(path),
                None,
                "{}",
                String::from_utf8_lossy(path)
            );
        }
    }

    #[test]
    fn a_value_reads_as_a_number() {
        assert_eq!(parse_number(b"1.5"), Some(1.5));
        assert_eq!(parse_number(b"0,5"), Some(0.5));
        assert_eq!(parse_number(b"-2"), Some(-2.0));
        assert_eq!(parse_number(b"-2,25"), Some(-2.25));
        assert_eq!(parse_number(b"+3"), Some(3.0));
        assert_eq!(parse_number(b"150"), Some(150.0));
        assert_eq!(parse_number(b"  7.0 "), Some(7.0));
        assert_eq!(parse_number(b".5"), Some(0.5));
        assert_eq!(parse_number(b"0"), Some(0.0));
    }

    #[test]
    fn a_value_that_is_no_number_is_refused() {
        for value in [
            &b""[..],
            b"   ",
            b"abc",
            b"1.5x",
            b"x1",
            b"1,2,3",
            b"1.2.3",
            b".",
            b",",
            b"-",
            b"--1",
            b"1 2",
            b"1e3",
            b"123456789012345678901",
        ] {
            assert_eq!(
                parse_number(value),
                None,
                "{}",
                String::from_utf8_lossy(value)
            );
        }
    }
}
