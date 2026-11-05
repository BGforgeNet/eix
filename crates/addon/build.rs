//! Linker flags for `addon.dll`.

fn main() {
    // The loaders find Handler and Apply by ordinal as well as by name: the module definition pins 1 and 2, which
    // the linker's own order (by name) would swap.
    let def = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("addon.def");
    println!("cargo::rerun-if-changed={}", def.display());
    println!("cargo::rustc-cdylib-link-arg={}", def.display());
    // The exports are stdcall-free cdecl functions, but DllMain is stdcall: the fixup links `_DllMain@12`.
    println!("cargo::rustc-cdylib-link-arg=-Wl,--enable-stdcall-fixup");
    commit();
}

/// `EIX_COMMIT`: the commit the build comes from, for addon.log's first line, so a report names the exact build;
/// "unknown" outside a git checkout (a source archive).
fn commit() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let git = root.join(".git");
    // A new commit or a branch switch moves HEAD or the ref it names.
    for path in ["HEAD", "refs/heads", "packed-refs"] {
        println!("cargo::rerun-if-changed={}", git.join(path).display());
    }
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .current_dir(&root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo::rustc-env=EIX_COMMIT={commit}");
}
