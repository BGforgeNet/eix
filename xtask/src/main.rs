//! Project tasks: `cargo xtask check` runs the gate; `cargo xtask dist` builds the release package;
//! `cargo xtask container [check|dist]` runs either in the build image (check by default); `cargo xtask public` runs
//! the gate's public-text check alone; `cargo xtask docs` writes the generated reference pages.

mod dist;
mod docs;
mod links;
mod public;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const WINDOWS: &str = "i686-pc-windows-gnu";
const IMAGE: &str = "eix-build";

/// Writes one line to stderr; a closed pipe drops the line and the task goes on.
fn say(line: &str) {
    let mut err = std::io::stderr().lock();
    match writeln!(err, "xtask: {line}") {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
        // Nothing else can report it: stderr is the reporting channel.
        Err(_) => std::process::exit(2),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["check"] => check(),
        ["dist"] => dist::dist(),
        ["container"] | ["container", "check"] => container("check"),
        ["container", "dist"] => container("dist"),
        ["public"] => public_text(),
        ["docs"] => docs::write(),
        _ => Err("usage: cargo xtask <check|dist|container [check|dist]|public|docs>".to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            say(&e);
            ExitCode::FAILURE
        }
    }
}

/// One gate step: a name, the program (cargo unless named), its arguments, extra environment.
struct Step {
    name: &'static str,
    program: Option<&'static str>,
    args: Vec<String>,
    env: Vec<(&'static str, &'static str)>,
}

/// A cargo step.
fn step(name: &'static str, args: &[&str]) -> Step {
    Step {
        name,
        program: None,
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        env: Vec::new(),
    }
}

/// A step of another program, found on `PATH` (the build image installs each).
fn tool(name: &'static str, program: &'static str, args: &[&str]) -> Step {
    Step {
        program: Some(program),
        ..step(name, args)
    }
}

impl Step {
    fn run(self) -> Result<(), String> {
        let program = match self.program {
            Some(p) => p.to_string(),
            None => std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()),
        };
        let shown = self.program.unwrap_or("cargo");
        say(&format!("{}: {shown} {}", self.name, self.args.join(" ")));
        let mut cmd = Command::new(program);
        cmd.args(&self.args).envs(self.env).current_dir(root());
        run(&mut cmd, self.name)
    }
}

/// Fails on any text in the tree that does not belong in the published tree (`public.rs`).
fn public_text() -> Result<(), String> {
    let hits = public::check(&root())?;
    for hit in &hits {
        say(hit);
    }
    match hits.len() {
        0 => Ok(()),
        n => Err(format!("public text: {n} hits")),
    }
}

/// The arguments that select the Windows crates.
const WINDOWS_CRATES: [&str; 5] = ["--workspace", "--exclude", "xtask", "--target", WINDOWS];

/// Builds the DLLs as they ship. Clippy and the tests never link a cdylib, so linker warnings surface only here.
fn release_build() -> Step {
    Step {
        env: vec![("RUSTFLAGS", "-D warnings")],
        ..step(
            "build (windows, release)",
            &[&["build", "--locked", "--release"][..], &WINDOWS_CRATES].concat(),
        )
    }
}

fn check() -> Result<(), String> {
    // The host steps cover only the host crates: engine, loader, addon and lua build for 32-bit Windows only
    // (thiscall, stdcall and the trampolines exist nowhere else).
    let host: &[&str] = &[
        "-p",
        "eix-core",
        "-p",
        "eix-docs",
        "-p",
        "eix-sites",
        "-p",
        "xtask",
    ];
    let windows: &[&str] = &WINDOWS_CRATES;
    let clippy: &[&str] = &["clippy", "--locked", "--all-targets"];
    let deny_warnings: &[&str] = &["--", "-D", "warnings"];
    let steps = [
        step("format", &["fmt", "--all", "--check"]),
        step("clippy (host)", &[clippy, host, deny_warnings].concat()),
        // Every feature, so the `test-panic` build of addon.dll compiles too.
        step(
            "clippy (windows)",
            &[clippy, windows, &["--all-features"], deny_warnings].concat(),
        ),
        step("tests (host)", &[&["test", "--locked"][..], host].concat()),
        // Runs the test binaries under Wine (the target's runner in .cargo/config.toml).
        step(
            "tests (windows)",
            &[&["test", "--locked"][..], windows].concat(),
        ),
        release_build(),
        Step {
            env: vec![("RUSTDOCFLAGS", "-D warnings")],
            ..step(
                "docs",
                &[&["doc", "--locked", "--no-deps"][..], windows].concat(),
            )
        },
        step("dependencies (deny)", &["deny", "check"]),
        step("unused dependencies (shear)", &["shear"]),
        tool("workflows (actionlint)", "actionlint", &[]),
        // Offline: the checks that ask GitHub's API are left out, so the gate needs no token and no network.
        tool(
            "workflows (zizmor)",
            "zizmor",
            &["--offline", ".github/workflows"],
        ),
    ];
    for s in steps {
        s.run()?;
    }
    say("generated pages");
    docs::check()?;
    say("book links");
    links::check()?;
    // mdBook builds a page whose link points nowhere, so the links are read above; the build still fails on a page
    // that does not parse or a summary entry without a file. It writes into `target/book`.
    tool("site (mdbook)", "mdbook", &["build"]).run()?;
    say("public text");
    public_text()?;
    say("check passed");
    Ok(())
}

