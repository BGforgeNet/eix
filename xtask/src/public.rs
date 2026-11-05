//! The public-text check: `main` is published, so no file in the tree may carry addresses or code of the original
//! modules, or notes on how a result was tested. A hit names the file, the line and the rule.

use std::path::{Path, PathBuf};

/// This file states the rules, so it is the one file they do not scan.
const SELF: &str = "xtask/src/public.rs";

/// Directories that hold no authored text.
const SKIP_DIRS: [&str; 2] = [".git", "target"];

/// The testing notes' evidence marks, matched as written: lower case, `[v]` is an index (`vec![v]`).
const MARKS: [&str; 2] = ["[V]", "[V-"];

/// Tools of the testing notes, which are not part of this tree; matched ignoring case.
const PHRASES: [&str; 5] = ["disassembly", "disassembler", "objdump", "ghidra", "emulat"];

/// Whole words (ASCII letters, digits and `_` join a word) that name the test bench or the original's code caves.
const WORDS: [&str; 4] = ["lab", "labs", "cave", "caves"];

/// A bit flag that falls in the range below; no address of the original modules is this round.
const NOT_AN_ADDRESS: u64 = 0x1000_0000;

/// The original modules load at 0x10000000 and up; game.exe's own addresses stay below 0x800000.
fn module_address(value: u64) -> bool {
    (0x1000_0000..0x1100_0000).contains(&value) && value != NOT_AN_ADDRESS
}

fn word_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// The rules one line breaks.
pub(crate) fn line_hits(line: &str) -> Vec<String> {
    let lower = line.to_ascii_lowercase();
    let b = lower.as_bytes();
    let mut hits: Vec<String> = MARKS
        .iter()
        .filter(|m| line.contains(*m))
        .chain(PHRASES.iter().filter(|p| lower.contains(*p)))
        .map(|p| format!("names {p:?}"))
        .collect();
    let mut i = 0;
    while i < b.len() {
        if !word_char(b[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && word_char(b[i]) {
            i += 1;
        }
        let word = &lower[start..i];
        if WORDS.contains(&word) {
            hits.push(format!("the word {word:?}"));
        }
        let hex = word.strip_prefix("0x").map(|h| h.replace('_', ""));
        if let Some(value) = hex.and_then(|h| u64::from_str_radix(&h, 16).ok())
            && module_address(value)
        {
            hits.push(format!("an address in the original modules: {word}"));
        }
    }
    hits
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        let skipped = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| SKIP_DIRS.contains(&n));
        if path.is_dir() {
            if !skipped {
                files(&path, out)?;
            }
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// Every hit under `root`, as `path:line: rule`. Binary files are scanned too, since their strings get published as
/// well; their bytes outside UTF-8 read as separators.
pub(crate) fn check(root: &Path) -> Result<Vec<String>, String> {
    let mut paths = Vec::new();
    files(root, &mut paths)?;
    paths.sort();
    let mut found = Vec::new();
    for path in paths {
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let rel = rel.to_string_lossy().replace('\\', "/");
        if rel == SELF {
            continue;
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("{rel}: {e}"))?;
        let text = String::from_utf8_lossy(&bytes);
        for (n, line) in text.lines().enumerate() {
            for hit in line_hits(line) {
                found.push(format!("{rel}:{}: {hit}", n + 1));
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::{check, line_hits};

    #[test]
    fn binary_files_are_scanned() {
        let root = std::env::temp_dir().join(format!("eix-public-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("data.reg"), b"\xFB\x3E\x03\x00lab\xFF\x02").unwrap();
        let found = check(&root);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(found, Ok(vec!["data.reg:1: the word \"lab\"".to_owned()]));
    }

    #[test]
    fn each_rule_fires() {
        assert_eq!(line_hits("seen in the Lab run").len(), 1);
        assert_eq!(line_hits("// the cave's operand").len(), 1);
        assert_eq!(line_hits("found by Disassembly").len(), 1);
        assert_eq!(line_hits("[V-dis] read").len(), 1);
        assert_eq!(line_hits("block 0x1005095e").len(), 1);
        assert_eq!(
            line_hits("Feature::new(\"a\", \"b\", 0x1004_F6E6)").len(),
            1
        );
    }

    #[test]
    fn correct_text_passes() {
        for line in [
            "label, labels, collaborate",
            "nightvisionincaves and night_vision_in_caves",
            "const NO_SPELLS: u32 = 0x1000_0000;",
            "game.exe at 0x6EC226, a value of 0x7FFFFFFF",
            "assemble and disassemble an item",
            "a reference/pointer pair",
            "self.as_int().map(|v| vec![v])",
        ] {
            assert!(line_hits(line).is_empty(), "{line}: {:?}", line_hits(line));
        }
    }
}
