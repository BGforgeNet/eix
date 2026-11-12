//! The addon's script-engine rules that need no game memory: keyword matching, the `?` and `+` signature letters,
//! and the arithmetic and text rules of its script functions.

use crate::num::trunc_to_i32;

/// Names compared as the C runtime's `_stricmp` in the "C" locale compares them: ASCII letters fold.
pub fn same_name(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The parser's stricter keyword match: a keyword starting with a letter does not match text that goes on with a
/// letter, a digit, `_` or `#` (so `Orc1` is an identifier, not `Orc` and `1`). Bytes above 0x7F are neither
/// letters nor digits there.
pub fn rejects_keyword_match(keyword_first: u8, next: u8) -> bool {
    keyword_first.is_ascii_alphabetic()
        && (next.is_ascii_alphanumeric() || next == b'_' || next == b'#')
}

/// Whether a signature (result letter included) makes a keyword a function: its first letter is one of `FGOS`.
pub fn is_function_signature(signature: &[u8]) -> bool {
    signature.first().is_some_and(|c| b"FGOS".contains(c))
}

/// An argument type letter of a signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgType {
    /// `f`.
    Float,
    /// `s`.
    String,
    /// `o`.
    Object,
    /// `g`.
    Group,
}

impl ArgType {
    fn of(letter: u8) -> Option<Self> {
        match letter {
            b'f' => Some(Self::Float),
            b's' => Some(Self::String),
            b'o' => Some(Self::Object),
            b'g' => Some(Self::Group),
            _ => None,
        }
    }
}

/// How many slots of each type to drop.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    /// Floats.
    pub floats: i32,
    /// Strings.
    pub strings: i32,
    /// Objects.
    pub objects: i32,
    /// Groups.
    pub groups: i32,
}

/// What the parser does with a `?` or `+` at the current signature position, after a separator token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureStep {
    /// Neither letter: the game's own handling.
    None,
    /// Step past the letter.
    Skip,
    /// At `)` after `?`: the optional parameters' slots are dropped and the position moves to the end.
    DropOptional(Counts),
    /// After `,` at `+`: the position steps back to the repeated letter and a slot of its type is added (none for
    /// another letter).
    Repeat(Option<ArgType>),
}

/// The step for the signature from the current position on (`rest`, up to its NUL), after a separator token that is
/// `)` when `at_close`.
pub fn signature_step(rest: &[u8], previous: Option<u8>, at_close: bool) -> SignatureStep {
    match (rest.first(), at_close) {
        (Some(b'+'), true) | (Some(b'?'), false) => SignatureStep::Skip,
        (Some(b'?'), true) => {
            let mut counts = Counts::default();
            for &c in rest {
                match ArgType::of(c) {
                    Some(ArgType::Float) => counts.floats += 1,
                    Some(ArgType::String) => counts.strings += 1,
                    Some(ArgType::Object) => counts.objects += 1,
                    Some(ArgType::Group) => counts.groups += 1,
                    None => {}
                }
            }
            SignatureStep::DropOptional(counts)
        }
        (Some(b'+'), false) => SignatureStep::Repeat(previous.and_then(ArgType::of)),
        _ => SignatureStep::None,
    }
}

/// Whether a record's signature passes the addon's check before it is registered: argument letters only from
/// `fgos+?`; a `?` followed by an argument letter, then the end or another `?`; a `+` only last and not first.
pub fn signature_is_valid(signature: &[u8]) -> bool {
    let args = if is_function_signature(signature) {
        &signature[1..]
    } else {
        signature
    };
    for (i, &c) in args.iter().enumerate() {
        if !b"fgos+?".contains(&c) {
            return false;
        }
        let next = args.get(i + 1).copied().unwrap_or(0);
        let after = args.get(i + 2).copied().unwrap_or(0);
        if c == b'?' && !b"fgos".contains(&next) && after != 0 && after != b'?' {
            return false;
        }
        if c == b'+' && (next != 0 || i == 0) {
            return false;
        }
    }
    true
}

