# Development

## Build

Install podman, then:

```sh
cargo xtask container
```

- Runs `cargo xtask check` in the image from `build/Containerfile`: format, clippy (host and Windows), tests (Windows
  ones under Wine), release build, docs, dependency checks, workflow checks, and the public-text check
  (`cargo xtask public`).
- Output: `target/container/i686-pc-windows-gnu/release/{dinput,addon}.dll`.
- Release package: `cargo xtask container dist` writes `target/container/dist/eix-<version>.zip` (the DLLs,
  `dist/spelladdon.ini`, the README, the licence, the changelog, the third-party notices and `docs/`) and the release
  notes.
- Without podman: the toolchain from `rust-toolchain.toml`, `i686-w64-mingw32-gcc` on `PATH`, Wine, and the tools
  `build/Containerfile` installs, then `cargo xtask check`.
- Tests that need the game's files are ignored by the gate. Run them with `EIX_GAME_DIR` set to the game folder as a
  Windows path (under Wine, `Z:\...`):
  `cargo test -p eix-addon --target i686-pc-windows-gnu -- --ignored`. They check every expected byte of the fixes
  and SpellAddon's features against `game.exe`, and the vertex lighting against `3dfpfpu.dll`.

## Continuous integration

- `.github/workflows/ci.yml`: the gate (`cargo xtask container`) on every push to `main` and every pull request.
- `.github/workflows/release.yml`: on a `v*` tag, the gate, the package, and a draft GitHub release.

## Release

1. Set the version in `Cargo.toml` and add its section to `CHANGELOG.md`.
2. Push a tag `v<version>`. The release workflow runs the gate, builds the package, and drafts a GitHub release with
   it and the changelog section.
3. Check the draft and publish it.

Keep each release's package: an address inside addon.dll in a crash record is read against that exact file, which
keeps its symbols.

## Layout

| Path | Content |
|---|---|
| `crates/core` | platform-free logic: registry format, options, group conditions, spelladdon.ini keys |
| `crates/sites` | game.exe addresses, one named constant each, per exe build |
| `crates/engine` | game.exe access: memory, hooks, typed game structures, Win32 |
| `crates/lua` | the Lua host (LuaJIT) |
| `crates/spelladdon` | the built-in SpellAddon |
| `crates/loader` | `dinput.dll` |
| `crates/addon` | `addon.dll`: patch groups, command line, engine fixes |
| `xtask` | project tasks |
| `build` | the build image |
| `dist` | the release package's spelladdon.ini and licence-notice settings |
