//! Reading values from INI files for scripts: which paths a script may name, how a value reads as a number, which
//! variable names name an INI value, and an INI file's text.

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

/// The prefix of a variable that names an INI value, matched without regard to case as the game matches variable
/// names.
pub const VARIABLE_PREFIX: &[u8] = b"ini.";

/// An INI value a variable names: `ini.<file>.<section>.<key>` reads `[section] key` of `<file>.ini`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IniVariable<'a> {
    /// The file's name without `.ini`: no folder, so only the mod and game folders can hold it.
    pub file: &'a [u8],
    /// The section.
    pub section: &'a [u8],
    /// The key: the rest of the name, which may hold dots.
    pub key: &'a [u8],
}

/// The INI value `name` names; `None` for a name without the prefix, or with an empty part, or whose file part is not
/// a plain file name (a `\`, `/` or `:`, a NUL, or only spaces).
pub fn variable(name: &[u8]) -> Option<IniVariable<'_>> {
    let prefix = name.get(..VARIABLE_PREFIX.len())?;
    if !prefix.eq_ignore_ascii_case(VARIABLE_PREFIX) {
        return None;
    }
    let mut parts = name[VARIABLE_PREFIX.len()..].splitn(3, |&c| c == b'.');
    let (file, section, key) = (parts.next()?, parts.next()?, parts.next()?);
    let plain = |c: &u8| !matches!(c, b'\\' | b'/' | b':' | 0);
    if file.trim_ascii().is_empty()
        || !file.iter().all(plain)
        || section.is_empty()
        || key.is_empty()
    {
        return None;
    }
    Some(IniVariable { file, section, key })
}

/// An INI file's text, read as the system's INI reads do: `[section]` lines and `key=value` lines; names compared
/// without regard to ASCII case and trimmed of spaces; the first of two equal keys wins; a line that starts with `;`,
/// and a key before any section, are skipped; a value in double quotes loses them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Document {
    entries: Vec<(Vec<u8>, Vec<u8>, Vec<u8>)>,
}

/// Section, key and raw value of each INI entry, with surrounding spaces removed. A section without `]` uses the
/// rest of its line as its name, just as the value reader does.
pub(crate) fn entries(text: &[u8]) -> impl Iterator<Item = (&[u8], &[u8], &[u8])> {
    let mut section = None;
    text.split(|&c| c == b'\n').filter_map(move |line| {
        let line = line.trim_ascii();
        if line.is_empty() || line.starts_with(b";") {
            return None;
        }
        if let Some(rest) = line.strip_prefix(b"[") {
            section = Some(
                rest.split(|&c| c == b']')
                    .next()
                    .unwrap_or(rest)
                    .trim_ascii(),
            );
            return None;
        }
        let current = section?;
        let at = line.iter().position(|&c| c == b'=')?;
        let key = line[..at].trim_ascii();
        if key.is_empty() {
            return None;
        }
        Some((current, key, line[at + 1..].trim_ascii()))
    })
}

impl Document {
    /// The values of `text`.
    pub fn parse(text: &[u8]) -> Self {
        let mut entries = Vec::new();
        for (section, key, value) in self::entries(text) {
            let value = match value {
                [b'"', inner @ .., b'"'] => inner,
                other => other,
            };
            entries.push((
                section.to_ascii_lowercase(),
                key.to_ascii_lowercase(),
                value.to_vec(),
            ));
        }
        Self { entries }
    }

    /// The value of `key` in `[section]`, `None` when there is none.
    pub fn get(&self, section: &[u8], key: &[u8]) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|(s, k, _)| s.eq_ignore_ascii_case(section) && k.eq_ignore_ascii_case(key))
            .map(|(_, _, v)| v.as_slice())
    }
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

    #[test]
    fn a_variable_names_file_section_and_key() {
        let expected = |file: &'static [u8], section: &'static [u8], key: &'static [u8]| {
            Some(IniVariable { file, section, key })
        };
        assert_eq!(
            variable(b"ini.eitweaks.Companions.Travel"),
            expected(b"eitweaks", b"Companions", b"Travel")
        );
        assert_eq!(variable(b"INI.a.b.c"), expected(b"a", b"b", b"c"));
        assert_eq!(variable(b"ini.a.b.c.d"), expected(b"a", b"b", b"c.d"));
        assert_eq!(variable(b"ini.my mod.b.c"), expected(b"my mod", b"b", b"c"));
    }

    #[test]
    fn other_names_are_no_ini_variable() {
        for name in [
            &b""[..],
            b"ini",
            b"ini.",
            b"ini.a",
            b"ini.a.b",
            b"ini.a.b.",
            b"ini..b.c",
            b"ini.a..c",
            b"ini. .b.c",
            b"ini.a\\x.b.c",
            b"ini.a/x.b.c",
            b"ini.c:.b.c",
            b"ini.a\0.b.c",
            b"gvar.a.b.c",
            b"xini.a.b.c",
            b"Diff",
        ] {
            assert_eq!(variable(name), None, "{}", String::from_utf8_lossy(name));
        }
    }

    #[test]
    fn a_document_reads_as_the_system_does() {
        let text =
            b"top=0\r\n; comment=1\r\n[Companions]\r\n Travel = 1 \r\nTravel=2\r\nname=\"a b\"\r\n\
                     empty=\r\n=5\r\nnoequals\r\n[ Other ]\r\ntravel=3\n";
        let doc = Document::parse(text);
        assert_eq!(doc.get(b"companions", b"TRAVEL"), Some(&b"1"[..]));
        assert_eq!(doc.get(b"Companions", b"name"), Some(&b"a b"[..]));
        assert_eq!(doc.get(b"Companions", b"empty"), Some(&b""[..]));
        assert_eq!(doc.get(b"Other", b"Travel"), Some(&b"3"[..]));
        assert_eq!(doc.get(b"Companions", b"comment"), None);
        assert_eq!(doc.get(b"", b"top"), None);
        assert_eq!(doc.get(b"Companions", b"missing"), None);
        assert_eq!(doc.get(b"Missing", b"Travel"), None);
        assert_eq!(Document::parse(b"").get(b"a", b"b"), None);
    }
}