/// `FloatToStr`'s decimals: the second argument truncated, 3 without one, clamped to 0..8.
pub fn float_to_str_decimals(second: Option<f32>) -> i32 {
    second.map_or(3, |d| trunc_to_i32(f64::from(d))).clamp(0, 8)
}

/// `FloatToStr`'s cut of the formatted number: the fraction's trailing zeros go, then a trailing `.`. A number with
/// no point keeps its zeros (the original cuts those too, so `100` with no decimals became `1`).
pub fn trim_float_text(text: &mut Vec<u8>) {
    if !text.contains(&b'.') {
        return;
    }
    while text.last() == Some(&b'0') {
        text.pop();
    }
    if text.last() == Some(&b'.') {
        text.pop();
    }
}

/// `StrMid(text, start, count)` over a text of `len` bytes: the byte range of the result, or `None` when a negative
/// count leaves the result as it was. `start` counts from 1, from the end when negative (-1 is the last byte); a
/// count of 0 means the rest.
pub fn str_mid(len: i32, start: i32, count: i32) -> Option<(usize, usize)> {
    let mut start = if start < 0 {
        start.wrapping_add(len).wrapping_add(1)
    } else {
        start
    };
    let mut count = match count {
        0 => len,
        c if c < 0 => return None,
        c => c,
    };
    if len.min(start) < 1 {
        start = 1;
    } else if len < start {
        start = len;
    }
    let available = len - start + 1;
    if count >= available {
        count = available;
    }
    let first = usize::try_from(start - 1).unwrap_or(0);
    let last = usize::try_from(start - 1 + count).unwrap_or(0);
    Some((first, last.max(first)))
}

/// `GetUnitExp`'s kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpKind {
    /// `max` (the default): the experience.
    Max,
    /// `spent`.
    Spent,
    /// `penalty`: the experience not yet added.
    Penalty,
    /// `real`: penalty plus experience.
    Real,
}

/// The kind `GetUnitExp` names, ignoring case.
pub fn exp_kind(name: &[u8]) -> Option<ExpKind> {
    [
        (&b"max"[..], ExpKind::Max),
        (b"spent", ExpKind::Spent),
        (b"penalty", ExpKind::Penalty),
        (b"real", ExpKind::Real),
    ]
    .into_iter()
    .find(|(n, _)| same_name(n, name))
    .map(|(_, k)| k)
}

/// `GetSkill`'s names and the byte each reads: an attribute byte (`science`) or a skill byte, by index.
pub fn skill(name: &[u8]) -> Option<Skill> {
    [
        (&b"science"[..], Skill::Attribute(0)),
        (b"melee", Skill::Skill(0)),
        (b"archery", Skill::Skill(1)),
        (b"elemental", Skill::Skill(3)),
        (b"sense", Skill::Skill(4)),
        (b"astral", Skill::Skill(5)),
    ]
    .into_iter()
    .find(|(n, _)| same_name(n, name))
    .map(|(_, s)| s)
}

/// A byte of the unit's statistics `GetSkill` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skill {
    /// An attribute byte.
    Attribute(usize),
    /// A skill byte.
    Skill(usize),
}

/// `GetPerk`'s names, in the order of the unit's perk bytes.
pub const PERKS: [&str; 26] = [
    "sword",
    "axe",
    "dagger",
    "spear",
    "hammer",
    "bow",
    "crossbow",
    "fire",
    "lightning",
    "acid",
    "illusion",
    "divination",
    "enchantments",
    "healing",
    "domination",
    "infra",
    "health",
    "mana",
    "vitality",
    "spirit",
    "quickness",
    "lift",
    "backstab",
    "strength",
    "dexterity",
    "intelligence",
];

/// The perk byte `GetPerk` names, ignoring case.
pub fn perk(name: &[u8]) -> Option<usize> {
    PERKS.iter().position(|p| same_name(p.as_bytes(), name))
}

