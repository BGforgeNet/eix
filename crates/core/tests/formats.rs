//! The registry reader and writer, the options model and the group predicates, on files built by
//! `um-multitool inireg --ini2reg`.
//!
//! `tests/data/{all,all-ai,defaults}.reg` are `um-multitool inireg --ini2reg` of a mod config with every option on,
//! its ai.reg (the addon's sections and one game key), and a minimal one. `tests/data/groups-{all,defaults}.tsv` are
//! the patch groups the original addon.dll applies with those inputs.

use std::collections::BTreeMap;
use std::path::PathBuf;

use eix_core::eix_ini::EixIni;
use eix_core::groups::{Decision, Group};
use eix_core::options::{
    AiReg, CommandLine, ConfigReg, Environment, MpZone, Options, SETTING_DWORDS, SettingValue,
    Settings,
};
use eix_core::reg::{
    Key, LoadError, MAGIC, Registry, Section, Value, ai_load_error, config_load_error, loads,
};

fn data(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn parse(name: &str) -> Registry {
    Registry::parse(&data(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

const FIXTURES: [&str; 3] = ["all.reg", "all-ai.reg", "defaults.reg"];

#[test]
fn fixtures_round_trip_byte_for_byte() {
    for name in FIXTURES {
        let bytes = data(name);
        let reg = Registry::parse(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(reg.to_bytes().expect("writes"), bytes, "{name}");
    }
}

#[test]
fn fixtures_hold_the_ini_values() {
    let all = parse("all.reg");
    let names: Vec<&[u8]> = all.sections.iter().map(|s| s.name.as_slice()).collect();
    assert_eq!(names, [&b"MOD"[..], b"RES", b"CONFIG", b"DEV", b"MPZone"]);
    assert_eq!(all.get_str("MOD", "Title"), Some(&b"all-on"[..]));
    assert_eq!(all.get("MOD", "Multi"), Some(&Value::Int(0)));
    assert_eq!(
        all.get_str("CONFIG", "ai.reg"),
        Some(&b"config\\ai.reg"[..])
    );

    let ai = parse("all-ai.reg");
    assert_eq!(ai.get("AddonExp", "MulFactor"), Some(&Value::Float(2.0)));
    assert_eq!(ai.get("Logic", "GuardRadius"), Some(&Value::Int(10)));
    assert_eq!(
        ai.get_float_array("DifficultyLevels", "Absorption"),
        Some(vec![1.0, 0.33])
    );
    // The INI converter writes a repeated key as an array.
    assert!(matches!(
        ai.get("DifficultyLevels", "Attack"),
        Some(Value::FloatArray(_))
    ));
}

#[test]
fn lookups_ignore_case_and_take_the_first_match() {
    let all = parse("all.reg");
    assert_eq!(
        all.get_str("config", "AI.REG"),
        all.get_str("CONFIG", "ai.reg")
    );
    let reg = Registry {
        sections: vec![
            Section {
                name: b"Addon".to_vec(),
                keys: vec![Key {
                    name: b"A".to_vec(),
                    value: Value::Int(1),
                }],
            },
            Section {
                name: b"ADDON".to_vec(),
                keys: vec![Key {
                    name: b"B".to_vec(),
                    value: Value::Int(2),
                }],
            },
        ],
    };
    assert_eq!(reg.get_int("addon", "a"), Some(1));
    assert_eq!(
        reg.get_int("addon", "b"),
        None,
        "only the first section of a name is found"
    );
}

#[test]
fn numeric_readers_accept_the_other_type() {
    let reg = Registry {
        sections: vec![Section {
            name: b"S".to_vec(),
            keys: vec![
                Key {
                    name: b"f".to_vec(),
                    value: Value::Float(2.9),
                },
                Key {
                    name: b"i".to_vec(),
                    value: Value::Int(3),
                },
                Key {
                    name: b"s".to_vec(),
                    value: Value::Str(b"x".to_vec()),
                },
            ],
        }],
    };
    assert_eq!(reg.get_int("S", "f"), Some(2));
    assert_eq!(reg.get_float("S", "i"), Some(3.0));
    assert_eq!(reg.get_int("S", "s"), None);
    assert_eq!(reg.get_str("S", "i"), None);
    assert_eq!(reg.get_str_array("S", "s"), Some(vec![&b"x"[..]]));
}

/// The readers' conversions as the original's compiled code does them.
#[test]
fn the_other_type_converts_as_the_originals_readers_do() {
    // A float read as an integer: truncated toward zero through 64 bits, the low word kept.
    assert_eq!(Value::Float(-2.9).as_int(), Some(-2));
    assert_eq!(Value::Float(3.0e9).as_int(), Some(-1_294_967_296));
    assert_eq!(Value::Float(f32::NAN).as_int(), Some(0));
    // An integer read as a float keeps its sign.
    assert_eq!(Value::Int(3).as_float(), Some(3.0));
    assert_eq!(Value::Int(-1).as_float(), Some(-1.0));
    assert_eq!(Value::Int(i32::MIN).as_float(), Some(-2_147_483_648.0));
    // A float array key takes a float array or a float scalar, not an integer scalar.
    assert_eq!(Value::Float(2.5).as_float_array(), Some(vec![2.5]));
    assert_eq!(
        Value::FloatArray(vec![1.0, 2.0]).as_float_array(),
        Some(vec![1.0, 2.0])
    );
    assert_eq!(Value::Int(2).as_float_array(), None);
    assert_eq!(Value::IntArray(vec![1, 2]).as_float_array(), None);
}

/// Which files stop the game: the original's loader tests the header only.
#[test]
fn the_loader_refuses_a_file_without_the_header_and_nothing_else() {
    let header = |count: u16| {
        let mut v = MAGIC.to_vec();
        v.extend_from_slice(&count.to_le_bytes());
        v
    };
    assert!(!loads(b""));
    assert!(!loads(&MAGIC));
    assert!(!loads(
        &MAGIC.iter().chain(&[0]).copied().collect::<Vec<u8>>()
    ));
    assert!(!loads(b"this is not a registry file\n"));
    assert!(loads(&header(0)));
    assert_eq!(Registry::load(b""), Err(LoadError::Unusable));
    assert_eq!(Registry::load(b"not a registry"), Err(LoadError::Unusable));
    assert_eq!(Registry::load(&header(0)), Ok(Registry::default()));
    // The header but a table that is not there: the original goes on (unchecked offsets), ours reports it.
    assert!(matches!(
        Registry::load(&header(5)),
        Err(LoadError::Malformed(_))
    ));
    assert_eq!(
        config_load_error("C:\\EIStarter\\Mods\\inv\\config.reg"),
        "Can't open file: C:\\EIStarter\\Mods\\inv\\config.reg"
    );
    assert_eq!(
        ai_load_error("C:\\EIStarter\\Mods\\inv\\config\\ai.reg"),
        "Fail to load: C:\\EIStarter\\Mods\\inv\\config\\ai.reg"
    );
}

#[test]
fn every_value_type_round_trips() {
    let key = |n: &str, value| Key {
        name: n.as_bytes().to_vec(),
        value,
    };
    let reg = Registry {
        sections: vec![
            Section {
                name: vec![0xCF, 0xF0, 0xE8],
                keys: vec![
                    key("int", Value::Int(i32::MIN)),
                    key("float", Value::Float(-0.5)),
                    key("str", Value::Str(vec![0xFF, 0x00, b'a'])),
                    key("empty", Value::Str(Vec::new())),
                    key("ints", Value::IntArray(vec![1, -2, 3])),
                    key("floats", Value::FloatArray(vec![1.5])),
                    key("strs", Value::StrArray(vec![b"a".to_vec(), Vec::new()])),
                    key("none", Value::IntArray(Vec::new())),
                ],
            },
            Section {
                name: b"empty".to_vec(),
                keys: Vec::new(),
            },
        ],
    };
    let bytes = reg.to_bytes().expect("writes");
    assert_eq!(Registry::parse(&bytes).as_ref(), Ok(&reg));
    // A keyless section is a missing key, where the original's lookup divides by its slot count 0 (loader.md).
    assert_eq!(
        Registry::parse(&bytes).expect("parses").get("empty", "x"),
        None
    );
    let empty = Registry::default().to_bytes().expect("writes");
    assert_eq!(empty, [MAGIC.as_slice(), &[0, 0]].concat());
    assert_eq!(Registry::parse(&empty), Ok(Registry::default()));
}

fn u16_at(b: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([b[at], b[at + 1]]))
}

fn u32_at(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize
}

fn home(name: &str, slots: usize) -> usize {
    let sum: i32 = name
        .bytes()
        .map(|b| i32::from(b.to_ascii_lowercase() as i8))
        .sum();
    sum as usize % slots
}

/// The game's lookup: start at the home slot, follow the chain until the stored name matches. Returns the offset of
/// the entry's record.
fn find(
    file: &[u8],
    table: usize,
    count: usize,
    base: usize,
    name: &str,
    name_at: usize,
) -> Option<usize> {
    let mut slot = home(name, count);
    loop {
        let entry = table + 6 * slot;
        let record = base + u32_at(file, entry + 2);
        let len = u16_at(file, record + name_at - 2);
        if file[record + name_at..record + name_at + len].eq_ignore_ascii_case(name.as_bytes()) {
            return Some(record);
        }
        slot = u16_at(file, entry);
        if slot == 0xFFFF {
            return None;
        }
    }
}

#[test]
fn written_tables_can_be_walked_the_way_the_game_looks_up() {
    // Many sections and keys, mixed case and colliding names, so the chains get long.
    let sections: Vec<Section> = (0..40)
        .map(|s| Section {
            name: format!("Sec{s}").into_bytes(),
            keys: (0..120)
                .map(|k| Key {
                    name: format!("Key{}", (k * 7) % 120 + s).into_bytes(),
                    value: Value::Int(k),
                })
                .collect::<Vec<_>>()
                .into_iter()
                .enumerate()
                .map(|(i, mut key)| {
                    key.name.extend_from_slice(format!("_{i}").as_bytes());
                    key
                })
                .collect(),
        })
        .collect();
    let reg = Registry { sections };
    let bytes = reg.to_bytes().expect("writes");
    let count = reg.sections.len();
    for section in &reg.sections {
        let name = String::from_utf8_lossy(&section.name).to_uppercase();
        let at = find(&bytes, 6, count, 0, &name, 4).expect("section found");
        let keys = u16_at(&bytes, at);
        let table = at + 4 + section.name.len();
        for key in &section.keys {
            let name = String::from_utf8_lossy(&key.name).to_lowercase();
            // A key record is: type byte, u16 name length, name.
            let found = find(&bytes, table, keys, at, &name, 3).expect("key found");
            assert_eq!(
                bytes[found + 3..found + 3 + key.name.len()].to_ascii_lowercase(),
                name.as_bytes()
            );
        }
    }
    assert_eq!(Registry::parse(&bytes).as_ref(), Ok(&reg));
}

#[test]
fn the_writer_refuses_what_does_not_fit() {
    let long = Section {
        name: vec![b'x'; 65_536],
        keys: Vec::new(),
    };
    let err = Registry {
        sections: vec![long],
    }
    .to_bytes()
    .expect_err("name too long");
    assert!(err.to_string().contains("section name"), "{err}");
}

#[test]
fn malformed_input_names_the_offset_and_the_expectation() {
    let err = Registry::parse(b"").expect_err("empty");
    assert_eq!((err.offset, err.expected), (0, "magic FB 3E AB 45"));
    let err = Registry::parse(b"NOPE\0\0").expect_err("magic");
    assert_eq!(err.offset, 0);
    let mut bytes = data("defaults.reg");
    bytes.truncate(40);
    let err = Registry::parse(&bytes).expect_err("truncated");
    assert!(err.offset <= bytes.len(), "{err}");
    assert!(err.to_string().contains("expected"), "{err}");
    // The first key's type byte sits at offset 31 in defaults.reg.
    let mut bytes = data("defaults.reg");
    bytes[31] = 0x07;
    let err = Registry::parse(&bytes).expect_err("type byte");
    assert_eq!(err.offset, 31, "{err}");
    // A section offset past the end.
    let mut bytes = data("defaults.reg");
    bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(Registry::parse(&bytes).is_err());
}

#[test]
fn overlapping_data_cannot_multiply_memory() {
    // 60000 sections all pointing at one section whose name is 60000 bytes long.
    let name_len: u16 = 60_000;
    let sections: u16 = 60_000;
    let table = 6 + 6 * usize::from(sections);
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&sections.to_le_bytes());
    for _ in 0..sections {
        bytes.extend_from_slice(&(-1i16).to_le_bytes());
        bytes.extend_from_slice(&(table as u32).to_le_bytes());
    }
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&name_len.to_le_bytes());
    bytes.resize(bytes.len() + usize::from(name_len), b'x');
    let err = Registry::parse(&bytes).expect_err("overlap");
    assert!(err.expected.contains("no other"), "{err}");
}

#[test]
fn damaged_fixtures_never_panic() {
    for name in FIXTURES {
        let bytes = data(name);
        for end in 0..bytes.len() {
            let _ = Registry::parse(&bytes[..end]);
        }
        for i in 0..bytes.len() {
            for flip in [0x01, 0x80, 0xFF] {
                let mut copy = bytes.clone();
                copy[i] ^= flip;
                let _ = Registry::parse(&copy);
            }
        }
    }
}

// --- options and group predicates ---------------------------------------------------------------------------------

/// The registry values that go with `all.reg`.
fn all_on_values() -> BTreeMap<String, SettingValue> {
    let dword = |v| SettingValue::Dword(v);
    BTreeMap::from([
        ("WideScreen".to_owned(), dword(1)),
        ("ObjectsDrawRadius".to_owned(), dword(200)),
        ("LandscapeDrawRadius".to_owned(), dword(200)),
        ("MP console".to_owned(), dword(1)),
        (
            "Console AutoExec".to_owned(),
            SettingValue::MultiText(vec!["addon info".to_owned()]),
        ),
        ("MQ client".to_owned(), dword(1)),
        ("MP Speed-control".to_owned(), dword(1)),
        ("Rate".to_owned(), dword(2500)),
        (
            "VideoResolution".to_owned(),
            SettingValue::Text("1056x768".to_owned()),
        ),
    ])
}

fn all_on_options() -> Options {
    Options {
        config: ConfigReg::from_registry(&parse("all.reg")),
        ai: AiReg::from_registry(&parse("all-ai.reg")),
        settings: Settings::from_values(&all_on_values()),
        eix: EixIni::default(),
        env: Environment {
            mod_loaded: true,
            bink_dll_in_mod: true,
            game_bink_loaded: true,
            bink_open_exported: true,
            ..Environment::default()
        },
    }
}

/// The minimal inputs: the starter's registry sets MQ client and MP Speed-control and a 1024x768 mode (WideScreen is
/// unset, so on, but 4:3 is not wide); the mod has no ai.reg.
fn defaults_options() -> Options {
    let values = BTreeMap::from([
        ("MP console".to_owned(), SettingValue::Dword(0)),
        ("MP Speed-control".to_owned(), SettingValue::Dword(1)),
        ("MQ client".to_owned(), SettingValue::Dword(1)),
        (
            "VideoResolution".to_owned(),
            SettingValue::Text("1024x768".to_owned()),
        ),
    ]);
    Options {
        config: ConfigReg::from_registry(&parse("defaults.reg")),
        ai: AiReg::default(),
        settings: Settings::from_values(&values),
        eix: EixIni::default(),
        env: Environment {
            mod_loaded: true,
            // game.exe imports binkw32.dll, so it is loaded in every run.
            game_bink_loaded: true,
            bink_open_exported: true,
            ..Environment::default()
        },
    }
}

/// The rows of a groups file: name and state (`applied` or `disabled`).
fn original_groups(file: &str) -> Vec<(String, String)> {
    let text = String::from_utf8(data(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| match l.split_once('\t') {
            Some((name, state)) => (name.to_owned(), state.to_owned()),
            None => panic!("{file}: {l:?}"),
        })
        .collect()
}

fn check_against_original(file: &str, options: &Options, expect_unlisted_disabled: bool) {
    let recorded = original_groups(file);
    let by_name: BTreeMap<&str, &str> = recorded
        .iter()
        .map(|(n, s)| (n.as_str(), s.as_str()))
        .collect();
    let mut checked = 0;
    for &group in Group::ALL {
        let decision = group.decide(options);
        match by_name.get(group.name()) {
            Some(&state) => {
                let want = state == "applied";
                assert_eq!(
                    decision.is_enabled(),
                    want,
                    "{file}: {} is {state}, got {decision:?}",
                    group.name()
                );
                checked += 1;
            }
            None if expect_unlisted_disabled => {
                assert!(
                    !decision.is_enabled(),
                    "{file}: unlisted {} should be disabled",
                    group.name()
                );
            }
            None => panic!("{file}: {} is not listed", group.name()),
        }
    }
    let known: Vec<&str> = Group::ALL.iter().map(|g| g.name()).collect();
    for (name, _) in &recorded {
        assert!(
            name == "(start-up)" || known.contains(&name.as_str()),
            "{file}: {name} has no predicate"
        );
    }
    assert!(checked > 0);
}

#[test]
fn there_are_87_groups_in_five_tables() {
    assert_eq!(Group::ALL.len(), 87);
    let mut names: Vec<&str> = Group::ALL.iter().map(|g| g.name()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 87);
}

#[test]
fn all_on_inputs_give_the_originals_groups() {
    let o = all_on_options();
    check_against_original("groups-all.tsv", &o, false);
    let applied = o
        .decide_all()
        .iter()
        .filter(|(_, d)| d.is_enabled())
        .count();
    assert_eq!(applied, 85);
}

#[test]
fn default_inputs_give_the_originals_groups() {
    check_against_original("groups-defaults.tsv", &defaults_options(), true);
}

#[test]
fn the_command_line_groups_follow_the_arguments() {
    let mut o = all_on_options();
    o.env.command_line = CommandLine::Addon;
    assert!(Group::CommandLine.decide(&o).is_enabled());
    assert!(!Group::GraphGenPatches.decide(&o).is_enabled());
    o.env.command_line = CommandLine::AiGraph;
    assert!(Group::GraphGenPatches.decide(&o).is_enabled());
}

#[test]
fn a_disabled_group_says_why() {
    let o = defaults_options();
    let Decision::Disabled(reason) = Group::ConfigMusicReg.decide(&o) else {
        panic!("enabled")
    };
    assert!(reason.contains("music.reg"), "{reason}");
    let Decision::Disabled(reason) = Group::PerkLimit.decide(&o) else {
        panic!("enabled")
    };
    assert!(reason.contains("T5"), "{reason}");
}

#[test]
fn thresholds_and_gates() {
    let mut o = all_on_options();
    // AntiCrash: the ai.reg value wins over the registry.
    o.ai.addon.anti_crash = Some(0);
    assert!(!Group::FixCreateItem.decide(&o).is_enabled());
    o.ai.addon.anti_crash = None;
    o.settings.anti_crash = 0;
    assert!(!Group::DisableExitOnCrashReport.decide(&o).is_enabled());
    // Without an ai.reg the registry value decides, whatever the parsed defaults say.
    o.config.config.ai_reg.clear();
    o.ai.addon.anti_crash = Some(1);
    o.settings.anti_crash = 1;
    assert!(Group::FixCreateItem.decide(&o).is_enabled());

    let mut o = all_on_options();
    // 4:3 is not wide; 800x480 is too small. A value below 640x480 or not WxH goes on to the group, which fails.
    for (res, on) in [
        ("1024x768", false),
        ("1056x768", true),
        ("799x400", true),
        ("800x480", false),
        ("bad", true),
    ] {
        o.settings.video_resolution = Some(res.to_owned());
        assert_eq!(Group::WideScreen.decide(&o).is_enabled(), on, "{res}");
    }
    // A perk limit of 100 everywhere is not a change; a Limit of exactly 0 is.
    o.ai.exp.limit = 0.0;
    assert!(Group::ExpLimit.decide(&o).is_enabled());
    o.ai.exp.limit = -0.5;
    assert!(!Group::ExpLimit.decide(&o).is_enabled());
    // Only a lights<X>.ini key of 11 characters or more counts.
    let mut cfg = parse("all.reg");
    cfg.sections[2]
        .keys
        .retain(|k| k.name != b"lightsgipat.ini");
    cfg.sections[2].keys.push(Key {
        name: b"lights.ini".to_vec(),
        value: Value::Str(b"x".to_vec()),
    });
    o.config = ConfigReg::from_registry(&cfg);
    assert!(!Group::ConfigLightsIni.decide(&o).is_enabled());
    // Keys keep their case, and a value that is not a string stands for the mod folder alone (an empty value).
    cfg.sections[2].keys.push(Key {
        name: b"LightsZone1.ini".to_vec(),
        value: Value::Int(1),
    });
    let lights = ConfigReg::from_registry(&cfg).config.lights_ini;
    assert_eq!(lights, [(b"LightsZone1.ini".to_vec(), Vec::new())]);
    // A duplicate zone name is reported, not hidden.
    let mut o = all_on_options();
    o.config.mp_zone.ingos = Some(vec![b"zone0".to_vec()]);
    assert_eq!(o.config.mp_zone.duplicate_name(), Some(&b"zone0"[..]));
}

#[test]
fn zone_names_repeat_within_or_across_keys_ignoring_case() {
    let list = |v: &[&[u8]]| Some(v.iter().map(|z| z.to_vec()).collect::<Vec<_>>());
    let mut z = MpZone {
        gipath: list(&[b"zone1", b"zone2"]),
        ingos: list(&[b"zone3"]),
        suslanger: list(&[b"zone4"]),
    };
    assert_eq!(z.duplicate_name(), None);
    z.ingos = list(&[b"zone3", b"ZONE3"]);
    assert_eq!(z.duplicate_name(), Some(&b"zone3"[..]));
    z.ingos = list(&[b"zone3", b"Zone2"]);
    assert_eq!(z.duplicate_name(), Some(&b"Zone2"[..]));
    // Without all three keys the group is off and nothing is checked.
    z.suslanger = None;
    assert_eq!(z.duplicate_name(), None);
    // One empty last entry of an array is dropped.
    let reg = Registry {
        sections: vec![Section {
            name: b"MPZone".to_vec(),
            keys: vec![Key {
                name: b"Gipath".to_vec(),
                value: Value::StrArray(vec![b"zone1".to_vec(), Vec::new()]),
            }],
        }],
    };
    assert_eq!(
        ConfigReg::from_registry(&reg).mp_zone.gipath,
        list(&[b"zone1"])
    );
}

#[test]
fn lua_main_script_defaults_to_main_lua() {
    let missing = AiReg::from_registry(&Registry::default());
    assert_eq!(missing.addon.lua_main_script_path, b"main.lua");
    let all = AiReg::from_registry(&Registry::parse(&data("all-ai.reg")).unwrap());
    assert_eq!(all.addon.lua_main_script_path, b"lua\\main.lua");
}

#[test]
fn auto_recover_is_on_unless_the_registry_says_zero() {
    let with = |v: Option<u32>| {
        let values = v
            .map(|v| ("AutoRecover".to_owned(), SettingValue::Dword(v)))
            .into_iter()
            .collect();
        Settings::from_values(&values).auto_recover
    };
    assert_eq!(with(None), 1);
    assert_eq!(with(Some(0)), 0);
    assert_eq!(with(Some(2)), 2);
}

#[test]
fn every_dword_setting_is_in_the_list_the_addon_fetches() {
    // No default is 7, so a DWORD option read under a name missing from the list keeps its default.
    let values = SETTING_DWORDS
        .iter()
        .map(|n| ((*n).to_owned(), SettingValue::Dword(7)))
        .collect();
    let s = Settings::from_values(&values);
    let read = [
        s.wide_screen,
        s.objects_draw_radius,
        s.landscape_draw_radius,
        s.anti_crash,
        s.auto_recover,
        s.rate,
        s.mq_client,
        s.mp_speed_control,
        s.mp_console,
    ];
    assert_eq!(read, [7; 9]);
}

#[test]
fn show_tutorial_default_is_unset_unless_the_mod_names_it() {
    let mut cfg = parse("all.reg");
    assert_eq!(
        ConfigReg::from_registry(&cfg).mod_.show_tutorial_default,
        None
    );
    let section = cfg
        .sections
        .iter_mut()
        .find(|s| s.name == b"MOD")
        .expect("all.reg has a [MOD] section");
    section.keys.push(Key {
        name: b"ShowTutorialDefault".to_vec(),
        value: Value::Int(0),
    });
    assert_eq!(
        ConfigReg::from_registry(&cfg).mod_.show_tutorial_default,
        Some(0)
    );
}
