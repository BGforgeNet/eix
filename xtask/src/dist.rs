//! `cargo xtask dist`: the release package. Builds the DLLs, then writes to `<target>/dist/`:
//! `eix-<version>.zip` (a folder `eix-<version>` with the two DLLs, spelladdon.ini, eix.ini, README.md, LICENSE,
//! CHANGELOG.md, THIRD-PARTY-NOTICES.txt and the `docs` folder, less the site's own files) and `notes.md`
//! (CHANGELOG.md's section for the version, for the release page).
//!
//! Runs where the gate runs (the build image has cargo-about and zip). With `EIX_TAG` set, the tag must be
//! `v<version>`, so a release never ships a version its tag does not name.

use std::path::Path;
use std::process::Command;

use crate::{WINDOWS, release_build, root, run, say, target_dir, tool};

/// The environment variable holding the release tag.
pub(crate) const TAG_VAR: &str = "EIX_TAG";

/// The version every crate of the workspace carries.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The files of the package that come from the repository as they are.
const DOCS: [&str; 3] = ["README.md", "LICENSE", "CHANGELOG.md"];

/// The files of `docs/` that only the site uses.
const SITE_ONLY: [&str; 3] = ["SUMMARY.md", "index.md", "eix-badge.css"];

/// The settings files, from `dist/`.
const INIS: [&str; 2] = ["spelladdon.ini", "eix.ini"];

/// The DLLs, from the release build.
const DLLS: [&str; 2] = ["dinput.dll", "addon.dll"];

pub(crate) fn dist() -> Result<(), String> {
    if let Some(tag) = std::env::var_os(TAG_VAR) {
        let tag = tag.to_string_lossy().into_owned();
        if tag != format!("v{VERSION}") {
            return Err(format!(
                "tag {tag} does not name version {VERSION} (Cargo.toml): expected v{VERSION}"
            ));
        }
    }
    let notes = release_notes(&read(&root().join("CHANGELOG.md"))?, VERSION)?;
    release_build().run()?;

    let out = target_dir().join("dist");
    let name = format!("eix-{VERSION}");
    let stage = out.join(&name);
    if out.exists() {
        std::fs::remove_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    std::fs::create_dir_all(&stage).map_err(|e| format!("{}: {e}", stage.display()))?;

    let release = target_dir().join(WINDOWS).join("release");
    for dll in DLLS {
        copy(&release.join(dll), &stage.join(dll))?;
    }
    for doc in DOCS {
        copy(&root().join(doc), &stage.join(doc))?;
    }
    // The README links into `docs/`, so the folder ships, less the site's own files: its contents, its front page
    // (which includes README sections, unreadable outside the site) and its stylesheet.
    let docs = stage.join("docs");
    std::fs::create_dir_all(&docs).map_err(|e| format!("{}: {e}", docs.display()))?;
    let entries = std::fs::read_dir(root().join("docs")).map_err(|e| format!("docs: {e}"))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("docs: {e}"))?.path();
        if let Some(name) = path.file_name()
            && !SITE_ONLY.iter().any(|s| name == *s)
        {
            copy(&path, &docs.join(name))?;
        }
    }
    for ini in INIS {
        copy(&root().join("dist").join(ini), &stage.join(ini))?;
    }
    let notices = stage.join("THIRD-PARTY-NOTICES.txt").display().to_string();
    tool(
        "licence notices (cargo-about)",
        "cargo-about",
        &[
            "generate",
            "--fail",
            "--locked",
            "-c",
            "dist/about.toml",
            "-m",
            "crates/addon/Cargo.toml",
            "-o",
            &notices,
            "dist/notices.hbs",
        ],
    )
    .run()?;

    let zip = format!("{name}.zip");
    // -X leaves out extra file attributes, -r takes the folder whole; run from `out` so the paths start at the folder.
    run(
        Command::new("zip")
            .args(["-q", "-X", "-r", &zip, &name])
            .current_dir(&out),
        "zip",
    )?;
    std::fs::write(out.join("notes.md"), notes).map_err(|e| format!("notes.md: {e}"))?;
    say(&format!("package: {}", out.join(&zip).display()));
    Ok(())
}

/// The body of `## <version>` in the changelog: the lines up to the next `## ` heading.
pub(crate) fn release_notes(changelog: &str, version: &str) -> Result<String, String> {
    let heading = format!("## {version}");
    let mut lines = changelog.lines().skip_while(|l| l.trim_end() != heading);
    if lines.next().is_none() {
        return Err(format!("CHANGELOG.md has no section {heading:?}"));
    }
    let body: Vec<&str> = lines.take_while(|l| !l.starts_with("## ")).collect();
    let body = body.join("\n").trim().to_string();
    if body.is_empty() {
        return Err(format!("CHANGELOG.md's section {heading:?} is empty"));
    }
    Ok(body + "\n")
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))
}

#[cfg(test)]
mod tests {
    use super::release_notes;

    const LOG: &str = "# Changelog\n\n## 0.2.0\n\n- New.\n\n## 0.1.0\n\nFirst release.\n\n- Old.\n";

    #[test]
    fn the_notes_are_the_versions_section() {
        assert_eq!(release_notes(LOG, "0.2.0").unwrap(), "- New.\n");
        assert_eq!(
            release_notes(LOG, "0.1.0").unwrap(),
            "First release.\n\n- Old.\n"
        );
    }

    #[test]
    fn a_missing_or_empty_section_is_an_error() {
        assert!(release_notes(LOG, "0.3.0").is_err());
        assert!(release_notes("## 0.1.0\n\n## 0.0.1\n- x\n", "0.1.0").is_err());
    }

    #[test]
    fn the_real_changelog_has_this_versions_notes() {
        let log = std::fs::read_to_string(crate::root().join("CHANGELOG.md")).unwrap();
        assert!(release_notes(&log, super::VERSION).is_ok());
    }
}
