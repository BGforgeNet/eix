//! `cargo xtask dist`: the release package. Builds the DLLs, then writes to `<target>/dist/`:
//! `<name>.zip` (a folder `<name>` with the two DLLs, spelladdon.ini, eix.ini, README.md, LICENSE, CHANGELOG.md,
//! THIRD-PARTY-NOTICES.txt and the `docs` folder, less the site's own files) and `notes.md` (CHANGELOG.md's section
//! for the version, for the release page).
//!
//! Runs where the gate runs (the build image has cargo-about and zip). With `EIX_TAG` set, the tag must be
//! `v<version>`, so a release never ships a version its tag does not name, and the name is `eix-<version>`; without
//! it, the name is `eix-<short commit hash>`.

use std::path::Path;
use std::process::Command;

use crate::{WINDOWS, release_build, rev_parse, root, run, say, target_dir, tool};

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
    // A build between releases is named for its commit: the version still names the last release.
    let name = match std::env::var_os(TAG_VAR) {
        Some(tag) => {
            let tag = tag.to_string_lossy().into_owned();
            if tag != format!("v{VERSION}") {
                return Err(format!(
                    "tag {tag} does not name version {VERSION} (Cargo.toml): expected v{VERSION}"
                ));
            }
            format!("eix-{VERSION}")
        }
        None => format!("eix-{}", rev_parse(&root(), &["--short", "HEAD"])?),
    };
    let notes = release_notes(&read(&root().join("CHANGELOG.md"))?, VERSION)?;
    release_build().run()?;

    let out = target_dir().join("dist");
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

/// The body of `## <version>` in the changelog, the lines up to the next `## ` heading, with its links made
/// absolute ([`tagged_links`]).
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
    Ok(tagged_links(&body, version) + "\n")
}

/// `text` with each relative link target, `](<path>)`, pointed at that file as tagged `v<version>`: a release page
/// shows a relative target as written, so the browser resolves it against the page's own URL. A target with a scheme
/// and an anchor alone stay.
fn tagged_links(text: &str, version: &str) -> String {
    let base = format!("{}/blob/v{version}/", env!("CARGO_PKG_REPOSITORY"));
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("](") {
        let (head, tail) = rest.split_at(at + 2);
        out.push_str(head);
        let target = tail.split(')').next().unwrap_or_default();
        if !target.starts_with('#') && !target.contains(':') {
            out.push_str(&base);
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Copies `from` to `to` without its execute bits: nothing in the package runs on the system that unpacks it, and the
/// linker marks the DLLs executable.
fn copy(from: &Path, to: &Path) -> Result<(), String> {
    let fail = |e: std::io::Error| format!("{} -> {}: {e}", from.display(), to.display());
    std::fs::copy(from, to).map_err(fail)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(to).map_err(fail)?.permissions();
        permissions.set_mode(permissions.mode() & !0o111);
        std::fs::set_permissions(to, permissions).map_err(fail)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::release_notes;

    #[cfg(unix)]
    #[test]
    fn a_copy_drops_the_execute_bits() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("eix-dist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (from, to) = (dir.join("from.dll"), dir.join("to.dll"));
        std::fs::write(&from, b"MZ").unwrap();
        std::fs::set_permissions(&from, std::fs::Permissions::from_mode(0o755)).unwrap();
        let copied = super::copy(&from, &to);
        let mode = std::fs::metadata(&to).map(|m| m.permissions().mode() & 0o777);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(copied, Ok(()));
        assert_eq!(mode.unwrap(), 0o644);
    }

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
    fn relative_links_point_at_the_tagged_files() {
        let log = "## 1.2.0\n\n- [Fixes](docs/engine.md#fixes), [Fixes](#fixes), [site](https://example.org/a).\n";
        assert_eq!(
            release_notes(log, "1.2.0").unwrap(),
            "- [Fixes](https://github.com/BGforgeNet/eix/blob/v1.2.0/docs/engine.md#fixes), [Fixes](#fixes), \
             [site](https://example.org/a).\n"
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
