# Development

## Build

Install podman, then:

```sh
cargo xtask container
```

- Runs `cargo xtask check` in the image from `build/Containerfile`: format, clippy (host and Windows), tests (Windows
  ones under Wine), release build, docs, dependency checks, workflow checks, and the public-text check
  (`cargo xtask public`).
- It also checks the generated reference pages (see [Documentation site](#documentation-site)) and builds the site.
- Output: `target/container/i686-pc-windows-gnu/release/{dinput,addon}.dll`.
- Release package: `cargo xtask container dist` writes `target/container/dist/eix-<version>.zip` (the DLLs,
  `dist/spelladdon.ini`, `dist/eix.ini`, the README, the licence, the changelog, the third-party notices and
  `docs/`) and the release notes.
- Without podman: the toolchain from `rust-toolchain.toml`, `i686-w64-mingw32-gcc` on `PATH`, Wine, and the tools
  `build/Containerfile` installs, then `cargo xtask check`.
- Tests that need the game's files are ignored by the gate. Run them with `EIX_GAME_DIR` set to the game folder as a
  Windows path (under Wine, `Z:\...`):
  `cargo test -p eix-addon --target i686-pc-windows-gnu -- --ignored`. They check the expected bytes of the fixes
  and SpellAddon's features against `game.exe`, and the vertex lighting against `3dfpfpu.dll`.

## Documentation site

The site is an [mdBook](https://rust-lang.github.io/mdBook/) book: `book.toml` at the repository root, `docs/` as its
source, `docs/SUMMARY.md` as its table of contents. Every page of `docs/` is listed there.

Two pages are generated, and each has one data file that holds every signature and description:

| Page | Data file |
|---|---|
| `docs/scripting.md` | `docs/scripting.yml`: the game's script functions and commands, with their source and eix's differences |
| `docs/lua-reference.md` | `docs/eix.lua`: the Lua API as LuaLS annotations; it ships as it is, for the Lua language server |

- Edit the data file, then run `cargo xtask docs` and commit both. Never edit a generated page: `cargo xtask check` fails
  when a page differs from a fresh generation.
- `cargo xtask check` also fails on a broken link between pages and on a page missing from `docs/SUMMARY.md`, then builds
  the book into `target/book`. The build image carries mdBook, pinned in `build/install-mdbook.sh`.
- Tests compare the data files with the code: every function and callback that addon.dll registers appears in
  `docs/scripting.yml` or `docs/eix.lua` with the same name. Run the test that compares the game's own
  entries with the keyword table of `game.exe` with `EIX_GAME_DIR` set, as for the other game tests:
  `cargo test -p eix-docs -- --ignored`.
- Build the book by hand with `mdbook build` and open `target/book/index.html`.
- Publishing: `.github/workflows/pages.yml` builds the book and deploys it to GitHub Pages after the CI gate succeeds on
  `main`. The
  repository's Pages source must be set to GitHub Actions once (Settings, Pages, Build and deployment).

## Continuous integration

- `.github/workflows/ci.yml`: the gate (`cargo xtask container`) on every push to `main` and every pull request. The
  build image is cached under a hash of `build/` and `rust-toolchain.toml`, and stored by pushes to `main` only;
  `EIX_IMAGE_KEY` makes xtask use a loaded image carrying that key instead of building one.
- `.github/workflows/pages.yml`: the documentation site, as above.
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
| `crates/docs` | the reference pages generated from `docs/scripting.yml` and `docs/eix.lua`, and the checks of those files |
| `crates/sites` | game.exe addresses, one named constant each, per exe build |
| `crates/engine` | game.exe access: memory, hooks, typed game structures, Win32 |
| `crates/lua` | the Lua host (LuaJIT) |
| `crates/spelladdon` | the built-in SpellAddon |
| `crates/loader` | `dinput.dll` |
| `crates/addon` | `addon.dll`: patch groups, command line, engine fixes |
| `xtask` | project tasks |
| `build` | the build image and the installers of its tools |
| `dist` | the release package's spelladdon.ini and licence-notice settings |
