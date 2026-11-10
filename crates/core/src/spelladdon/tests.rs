use std::collections::BTreeMap;

use super::*;

/// The key index: every key line of the shipped spelladdon.ini in file order, then the keys only the code reads;
/// "not read" marks a pair the original never asks for.
const KEY_INDEX: &str = include_str!("../../tests/data/spelladdon-keys.tsv");

/// A profile over fixed values, with the system's buffer behaviour: the string and its NUL, cut to `size - 1`.
#[derive(Default)]
struct Fake(BTreeMap<(String, String), String>);

impl Fake {
    fn with(pairs: &[(&str, &str, &str)]) -> Self {
        Self(
            pairs
                .iter()
                .map(|(s, k, v)| (((*s).to_owned(), (*k).to_owned()), (*v).to_owned()))
                .collect(),
        )
    }

    fn get(&self, section: &str, key: &str) -> Option<&String> {
        self.0.get(&(section.to_owned(), key.to_owned()))
    }
}

impl Profile for Fake {
    fn int(&self, section: &str, key: &str, default: i32) -> i32 {
        self.get(section, key)
            .map_or(default, |v| v.parse::<i64>().map_or(0, |n| n as i32))
    }

    fn text(&self, section: &str, key: &str, default: &str, size: usize) -> Vec<u8> {
        let value = self.get(section, key).map_or(default, String::as_str);
        let mut buf = vec![0xCC; size];
        let n = value.len().min(size - 1);
        buf[..n].copy_from_slice(&value.as_bytes()[..n]);
        buf[n] = 0;
        buf
    }
}

/// One row of the key index: section, key (a trailing space kept), kind, default.
fn rows() -> Vec<(String, String, String, String)> {
    KEY_INDEX
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| match l.split('\t').collect::<Vec<_>>()[..] {
            [section, name, kind, default] => (
                section.to_owned(),
                name.to_owned(),
                kind.to_owned(),
                default.to_owned(),
            ),
            _ => panic!("key index row {l:?}"),
        })
        .collect()
}

fn key(section: &str, name: &str) -> Option<&'static Key> {
    KEYS.iter().find(|k| k.section == section && k.name == name)
}

#[test]
fn every_key_matches_the_key_index() {
    let rows = rows();
    // 526 lines of the shipped INI and the keys only the code has.
    assert!(
        rows.len() > 550,
        "the key index parsed to {} rows",
        rows.len()
    );
    for (section, name, kind, default) in &rows {
        let found = key(section, name);
        let what = format!("[{section}] {name:?}");
        if kind == "not read" {
            assert!(
                found.is_none(),
                "{what} is in the table but the key index says not read"
            );
            continue;
        }
        let found = found.unwrap_or_else(|| panic!("{what} is missing"));
        match found.kind {
            Kind::Flag { index } => {
                assert_eq!(kind, &format!("flag {index}"), "{what}");
                assert_eq!(default, "0", "{what}");
            }
            Kind::Int { default: d, .. } => {
                assert_eq!(kind, "int", "{what}");
                // The one default that differs on purpose: the game's 55, not the original's 5.
                if (section.as_str(), name.as_str()) == ("speed", "normalkey") {
                    assert_eq!((default.as_str(), d), ("5", 55), "{what}");
                } else {
                    assert_eq!(default, &d.to_string(), "{what}");
                }
            }
            Kind::Text { default: d, .. } => {
                assert_eq!(kind, "str", "{what}");
                assert_eq!(default, d, "{what}");
            }
        }
    }
    for k in &KEYS {
        assert!(
            rows.iter()
                .any(|(s, n, _, _)| s == k.section && n == k.name),
            "[{}] {:?} is not in the key index",
            k.section,
            k.name
        );
    }
}

