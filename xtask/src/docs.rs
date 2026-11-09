//! `cargo xtask docs`: writes the generated reference pages under `docs/`; the gate fails when a page on disk differs
//! from a fresh generation.

use std::path::PathBuf;

use crate::root;

fn path(file: &str) -> PathBuf {
    root().join("docs").join(file)
}

/// Writes every generated page.
pub(crate) fn write() -> Result<(), String> {
    for page in eix_docs::pages().map_err(|e| e.to_string())? {
        let target = path(page.file);
        std::fs::write(&target, page.text).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    Ok(())
}

/// Fails when a generated page is missing or differs from a fresh generation.
pub(crate) fn check() -> Result<(), String> {
    let mut stale = Vec::new();
    for page in eix_docs::pages().map_err(|e| e.to_string())? {
        let target = path(page.file);
        match std::fs::read_to_string(&target) {
            Ok(text) if text == page.text => {}
            Ok(_) => stale.push(format!("docs/{} is stale", page.file)),
            Err(e) => stale.push(format!("docs/{}: {e}", page.file)),
        }
    }
    if stale.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{}; run `cargo xtask docs` and commit the result",
            stale.join("; ")
        ))
    }
}