/// The variable naming the build image's inputs, set by CI (`ci.yml`) to the key it caches the image under.
const IMAGE_KEY_VAR: &str = "EIX_IMAGE_KEY";

/// The image label that records the key the image was built for.
const IMAGE_KEY_LABEL: &str = "eix.image-key";

/// Runs `cargo xtask <task>` in the build image, which this builds first. With `EIX_IMAGE_KEY` set, an image already
/// labelled with that key is used as it is: podman's layer cache does not reuse an image loaded from an archive, so
/// CI's cached image would otherwise be rebuilt in full.
fn container(task: &str) -> Result<(), String> {
    let root = root();
    let key = std::env::var(IMAGE_KEY_VAR).ok().filter(|k| !k.is_empty());
    match &key {
        Some(key) if image_key().as_deref() == Some(key.as_str()) => {
            say(&format!(
                "image build: skipped, {IMAGE} is built for key {key}"
            ));
        }
        _ => {
            let mut build = Command::new("podman");
            build.args(["build", "-q", "-t", IMAGE, "-f", "build/Containerfile"]);
            if let Some(key) = &key {
                build.args(["--label", &format!("{IMAGE_KEY_LABEL}={key}")]);
            }
            run(build.arg(".").current_dir(&root), "image build")?;
        }
    }
    let src = format!("{}:/src:Z", root.display());
    let mut podman = Command::new("podman");
    podman.args(["run", "--rm", "-v", &src]);
    if let Some(mount) = git_mount(&root, &git_common_dir(&root)?) {
        podman.args(["-v", &mount]);
    }
    run(
        podman.args([
            "-v",
            "eix-cargo:/root/.cargo/registry",
            "-w",
            "/src",
            // Keeps the container's build apart from the host's.
            "-e",
            "CARGO_TARGET_DIR=/src/target/container",
            // The release tag `dist` checks the version against, when the caller sets it.
            "-e",
            dist::TAG_VAR,
            IMAGE,
            "cargo",
            "xtask",
            task,
        ]),
        &format!("container {task}"),
    )
}

/// The repository's shared git directory, as an absolute path.
fn git_common_dir(root: &Path) -> Result<PathBuf, String> {
    let out = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("git rev-parse could not start: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git rev-parse failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}

/// The read-only mount a linked worktree needs: its `.git` file points at the shared git directory by absolute path,
/// outside the tree mounted at /src, and the public-text check runs `git ls-files`. None for a plain clone.
fn git_mount(root: &Path, common: &Path) -> Option<String> {
    (!common.starts_with(root)).then(|| format!("{0}:{0}:ro", common.display()))
}

/// The key label of the build image; none when there is no image or it has no label.
fn image_key() -> Option<String> {
    let format = format!("{{{{index .Labels \"{IMAGE_KEY_LABEL}\"}}}}");
    let out = Command::new("podman")
        .args(["image", "inspect", "--format", &format, IMAGE])
        .output()
        .ok()?;
    let key = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !key.is_empty() && key != "<no value>").then_some(key)
}

fn run(cmd: &mut Command, name: &str) -> Result<(), String> {
    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("{name} failed: {s}")),
        Err(e) => Err(format!("{name} could not start: {e}")),
    }
}

fn root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    // xtask/ sits directly under the workspace root, so a parent always exists.
    manifest.parent().unwrap_or(manifest).to_path_buf()
}

/// Where cargo puts its output: `CARGO_TARGET_DIR`, else `target` under the root.
fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root().join("target"), PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::git_mount;
    use std::path::Path;

    #[test]
    fn a_plain_clone_needs_no_git_mount() {
        assert_eq!(
            git_mount(Path::new("/w/eix"), Path::new("/w/eix/.git")),
            None
        );
    }

    #[test]
    fn a_linked_worktree_mounts_the_shared_git_directory_read_only() {
        assert_eq!(
            git_mount(Path::new("/w/eix-wt/a"), Path::new("/w/eix/.git")).as_deref(),
            Some("/w/eix/.git:/w/eix/.git:ro")
        );
    }
}