#[test]
fn the_table_has_every_read() {
    let count = |f: fn(&Kind) -> bool| KEYS.iter().filter(|k| f(&k.kind)).count();
    assert_eq!(count(|k| matches!(k, Kind::Flag { .. })), 209);
    assert_eq!(count(|k| matches!(k, Kind::Int { .. })), 161);
    assert_eq!(count(|k| matches!(k, Kind::Text { .. })), 165);
    let mut indices: Vec<u8> = KEYS
        .iter()
        .filter_map(|k| match k.kind {
            Kind::Flag { index } => Some(index),
            Kind::Int { .. } | Kind::Text { .. } => None,
        })
        .collect();
    indices.sort_unstable();
    indices.dedup();
    assert_eq!(indices.len(), 209, "flag indices are unique");
    for (i, a) in KEYS.iter().enumerate() {
        assert!(
            KEYS[i + 1..]
                .iter()
                .all(|b| (a.section, a.name) != (b.section, b.name)),
            "[{}] {} twice",
            a.section,
            a.name
        );
    }
}

#[test]
fn absent_keys_take_the_built_in_defaults() {
    let v = Values::read(&Fake::default());
    assert_eq!(v.flag("gameplay", "newspells"), Ok(0));
    assert_eq!(v.int("gameplay", "maxcountofmerc"), Ok(27));
    assert_eq!(v.byte("aicastertable", "0"), Ok(8));
    assert_eq!(v.float("gameplay", "headmod"), Ok(3.0));
    assert_eq!(v.float("gameplay", "visionarc"), Ok(-2.0));
    assert_eq!(v.text("fonts", "newfont5"), Ok(&b"Times New Roman"[..]));
    assert_eq!(v.float("gameplay", "light07"), Ok(1.0));
    assert_eq!(v.float("gameplay", "framex"), Ok(75.0));
    assert_eq!(v.text_int("gameplay", "framey"), Ok(175));
    assert_eq!(v.float("gameplay", "balance"), Ok(75.0));
    assert_eq!(v.double("rebalanceengine", "pow"), Ok(1.1));
    // The digit parser takes the comma of these defaults as a point (atof would give 0).
    assert_eq!(v.double("rebalanceengine", "runconst"), Ok(0.006666666666));
    assert_eq!(v.double("rebalanceengine", "runconstex"), Ok(0.5));
    assert_eq!(v.flags_on().count(), 0);
}

#[test]
fn flags_keep_the_low_byte_compared_signed() {
    let v = Values::read(&Fake::with(&[
        ("gameplay", "newspells", "256"),
        ("gameplay", "aggrmod", "10"),
        ("gameplay", "saveall", "255"),
        ("gameplay", "unlockmoney", "257"),
        ("gameplay", "manygameexe", "-1"),
    ]));
    assert_eq!(v.flag("gameplay", "newspells"), Ok(0));
    assert_eq!(v.flag("gameplay", "aggrmod"), Ok(10));
    assert_eq!(v.flag("gameplay", "saveall"), Ok(-1));
    assert_eq!(v.flag("gameplay", "unlockmoney"), Ok(1));
    assert_eq!(v.flag("gameplay", "manygameexe"), Ok(-1));
    let on: Vec<(&str, i8)> = v.flags_on().map(|(k, n)| (k.name, n)).collect();
    assert_eq!(
        on,
        [
            ("unlockmoney", 1),
            ("manygameexe", -1),
            ("aggrmod", 10),
            ("saveall", -1)
        ]
    );
}

#[test]
fn out_of_signed_byte_flags_warn_with_written_and_used_values() {
    let mut messages = Vec::new();
    let v = Values::read_with_diagnostics(
        &Fake::with(&[
            ("gameplay", "newspells", "256"),
            ("gameplay", "saveall", "255"),
            ("gameplay", "unlockmoney", "-129"),
            ("gameplay", "aggrmod", "127"),
            ("gameplay", "manygameexe", "-128"),
        ]),
        |d| messages.push(d),
    );
    assert_eq!(v.flag("gameplay", "newspells"), Ok(0));
    assert_eq!(messages.len(), 3);
    for (key, written, used) in [
        ("newspells", "256", "0"),
        ("saveall", "255", "-1"),
        ("unlockmoney", "-129", "127"),
    ] {
        let d = messages
            .iter()
            .find(|d| d.key == format!("[gameplay] {key:?}"))
            .unwrap();
        assert!(d.warning);
        assert_eq!(
            d.message,
            format!("switch value {written} is outside the signed byte range; using {used}")
        );
    }
}

