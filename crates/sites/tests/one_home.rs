//! Code names a game.exe address only through this crate: no hex literal inside game.exe's image appears in the
//! extender crates' code. Comments and strings may cite addresses. `core` and the tools stay out: they hold no game
//! code, and their tests use addresses as plain sample values.

use std::path::{Path, PathBuf};

/// game.exe RU 1.06's image, from `.text` through `.data`.
const IMAGE: std::ops::Range<u64> =
    eix_sites::TEXT_SECTION.ru() as u64..eix_sites::RSRC_SECTION.ru() as u64;

/// The crates that stay out of the scan: this table, and `core` (see the top).
const OUT: [&str; 2] = ["sites", "core"];

/// Every crate under `crates/` but [`OUT`], so a new one is scanned without a change here.
fn scanned_crates(crates: &Path) -> Vec<PathBuf> {
    let entries = std::fs::read_dir(crates).unwrap_or_else(|e| panic!("{}: {e}", crates.display()));
    let mut out: Vec<PathBuf> = entries
        .map(|e| {
            e.unwrap_or_else(|e| panic!("{}: {e}", crates.display()))
                .path()
        })
        .filter(|p| p.is_dir() && !p.file_name().is_some_and(|n| OUT.iter().any(|o| n == *o)))
        .collect();
    out.sort();
    out
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The hex literals in `src`'s code, outside comments, strings and chars, as (line, text).
fn hex_literals(src: &str) -> Vec<(usize, String)> {
    let b = src.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    let line_of = |at: usize| src[..at].matches('\n').count() + 1;
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    while i < b.len() {
        let rest = &src[i..];
        if rest.starts_with("//") {
            i += rest.find('\n').unwrap_or(rest.len());
        } else if rest.starts_with("/*") {
            let mut depth = 0usize;
            while i < b.len() {
                if src[i..].starts_with("/*") {
                    depth += 1;
                    i += 2;
                } else if src[i..].starts_with("*/") {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if b[i] == b'r'
            && (i == 0 || !word(b[i - 1]))
            && rest[1..].trim_start_matches('#').starts_with('"')
        {
            let hashes = rest[1..].len() - rest[1..].trim_start_matches('#').len();
            let close = format!("\"{}", "#".repeat(hashes));
            let body = i + 2 + hashes;
            let end = src[body..]
                .find(&close)
                .unwrap_or_else(|| panic!("line {}: unterminated raw string", line_of(i)));
            i = body + end + close.len();
        } else if b[i] == b'"' {
            i += 1;
            while i < b.len() && b[i] != b'"' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
        } else if b[i] == b'\'' {
            // A char literal ('x', '\n', '\''); otherwise a lifetime or label.
            let len = match (b.get(i + 1), b.get(i + 2)) {
                // The escaped character itself may be a quote: the closing one comes after it.
                (Some(b'\\'), _) => src[i + 3..].find('\'').map_or(1, |n| n + 4),
                (Some(_), Some(b'\'')) => 3,
                _ => 1,
            };
            i += len;
        } else if rest.starts_with("0x") && (i == 0 || !word(b[i - 1])) {
            let len = rest.bytes().take_while(|&c| word(c)).count();
            found.push((line_of(i), rest[..len].to_owned()));
            i += len;
        } else {
            i += 1;
        }
    }
    found
}

/// A literal's value, its `_` separators and any type suffix dropped.
fn value(literal: &str) -> Option<u64> {
    let digits: String = literal[2..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit() || *c == '_')
        .filter(|c| *c != '_')
        .collect();
    u64::from_str_radix(&digits, 16).ok()
}

#[test]
fn no_game_address_outside_the_table() {
    let crates = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let scanned = scanned_crates(&crates);
    let mut files = Vec::new();
    for c in &scanned {
        rust_files(c, &mut files);
    }
    assert!(
        files.len() > scanned.len(),
        "found only {} files under {}",
        files.len(),
        crates.display()
    );
    let mut stray = Vec::new();
    for f in &files {
        let src = std::fs::read_to_string(f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        for (line, literal) in hex_literals(&src) {
            if value(&literal).is_some_and(|v| IMAGE.contains(&v)) {
                stray.push(format!("{}:{line}: {literal}", f.display()));
            }
        }
    }
    assert!(
        stray.is_empty(),
        "game.exe addresses outside eix-sites; add each to crates/sites/src/ru106.rs and use its name:\n{}",
        stray.join("\n")
    );
}

#[test]
fn the_scan_sees_code_and_skips_comments_and_strings() {
    let src = "// 0x55_3B07\nconst A: usize = 0x55_3B07; /* 0x55_3B08 */ let s = \"0x55_3B09\"; let c = '\\'';\n\
               let r = r#\"0x55_3B0A\"#; let l: &'static u8 = &0x0F; let d = 0x0040_2000u32;";
    let lits: Vec<_> = hex_literals(src)
        .into_iter()
        .map(|(line, l)| {
            let v = value(&l);
            (line, l, v)
        })
        .collect();
    assert_eq!(
        lits,
        [
            (2, "0x55_3B07".to_owned(), Some(0x55_3B07)),
            (3, "0x0F".to_owned(), Some(0x0F)),
            (3, "0x0040_2000u32".to_owned(), Some(0x40_2000)),
        ]
    );
}