/// `SetWeather`'s kind and strength: `rain` 1, `clear` 0, `snow` 2 at 40; anything else clear. The names compare
/// with case.
pub fn weather(name: &[u8]) -> (i32, f32) {
    match name {
        b"rain" => (1, 20.0),
        b"snow" => (2, 40.0),
        _ => (0, 20.0),
    }
}

/// The distance in the plane as the addon computes it: each difference and the sum of squares rounded to float,
/// then the double square root rounded to float.
pub fn distance(x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let sum = dx * dx + dy * dy;
    f64::from(sum).sqrt() as f32
}

/// `Round`: half away from zero, exactly (the original rounds `x + 0.5` to float first, so 8388609 became 8388610 and
/// 0.49999997 became 1).
pub fn round(x: f32) -> f32 {
    x.round()
}

/// `Max`: an argument replaces the maximum unless it is (ordered) less, so equal and NaN arguments do.
pub fn max(values: &[f32]) -> f32 {
    let Some((&first, rest)) = values.split_first() else {
        return 0.0;
    };
    rest.iter().fold(first, |m, &v| if v < m { m } else { v })
}

/// `Min`: an argument replaces the minimum unless it is (ordered) greater.
pub fn min(values: &[f32]) -> f32 {
    let Some((&first, rest)) = values.split_first() else {
        return 0.0;
    };
    rest.iter().fold(first, |m, &v| if v > m { m } else { v })
}

/// `And`: 1 unless an argument truncates to 0 (NaN truncates to the minimum, so it counts as true).
pub fn and(values: &[f32]) -> f32 {
    if values.iter().any(|&v| trunc_to_i32(f64::from(v)) == 0) {
        0.0
    } else {
        1.0
    }
}

/// `Or`: 1 when an argument does not truncate to 0.
pub fn or(values: &[f32]) -> f32 {
    if values.iter().any(|&v| trunc_to_i32(f64::from(v)) != 0) {
        1.0
    } else {
        0.0
    }
}

/// `Add`: the sum, each step `value + sum` rounded to float.
pub fn sum(values: &[f32]) -> f32 {
    values.iter().fold(0.0, |s, &v| v + s)
}

/// `Mul`: the product, each step `value * product` rounded to float.
pub fn product(values: &[f32]) -> f32 {
    values.iter().fold(1.0, |p, &v| v * p)
}

