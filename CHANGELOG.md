# Changelog

## 0.1.0

First release.

### Added

- `dinput.dll`: the addon loader, a drop-in for EI starter's Addon Loader 1.1.0.0.
- `addon.dll`: all 87 patch groups of addon.dll 0.12.4.2, the Lua host (LuaJIT 2.1) with its script functions and
  callbacks, the command line (`game.exe addon <command>`), plugins, addon.log and AutoRecover.
- Built-in SpellAddon: every feature key of SpellAddon 20.6's spelladdon.ini, read from the mod folder or the game
  folder. A separate `SpellAddon*.asi` is not loaded. The package's `spelladdon.ini` has every feature off apart from
  bug fixes.
- [Engine fixes](docs/engine-fixes.md): 34 fixes of game.exe crashes, hangs, leaks and slow paths, behind the
  `EngineFixes` setting; KeepWindowOnFocusLoss, SmoothCamera, PlayerListOnJoin and SkipDiscCheck (the starter's
  game.exe starts without the game disc) each have their own setting.
- `[MOD] ShowTutorialDefault` in config.reg: a mod sets whether tutorials show until the player chooses.
- Bug reports: addon.log records the eix build and commit, the system, the game.exe build and the loaded modules; a
  crash adds a record naming the patch group, fix or feature behind each address, and its box points to the issue
  tracker with the files to attach. Fatal Lua errors go to addon.log too.
- Differences from the originals: [addon and loader](docs/addon.md), [SpellAddon options](docs/spelladdon.md).
- Quirk fixes: fractional and whole-number INI values, common key aliases, skill and stealing caps, Lua alive flags
  and intelligence change marks; warnings for duplicate keys, ignored aliases and truncated switches.
- More fixes of the originals: SpellAddon features that change the same code both apply, or the one with no effect
  left is skipped with a warning; `FloatToStr` keeps a whole number's zeros and `Round` is exact; negative integers
  and signs in ai.reg and spelladdon.ini values keep their sign; the console pastes and repeats lines without a
  stray selection; the chat log escapes messages and shows colours; equal music pause limits no longer crash; every
  renewed spell effect goes; an item with no maximum condition has a repair price.
