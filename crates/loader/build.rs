//! Linker flags for `dinput.dll`.

fn main() {
    // The exports are stdcall: Rust names them `_Name`, the objects define `_Name@N`. The fixup links the two, which
    // is what an undecorated export (as game.exe imports it) needs; enabling it explicitly keeps the linker quiet.
    println!("cargo::rustc-cdylib-link-arg=-Wl,--enable-stdcall-fixup");
}