#[test]
fn byte_parameters_outside_0_to_255_warn_with_written_and_used_values() {
    let mut messages = Vec::new();
    Values::read_with_diagnostics(
        &Fake::with(&[
            ("gameplay", "astralbeam", "300"),
            ("gameplay", "destroyray", "255"),
            ("gameplay", "firearrow", "-1"),
            ("gameplay", "lightningbeam", "0"),
        ]),
        |d| messages.push(d),
    );
    let warned: Vec<_> = messages
        .iter()
        .map(|d| (d.warning, d.key.as_str(), d.message.as_str()))
        .collect();
    assert_eq!(
        warned,
        [
            (
                true,
                "[gameplay] \"astralbeam\"",
                "value 300 is outside the byte range 0..255; using 44"
            ),
            (
                true,
                "[gameplay] \"firearrow\"",
                "value -1 is outside the byte range 0..255; using 255"
            ),
        ]
    );
}

#[test]
fn a_switch_that_wraps_to_zero_still_warns_with_the_written_number() {
    let mut messages = Vec::new();
    let v = Values::read_with_diagnostics(
        &Fake::with(&[("gameplay", "newspells", "4294967296")]),
        |d| messages.push(d),
    );
    assert_eq!(v.flag("gameplay", "newspells"), Ok(0));
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0].message,
        "switch value 4294967296 is outside the signed byte range; using 0"
    );
}

#[test]
fn aliases_supply_only_absent_canonical_keys() {
    for (section, name, target_section, target_name) in [
        ("gameplay", "avrgarmors", "gameplay", "avgarmors"),
        ("gameplay", "spellsaimod", "speed", "spellsaimod"),
        (
            "gameplay",
            "spellsarealwaysconstructable",
            "speed",
            "spellsarealwaysconstructable",
        ),
        (
            "gameplay",
            "showallquestsinmulti",
            "speed",
            "showallquestsinmulti",
        ),
    ] {
        let mut messages = Vec::new();
        let v = Values::read_with_diagnostics(&Fake::with(&[(section, name, "1")]), |d| {
            messages.push(d)
        });
        assert_eq!(v.flag(target_section, target_name), Ok(1));
        assert_eq!(messages.len(), 1);
        assert!(!messages[0].warning);
        assert!(
            messages[0]
                .message
                .contains(&format!("[{section}] {name:?}"))
        );
        messages.clear();
        let v = Values::read_with_diagnostics(
            &Fake::with(&[(section, name, "1"), (target_section, target_name, "0")]),
            |d| messages.push(d),
        );
        assert_eq!(v.flag(target_section, target_name), Ok(0));
        assert_eq!(messages.len(), 1);
        assert!(messages[0].warning);
        assert!(
            messages[0]
                .message
                .contains(&format!("[{section}] {name:?}"))
        );
    }
}

#[test]
fn duplicates_warn_for_each_ignored_value_including_unknown_keys() {
    let messages = duplicate_keys(b"; ignored=1\r\n[gameplay]\r\nNewSpells=256\r\nnewspells =1\r\nnewspells=2\r\nunknown=first\r\nUNKNOWN=last\r\n[speed]\r\nnewspells=3\r\n");
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].key, "[gameplay] \"newspells\"");
    assert_eq!(
        messages[0].message,
        "duplicate key: using \"256\", ignoring \"1\""
    );
    assert_eq!(
        messages[1].message,
        "duplicate key: using \"256\", ignoring \"2\""
    );
    assert_eq!(
        messages[2].message,
        "duplicate key: using \"first\", ignoring \"last\""
    );
    assert!(messages.iter().all(|d| d.warning));
}

