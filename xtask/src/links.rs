//! The internal links of the book. mdBook builds a page whose link points nowhere, so the gate reads every link of
//! the pages in `docs/`: a relative link must name a file inside `docs/`, and a fragment must name a heading of the
//! page it points at.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use eix_docs::anchor;

use crate::root;

/// The links of one page: the target of each `[text](target)` outside code.
fn links(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        // A span of inline code holds no link.
        let outside: String = line
            .split('`')
            .enumerate()
            .filter(|(i, _)| i % 2 == 0)
            .map(|(_, part)| part)
            .collect::<Vec<_>>()
            .join(" ");
        let mut rest = outside.as_str();
        while let Some(at) = rest.find("](") {
            rest = &rest[at + 2..];
            if let Some(end) = rest.find(')') {
                found.push(rest[..end].to_owned());
                rest = &rest[end..];
            }
        }
    }
    found
}

/// The anchors of a page's headings.
fn anchors(text: &str) -> HashSet<String> {
    let mut fenced = false;
    let mut set = HashSet::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        } else if !fenced && line.starts_with('#') {
            let heading = line.trim_start_matches('#').replace(['`', '*'], "");
            set.insert(anchor(heading.trim()));
        }
    }
    set
}

fn pages(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut pages = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        if path.extension().is_some_and(|e| e == "md") {
            pages.push(path);
        }
    }
    pages.sort();
    Ok(pages)
}

/// Fails on a link that points outside `docs/`, at a missing file, or at a heading the page does not have.
pub(crate) fn check() -> Result<(), String> {
    let dir = root().join("docs");
    let mut broken = Vec::new();
    let summary = dir.join("SUMMARY.md");
    let summary_text =
        std::fs::read_to_string(&summary).map_err(|e| format!("{}: {e}", summary.display()))?;
    let listed: HashSet<String> = links(&summary_text).into_iter().collect();
    for page in pages(&dir)? {
        let text =
            std::fs::read_to_string(&page).map_err(|e| format!("{}: {e}", page.display()))?;
        let name = page
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        // mdBook builds only the pages the summary names.
        if name != "SUMMARY.md" && !listed.contains(&name) {
            broken.push(format!("docs/{name} is not in SUMMARY.md"));
        }
        for link in links(&text) {
            if link.contains("://") || link.starts_with("mailto:") {
                continue;
            }
            let (file, fragment) = link
                .split_once('#')
                .map_or((link.as_str(), None), |(f, a)| (f, Some(a)));
            let target = if file.is_empty() {
                page.clone()
            } else {
                dir.join(file)
            };
            if file.contains("..") || !target.starts_with(&dir) {
                broken.push(format!("docs/{name}: {link} leaves the book"));
                continue;
            }
            let Ok(target_text) = std::fs::read_to_string(&target) else {
                broken.push(format!("docs/{name}: {link} names no file"));
                continue;
            };
            if let Some(fragment) = fragment
                && target.extension().is_some_and(|e| e == "md")
                && !anchors(&target_text).contains(fragment)
            {
                broken.push(format!("docs/{name}: {link} names no heading"));
            }
        }
    }
    if broken.is_empty() {
        Ok(())
    } else {
        Err(format!("broken links: {}", broken.join("; ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_links_outside_code_only() {
        let text = "See [a](a.md#x) and `[b](b.md)`.\n```\n[c](c.md)\n```\n[d](https://e.org)\n";
        assert_eq!(links(text), ["a.md#x", "https://e.org"]);
    }

    #[test]
    fn headings_make_anchors() {
        let set = anchors("# Top\n## HP - SpellAddon `hptrans`\n```\n# not a heading\n```\n");
        assert!(set.contains("top"));
        assert!(set.contains("hp---spelladdon-hptrans"));
        assert!(!set.contains("not-a-heading"));
    }

    #[test]
    fn the_docs_links_hold() {
        assert_eq!(check(), Ok(()));
    }
}