/// `Abs`: negated only when below 0, so -0 and NaN stay.
pub fn abs(x: f32) -> f32 {
    if x < 0.0 { -x } else { x }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_matches_stop_at_identifier_characters() {
        assert!(rejects_keyword_match(b'O', b'1'));
        assert!(rejects_keyword_match(b'O', b'_'));
        assert!(rejects_keyword_match(b'O', b'#'));
        assert!(!rejects_keyword_match(b'O', b'('));
        assert!(!rejects_keyword_match(b'O', 0xE0));
        assert!(!rejects_keyword_match(b'(', b'a'));
    }

    #[test]
    fn functions_start_with_a_result_letter() {
        assert!(is_function_signature(b"Fo?s"));
        assert!(is_function_signature(b"S"));
        assert!(!is_function_signature(b"fsf"));
        assert!(!is_function_signature(b""));
    }

    #[test]
    fn signature_letters_step_as_the_parser_needs() {
        assert_eq!(signature_step(b"f", Some(b's'), false), SignatureStep::None);
        assert_eq!(signature_step(b"+", Some(b's'), true), SignatureStep::Skip);
        assert_eq!(
            signature_step(b"?f", Some(b'f'), false),
            SignatureStep::Skip
        );
        assert_eq!(
            signature_step(b"+", Some(b's'), false),
            SignatureStep::Repeat(Some(ArgType::String))
        );
        assert_eq!(
            signature_step(b"?sfof", Some(b'o'), true),
            SignatureStep::DropOptional(Counts {
                floats: 2,
                strings: 1,
                objects: 1,
                groups: 0
            })
        );
        assert_eq!(signature_step(b"", None, true), SignatureStep::None);
    }

    #[test]
    fn every_record_signature_is_valid() {
        for sig in [
            &b"Fo"[..],
            b"Fo?s",
            b"Sss+",
            b"",
            b"s+",
            b"Gf+",
            b"s?f",
            b"Ff?f",
        ] {
            assert!(signature_is_valid(sig), "{}", String::from_utf8_lossy(sig));
        }
        assert!(!signature_is_valid(b"Fx"));
        assert!(!signature_is_valid(b"+f"));
        assert!(!signature_is_valid(b"f+f"));
    }

    #[test]
    fn float_text_loses_the_fractions_trailing_zeros() {
        let cut = |s: &str| {
            let mut v = s.as_bytes().to_vec();
            trim_float_text(&mut v);
            String::from_utf8(v).unwrap_or_default()
        };
        assert_eq!(cut("1.500"), "1.5");
        assert_eq!(cut("2.000"), "2");
        assert_eq!(cut("100"), "100");
        assert_eq!(cut("20"), "20");
        assert_eq!(cut("100.000"), "100");
        assert_eq!(cut("0"), "0");
        assert_eq!(cut("0.000"), "0");
        assert_eq!(cut("1.#INF00"), "1.#INF");
        assert_eq!(cut("-0.250"), "-0.25");
        assert_eq!(float_to_str_decimals(None), 3);
        assert_eq!(float_to_str_decimals(Some(12.0)), 8);
        assert_eq!(float_to_str_decimals(Some(-1.0)), 0);
        assert_eq!(float_to_str_decimals(Some(f32::NAN)), 0);
    }

    #[test]
    fn str_mid_counts_bytes_from_one() {
        assert_eq!(str_mid(6, 2, 3), Some((1, 4)));
        assert_eq!(str_mid(6, -1, 0), Some((5, 6)));
        assert_eq!(str_mid(6, 0, 0), Some((0, 6)));
        assert_eq!(str_mid(6, 9, 2), Some((5, 6)));
        assert_eq!(str_mid(6, 2, -1), None);
        assert_eq!(str_mid(0, 1, 0), Some((0, 0)));
    }

    #[test]
    fn names_ignore_case() {
        assert_eq!(exp_kind(b"REAL"), Some(ExpKind::Real));
        assert_eq!(exp_kind(b"x"), None);
        assert_eq!(skill(b"Astral"), Some(Skill::Skill(5)));
        assert_eq!(skill(b"science"), Some(Skill::Attribute(0)));
        assert_eq!(perk(b"Intelligence"), Some(25));
        assert_eq!(weather(b"snow"), (2, 40.0));
        assert_eq!(weather(b"Rain"), (0, 20.0));
    }

    #[test]
    fn arithmetic_follows_the_addon() {
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -3.0);
        assert_eq!(round(8_388_609.0), 8_388_609.0);
        assert_eq!(round(-8_388_609.0), -8_388_609.0);
        assert_eq!(round(0.499_999_97), 0.0);
        assert!(round(f32::NAN).is_nan());
        assert!(max(&[1.0, f32::NAN]).is_nan());
        assert_eq!(max(&[f32::NAN, 1.0]), 1.0);
        assert!(min(&[1.0, f32::NAN]).is_nan());
        assert_eq!(and(&[1.0, 0.5]), 0.0);
        assert_eq!(and(&[f32::NAN]), 1.0);
        assert_eq!(or(&[0.0, 0.9]), 0.0);
        assert_eq!(or(&[0.0, -1.0]), 1.0);
        assert_eq!(sum(&[1.0, 2.0, 3.0]), 6.0);
        assert_eq!(product(&[]), 1.0);
        assert_eq!(abs(-0.0).to_bits(), (-0.0f32).to_bits());
        assert_eq!(distance(0.0, 0.0, 3.0, 4.0), 5.0);
    }
}
