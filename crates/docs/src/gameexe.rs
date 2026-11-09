//! The game's keyword table, read from game.exe, against the game entries of the script reference.

use std::collections::BTreeMap;

use eix_sites::K_SCRIPT_KEYWORDS;

use crate::scripting::{Scripting, Source};

/// The words of the script language that are not functions or commands.
const LANGUAGE: [&str; 6] = [
    "WorldScript",
    "Script",
    "DeclareScript",
    "GlobalVars",
    "if",
    "then",
];

/// One keyword of the table.
#[derive(Debug, PartialEq, Eq)]
struct Keyword {
    /// The result letter and the argument letters of a function.
    function: Option<String>,
    /// The argument letters of a command.
    command: Option<String>,
}

fn u16_at(file: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([file[at], file[at + 1]]))
}

fn u32_at(file: &[u8], at: usize) -> usize {
    u32::from_le_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]) as usize
}

/// The file offset of a virtual address.
fn offset(file: &[u8], address: usize) -> usize {
    let pe = u32_at(file, 0x3C);
    let optional = pe + 24;
    let base = u32_at(file, optional + 28);
    (0..u16_at(file, pe + 6))
        .map(|i| optional + u16_at(file, pe + 20) + 40 * i)
        .find_map(|s| {
            let (rva, size, raw) = (
                u32_at(file, s + 12),
                u32_at(file, s + 16),
                u32_at(file, s + 20),
            );
            let start = base + rva;
            (start..start + size)
                .contains(&address)
                .then(|| raw + address - start)
        })
        .unwrap_or_else(|| panic!("address {address:#X} is in no section of game.exe"))
}

fn text(file: &[u8], address: usize) -> String {
    let start = offset(file, address);
    let len = file[start..]
        .iter()
        .position(|&b| b == 0)
        .expect("a string ends");
    String::from_utf8_lossy(&file[start..start + len]).into_owned()
}

fn keywords(file: &[u8]) -> BTreeMap<String, Keyword> {
    let mut table = BTreeMap::new();
    // Records of four dwords (name, id, function signature, command signature) up to the first without a name.
    let mut at = offset(file, K_SCRIPT_KEYWORDS);
    while u32_at(file, at) != 0 {
        let name = text(file, u32_at(file, at));
        let (function, command) = (u32_at(file, at + 8), u32_at(file, at + 12));
        let word = |address: usize| (address != 0).then(|| text(file, address));
        table.insert(
            name,
            Keyword {
                function: word(function),
                command: word(command),
            },
        );
        at += 16;
    }
    table
}

#[test]
#[ignore = "needs the game's game.exe: EIX_GAME_DIR=<the game folder, as a Windows path under Wine>"]
fn the_game_entries_match_the_keyword_table() {
    let dir = std::env::var("EIX_GAME_DIR").expect("EIX_GAME_DIR names the game folder");
    let file = std::fs::read(std::path::Path::new(&dir).join("game.exe")).expect("game.exe reads");
    let table = keywords(&file);
    let data = Scripting::parse(crate::SCRIPTING_DATA).expect("docs/scripting.yml is valid");
    let mut problems = Vec::new();

    let mut judged = 0;
    for (name, keyword) in &table {
        if LANGUAGE.contains(&name.as_str()) || !name.starts_with(|c: char| c.is_ascii_alphabetic())
        {
            continue;
        }
        judged += 1;
        let game = data
            .entries
            .iter()
            .find(|e| e.source == Source::Game && e.name == *name);
        let addon = data
            .entries
            .iter()
            .any(|e| e.source == Source::Addon && e.name == *name);
        match (game, addon) {
            (Some(entry), _) => {
                let signature = keyword.function.as_ref().or(keyword.command.as_ref());
                if signature != Some(&entry.signature()) {
                    problems.push(format!(
                        "{name}: the table has {signature:?}, the data file {:?}",
                        entry.signature()
                    ));
                }
            }
            // The addon takes the game's id and its own signature: the entry is documented under the addon.
            (None, true) => {}
            (None, false) => {
                problems.push(format!("{name} is in the table but not in the data file"))
            }
        }
    }
    for entry in data.entries.iter().filter(|e| e.source == Source::Game) {
        if !table.contains_key(&entry.name) {
            problems.push(format!(
                "{} is in the data file but not in the table",
                entry.name
            ));
        }
    }
    assert!(judged > 200, "only {judged} keywords judged");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
