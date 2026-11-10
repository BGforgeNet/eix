# Changelog

## 0.1.0

First release.

### Added

- `dinput.dll`: the addon loader, a drop-in for EI starter's Addon Loader 1.1.0.0.
- `addon.dll`: all 87 patch groups of addon.dll 0.12.4.2, the Lua host (LuaJIT 2.1) with its script functions and
  callbacks, the command line (`game.exe addon <command>`), plugins, addon.log and AutoRecover. A mod's Lua script
  defines only the callbacks it changes; the game's own code runs for the rest.
- Built-in SpellAddon: every feature key of SpellAddon 20.6's spelladdon.ini, read from the mod folder or the game
  folder. A separate `SpellAddon*.asi` is not loaded. The package's `spelladdon.ini` has every feature off apart from
  bug fixes.
- Mods read values from INI files in the mod folder: `GetIniValue` in Lua, `GSGetIniNumber` and `GSGetIniString` in
  map scripts.
- [Fixes](docs/engine.md#fixes): 30 fixes of game.exe crashes, hangs, leaks and slow paths, behind eix.ini's
  `[EngineFixes] Enabled`.
- [Game options](docs/engine.md#game-options), each with its own setting in `[EngineFixes]`:
  - On by default: `KeepWindowOnFocusLoss`, `SmoothCamera`, `PlayerListOnJoin`, `SkipDiscCheck` (the starter's
    game.exe starts without the game disc), `UnitPanelSpells` (the unit panel's values page lists the spells of a
    unit outside the party, each with its icon and effect, and gets a scroll bar, the mouse wheel over the page
    scrolling it, when it outgrows the panel) and `SpellsNeedAcceptedSkills` (the camp's spell constructor counts
    only accepted skill purchases).
  - Off by default: `KeepRunningOnFocusLoss` (a windowed game keeps running when it loses the focus),
    `ClipCursorToWindow` (the cursor stays in the window) and `UnitShadowResolution` (128 or 256 for sharper unit
    shadows).
- [eix.ini](docs/eix-ini.md), eix's own settings in the mod's folder: `[EngineFixes]`, the switches above, and
  [`[AutoPause]`](docs/engine.md#autopause), off by default: in single player the game pauses when a party member's health drops below `Health`
  hit points or `HealthPercent` of its maximum, when an enemy comes into view (`EnemySighted`), turns on the party
  (`EnemyAggro`) or dies in view (`EnemyDead`).
- [Encumbrance](docs/engine.md#encumbrance), eix.ini's `[Encumbrance]`, off by default: the load a hero carries
  slows running (20% per maximum load), and above the maximum load walking too, and running costs as much more
  mana as it is slower; no load stops a hero from running, and wounded legs still slow every movement.
- [FLAC music](docs/engine.md#flac-music): a track plays from `Stream\<name>.flac`, the mod's or the game's, when
  there is one; eix.ini's `[Music] Flac=0` turns it off.
- `[MOD] ShowTutorialDefault` in config.reg: a mod sets whether tutorials show until the player chooses.
- Bug reports: addon.log records the eix build and commit, the system, the game.exe build and the loaded modules; a
  crash adds a record naming the patch group, fix or feature behind each address, and its box points to the issue
  tracker with the files to attach. Fatal Lua errors go to addon.log too.
- Differences from the originals: [addon and loader](docs/addon.md), [SpellAddon options](docs/spelladdon.md).
- References: every game script function with its signature, eix's marked ([scripting](docs/scripting.md)), and the
  Lua API ([Lua reference](docs/lua-reference.md)); `docs/eix.lua` and a ready `docs/.luarc.json` give Lua editors
  completion and hover through the Lua language server ([editor setup](docs/lua.md#editor-setup)).
- Quirk fixes: fractional and whole-number INI values, common key aliases, skill and stealing caps, Lua alive flags
  and intelligence change marks; warnings for duplicate keys, ignored aliases and truncated switches.
- More fixes of the originals: SpellAddon features that change the same code both apply, or the one with no effect
  left is skipped with a warning; `FloatToStr` keeps a whole number's zeros and `Round` is exact; negative integers
  and signs in ai.reg and spelladdon.ini values keep their sign; the console pastes and repeats lines without a
  stray selection; the chat log escapes messages and shows colours; equal music pause limits no longer crash; every
  renewed spell effect goes; an item with no maximum condition has a repair price; Ghostliness lowers loudness too
  and visibility stops at 0; SpellAddon's hero tests all take IDs from one billion up; byte parameters outside
  0..255 warn; anonymous objects of one mob file each get their own ID; `ChargeMethod=2` gives every hero the whole
  experience; with Lua attack scripts players hurt each other only with `enablekillsinmulti`, which keeps god mode;
  `normalkey` defaults to the game's 55; a cast of no time plays at normal speed.