#[test]
fn integers_keep_their_width() {
    let v = Values::read(&Fake::with(&[
        ("gameplay", "maxcountofmerc", "300"),
        ("gameplay", "astralbeam", "300"),
        ("skills", "skills", "100"),
        ("aicastertable", "53", "-1"),
    ]));
    assert_eq!(v.int("gameplay", "maxcountofmerc"), Ok(300));
    assert_eq!(v.byte("gameplay", "astralbeam"), Ok(44));
    assert_eq!(v.byte("skills", "skills"), Ok(100));
    assert_eq!(v.byte("aicastertable", "53"), Ok(0xFF));
    assert!(matches!(
        v.int("gameplay", "astralbeam"),
        Err(KeyError::Kind { .. })
    ));
}

#[test]
fn fractional_values_are_kept_and_frame_keys_still_have_integer_values() {
    let v = Values::read(&Fake::with(&[
        ("gameplay", "exred", "0.5"),
        ("gameplay", "exgreen", "1.0"),
        ("gameplay", "hitdistmul", "0.0"),
        ("gameplay", "framex", "75.9"),
        ("gameplay", "headmod", "0.5"),
    ]));
    assert_eq!(v.float("gameplay", "exred"), Ok(0.5));
    assert_eq!(v.float("gameplay", "exgreen"), Ok(1.0));
    assert_eq!(v.float("gameplay", "hitdistmul"), Ok(0.0));
    assert_eq!(v.float("gameplay", "framex"), Ok(75.9));
    assert_eq!(v.text_int("gameplay", "framex"), Ok(75));
    assert_eq!(v.float("gameplay", "headmod"), Ok(0.5));
    let atoi: Vec<&str> = KEYS
        .iter()
        .filter(|k| {
            matches!(
                k.kind,
                Kind::Text {
                    conv: Conv::FloatInt,
                    ..
                }
            )
        })
        .map(|k| k.name)
        .collect();
    assert_eq!(atoi, ["framey", "framex"]);
    for name in [
        "parammin",
        "parammax",
        "balance",
        "exred",
        "exgreen",
        "exblue",
        "hitdistmul",
        "hitdistadd",
    ] {
        let v = Values::read(&Fake::with(&[("gameplay", name, "2.75")]));
        assert_eq!(v.float("gameplay", name), Ok(2.75), "{name}");
    }
}

#[test]
fn four_string_keys_go_through_the_digit_parser() {
    let digits: Vec<(&str, &str)> = KEYS
        .iter()
        .filter(|k| {
            matches!(
                k.kind,
                Kind::Text {
                    conv: Conv::Digits,
                    ..
                }
            )
        })
        .map(|k| (k.section, k.name))
        .collect();
    assert_eq!(
        digits,
        [
            ("rebalanceengine", "runconst"),
            ("rebalanceengine", "runconstex"),
            ("rebalanceengine", "pow"),
            ("gameplay", "expforint")
        ]
    );
    let v = Values::read(&Fake::with(&[
        ("gameplay", "expforint", "150"),
        ("rebalanceengine", "pow", "150.0"),
        ("gameplay", "str", "150"),
    ]));
    assert_eq!(v.double("gameplay", "expforint"), Ok(150.0));
    assert_eq!(v.double("rebalanceengine", "pow"), Ok(150.0));
    assert_eq!(v.float("gameplay", "str"), Ok(150.0));
}

#[test]
fn the_buffer_cuts_long_strings() {
    let long = "A".repeat(100);
    let v = Values::read(&Fake::with(&[("fonts", "newfont1", &long)]));
    assert_eq!(v.text("fonts", "newfont1"), Ok(&long.as_bytes()[..0x2F]));
}

#[test]
fn unread_names_and_aliases_are_not_canonical_table_entries() {
    for (section, name) in [
        ("gameplay", "aggrrange"),
        ("gameplay", "avrgarmors"),
        ("gameplay", "infinitydurability"),
        ("gameplay", "minsarmorslight"),
        ("gameplay", "showallquestsinmulti"),
        ("gameplay", "spellsaimod"),
        ("gameplay", "spellsarealwaysconstructable"),
        ("speed", "disablepausekey"),
        ("aicastertable", "54"),
        ("aicastertable", "55"),
        ("aioffendtable", "38"),
        ("aioffendtable", "39"),
        ("speedrun", "starthk"),
        ("speedrun", "opacity"),
    ] {
        assert!(key(section, name).is_none(), "[{section}] {name}");
    }
    // Aliases resolve to these canonical entries; the native reader also accepts the pause key without a space.
    for (section, name) in [
        ("speed", "spellsaimod"),
        ("speed", "spellsarealwaysconstructable"),
        ("speed", "showallquestsinmulti"),
        ("speed", "disablepausekey "),
        ("gameplay", "avgarmors"),
    ] {
        assert!(
            matches!(key(section, name).map(|k| k.kind), Some(Kind::Flag { .. })),
            "[{section}] {name:?}"
        );
    }
    let v = Values::read(&Fake::default());
    assert_eq!(
        v.flag("gameplay", "aggrrange"),
        Err(KeyError::Unknown {
            section: "gameplay".to_owned(),
            name: "aggrrange".to_owned()
        })
    );
}

#[test]
fn the_ai_tables_read_54_and_38_bytes() {
    for (section, count) in [("aicastertable", 54), ("aioffendtable", 38)] {
        for i in 0..count {
            assert_eq!(
                key(section, &i.to_string()).map(|k| k.kind),
                Some(Kind::Int {
                    default: 8,
                    width: Width::Byte
                }),
                "[{section}] {i}"
            );
        }
    }
}

/// The release package's spelladdon.ini.
const SHIPPED_INI: &str = include_str!("../../../../dist/spelladdon.ini");

/// The switches the shipped INI turns on: those that only fix a defect of game.exe. `ultrafixes` skips an object
/// the game cannot send instead of crashing; every other switch changes play.
const SHIPPED_ON: [(&str, &str, i8); 1] = [("gameplay", "ultrafixes", 1)];

/// The shipped INI's key lines as the system reads them: names and values trimmed, comments skipped.
fn ini_pairs(text: &str) -> Vec<(String, String, String)> {
    let mut section = String::new();
    let mut pairs = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.to_owned();
        } else if let Some((key, value)) = line.split_once('=')
            && !line.starts_with(';')
        {
            pairs.push((
                section.clone(),
                key.trim().to_owned(),
                value.trim().to_owned(),
            ));
        }
    }
    pairs
}

#[test]
fn the_shipped_ini_turns_on_only_the_bug_fixes() {
    let text = SHIPPED_INI;
    assert!(text.contains(&format!(
        "; Options: {}/blob/main/docs/spelladdon.md\r\n",
        env!("CARGO_PKG_REPOSITORY")
    )));
    assert_eq!(duplicate_keys(text.as_bytes()), []);
    let pairs = ini_pairs(text);
    assert_eq!(pairs.len(), KEYS.len());
    let fake = |pairs: &[(String, String, String)]| {
        Fake(
            pairs
                .iter()
                .map(|(s, k, v)| ((s.clone(), k.clone()), v.clone()))
                .collect(),
        )
    };
    let on: Vec<_> = Values::read(&fake(&pairs))
        .flags_on()
        .map(|(k, v)| (k.section, k.name, v))
        .collect();
    assert_eq!(on, SHIPPED_ON);
    // Without the fixes, the file reads as no file at all.
    let rest: Vec<_> = pairs
        .into_iter()
        .filter(|(s, k, _)| !SHIPPED_ON.iter().any(|(fs, fk, _)| fs == s && fk == k))
        .collect();
    let none = Values::read(&Fake::default());
    // The file writes these two with a decimal point, the original's defaults with a comma: the same numbers.
    let pointed = ["runconst", "runconstex"];
    let read = Values::read(&fake(&rest));
    for name in pointed {
        assert_eq!(
            read.double("rebalanceengine", name),
            none.double("rebalanceengine", name),
            "{name}"
        );
    }
    let rest: Vec<_> = rest
        .into_iter()
        .filter(|(s, k, _)| !(s == "rebalanceengine" && pointed.contains(&k.as_str())))
        .collect();
    assert_eq!(Values::read(&fake(&rest)), none);
}
