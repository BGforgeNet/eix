# Addon and loader

`dinput.dll` loads `addon.dll`, which loads the selected mod, applies fixes and extensions, and writes `addon.log`.
For installation and supported game builds, see the [overview](index.md). SpellAddon has its own
[option reference](spelladdon.md), and mod scripts in Lua have the [Lua reference](lua.md); additional eix fixes
are under [engine fixes](engine-fixes.md).

The last column of each table gives eix's difference from addon.dll 0.12.4.2 and Addon Loader 1.1.0.0, with what
the original does in parentheses. `same` means the original's behavior;
`fixed:` marks a difference that only fixes a defect of the original. A name in
parentheses after a key is its patch group's name in `addon.log`.

## Configuration

Settings come from:

| Source | Contents |
|---|---|
| `HKCU\Software\Gipat.Ru\EI_Starter\Settings` | Launcher preferences; values below are DWORDs unless stated otherwise |
| Mod folder's `config.reg` | Mod paths, resource archives, menus and plugins |
| File named by `config.reg`'s `[CONFIG] ai.reg` | Gameplay settings, experience, skill caps and Lua |

The `.reg` files use the game's registry format, not Windows Registry Editor syntax. A missing key takes its
default, and so does a string where a number is expected or a single value where an array is expected. An integer
is accepted where a float is expected, and a float where an integer is expected (truncated). A negative integer
keeps its sign where a float is expected (the original reads -1 as 4294967296). Gameplay groups require a selected
mod and a nonempty `[CONFIG] ai.reg` path. Configure multiplayer participants consistently for gameplay changes.
Groups apply in a fixed order, and later groups can replace earlier changes. A failed byte check skips the whole
group and logs why.

### Launcher settings

| Value | Default | Effect | eix |
|---|---|---|---|
| `WideScreen` | 1 | Adapt the interface to a wide resolution; 0 disables. `VideoResolution` is a `WxH` string under `EI_Starter\EvilIslands\General Settings` | fixed: an out-of-range scaling value wraps instead of crashing; a failed curtain window or text size is skipped (the original writes through null) |
| `ObjectsDrawRadius` | 100 | Object draw radius; 100 leaves the game's value | same |
| `LandscapeDrawRadius` | 100 | Terrain draw radius; 100 leaves the game's value | same |
| `AntiCrash` (FixCreateItem, DisableExitOnCrashReport) | 1 | Report invalid item creation and fatal game errors without the game's forced exit; mod `[Addon] AntiCrash` overrides when present | same |
| `AutoRecover` | 1 | Back up the hosting player's party on a crash; 0 disables | fixed: with all 1000 backup names taken, nothing is backed up (the original writes into the drive's root) |
| `FullDump` | 0 | Nonzero includes the process's full memory in crash dumps | same |
| `LogLevel`, `ConsoleLogLevel` | 1, 3 | Minimum log/console severity: 0 debug, 1 information, 2 warning, 3 error, 4 off | same |
| `Rate` (SetDefaultRate) | 5000 | Only 2500 enables the group, which writes 2500, the value the game already holds; no visible effect on RU 1.06 | same |
| `Console AutoExec` (AutoExec) | Empty | `REG_MULTI_SZ` console commands, executed before `2utoexec.cfg` and the game's `autoexec.cfg` | same |
| `MQ client` (MQClient) | 0 | Nonzero prevents writing files received from the server | same |
| `MP Speed-control` (MPSpeedControl) | 0 | Nonzero allows speed keys and clock buttons in multiplayer | same |
| `MP console` (MPConsole) | 0 | Nonzero allows console commands in multiplayer | same |

The eix settings `EngineFixes`, `KeepWindowOnFocusLoss`, `SmoothCamera`, `PlayerListOnJoin` and `SkipDiscCheck`
default to 1; `UnitShadowResolution` defaults to 64, `KeepRunningOnFocusLoss` and `ClipCursorToWindow` to 0; see
[engine fixes](engine-fixes.md).

`Settings\AddonPath` is a string naming the selected mod folder. The loader searches for addon.dll using the
starter root's `AddonPath` (full DLL path), then `Settings\AddonPath\addon.dll`, then the starter root's
`Path\addon.dll`; the first existing file wins.

### Mod configuration: config.reg

Paths under `[CONFIG]` and plugin paths are relative to the mod folder. An empty `[CONFIG]` path other than
`lights<X>.ini` disables its override.

| Section and key | Default | Effect | eix |
|---|---|---|---|
| `[MOD] Title`, `Author` | Empty | Mod identification | same |
| `[MOD] Multi`, `Single` (MenuMultiDisable, MenuSingleDisable) | 1 each | 0 disables the respective main-menu entry | same |
| `[MOD] GameSaves` (SavesPath) | 1 | Nonzero puts saves and multiplayer heroes under the mod folder | same |
| `[MOD] PreventZone1Animation` | 0 | Nonzero skips the opening zone animation | same |
| `[MOD] ForceShowTutorial` | 0 | Nonzero prevents saved preferences and the options window from disabling tutorials | same |
| `[MOD] ShowTutorialDefault` | None | The "Show tutorial" option's value while none is stored; the player's later choice wins | eix's own ([engine fixes](engine-fixes.md)) |
| `[MOD] AddonVersion` | Empty | Required addon version as text. The game stops with a message when it is not a dotted version of at most four numbers, or names a version newer than 0.12.4.2 | same |
| `[CONFIG] ai.reg` (ConfigAiReg) | Empty | Replace the game's AI settings and enable the gameplay groups | fixed: a path of 260 bytes or more is refused and the game's own ai.reg loads (the original ends the process) |
| `[CONFIG] movie.ini`, `tutorial.ini` (ConfigMovieIni, ConfigTutorialIni) | Empty | Replace the respective configuration | fixed: a path of 260 bytes or more fails the group (the original ends the process) |
| `[CONFIG] music.reg`, `streamsn.reg` (ConfigMusicReg, ConfigStreamSNReg) | Empty | Replace music and streamed-sound configuration | fixed: a path of 260 bytes or more fails the group (the original ends the process) |
| `[CONFIG] lights<X>.ini` (ConfigLightsIni) | None | Replace a matching lights file; use lowercase key names. The key's presence enables it: an empty value names the mod folder itself | same |
| `[DEV] GameDebugFile` (GameExeDebug) | Empty | Write game debug messages to this file, relative to the game working folder | same |
| `[DEV] ChatHtmlFile` (ChatLog) | Empty | Append server chat to this HTML file, relative to the game working folder | fixed: names and messages are escaped, and the speakers' colours show (the original writes them raw, and its colour classes are not valid CSS) |
| `[DEV] Plugin` | Empty | Load a plugin DLL, a filename pattern, or a folder's `*.plg` files; empty or missing loads the mod folder's `*.plg` files | plugins are called in load order (the original: by module handle, which the system chooses) |
| `[MPZone] Gipath`, `Ingos`, `Suslanger` (MpZone) | Missing | Replace multiplayer zone lists; provide all three as strings or string arrays; duplicate names are rejected ignoring case | same |
| `[RES] <archive>` (ResOverride) | None | String or string array of replacement archive paths, in file order; a path starting with `-` suppresses the game's archive | fixed: archive paths of 260 bytes and more work (the original ends the process); an archive that fails to open is left out and logged |

`[RES]` replacement keys: `aiinfo.res`, `database.res`, `databaselmp.res`, `gfx.res`, `materials.res`, `outro.res`,
`redress.res`, `sfx.res`, `speech.res`, `texts.res`, `textslmp.res`, `textures.res`.
`figures.res` and `menus.res` also add figure archives;
`textures.res`, `redress.res` and `menus.res` add texture archives.
The listed mod archives are searched before the game's archive, in the supplied order.

Loose files in the mod folder override matching game files:

| Mod file | Effect | eix |
|---|---|---|
| `Maps` folder (Maps) | Maps | fixed: no length limit (the original overruns its buffers at 260 bytes and more) |
| `Movies` folder (MoviesPatch) | Movies | fixed: movie paths of 260 bytes and more work (the original ends the process) |
| `Camera` folder (CameraPatch) | Camera paths | fixed: camera paths of 260 bytes and more work (the original overruns its buffer) |
| `Stream` folder (StreamPatch) | Streamed sound | fixed: stream paths of 260 bytes and more work (the original ends the process) |
| `binkw32.dll` (BinkPatch) | Replace the game's movie decoder | fixed: a binkw32.dll without `BinkSetVolume`: the volume call does nothing, with a warning (the original crashes) |

### Gameplay: ai.reg

Unless stated otherwise, these `[Addon]` keys default to 0: 0 leaves the option off; any nonzero integer enables it.

| Key | Effect | eix |
|---|---|---|
| `ShowAllQuests` (MPShowAllQuests) | Offer every multiplayer zone separately instead of grouping quests | same |
| `DisableRunaway` | Keep an action when its target moves out of reach | same |
| `ArchersDefence` (EnableArchersDefence) | Retain defence while wielding a bow or crossbow | same |
| `DistanceModifier` (DisableDistanceModifier) | Remove the distance penalty from attack rolls | same |
| `WeightBonus` (DisableWeightBonus) | Remove the weapon-weight bonus from attack rolls | same |
| `MagicRandom` (RandomMagicDamage) | Float, default -1 (off); nonnegative values scale spell damage by `1 + MagicRandom * u`, with random `u` in `[-1, 1)` | same |
| `AllowMagicNegative` (NegativeMagicEffect) | Allow negative spell effects; choose the strongest overlapping effect by magnitude | same |
| `DeagrPlayerOnLoose` | Stop tracking an unseen enemy merely because it is on guard; clear a player's target when sight of another player's unit is lost | fixed: releases the target reference it clears (the original leaks it) |
| `AttackAnimSpeed` | Match attack and cast animation speed to their action duration | fixed: a cast of no time plays at normal speed, as an attack of no time does (the original's speed is then infinite) |
| `NewRunes` (NewRuneCodes) | Rune code `xa` adds its whole strength to cast time, with a minimum of one tick | same |
| `UsePrototypeActionsForMobAttack` | Use the natural attack interval for non-named units, scaled by their action rate | same |
| `UsePrototypeActionsForMobCast` | Use the natural attack interval for non-named units' casts, scaled by their action rate | same |
| `FastTeleport` | Reduce the extra teleport delay from 30 ticks to one | same |
| `DisableCrippledHL` | Disable crippling a hit location on a severing hit | same |
| `UnlockExperimentalSpells` | Allow Possession, Link and Charm through the spell-casting restriction | same |
| `LoadSpellAnimationFromSlotsField` | Select cast animations from the spell record's slots field | same |
| `ImproveInfoWindows` | Show spell damage to one decimal, cast time in seconds, and the unit rate percentage | fixed: info windows keep their text alive while it is drawn (the original draws freed memory); same text shown |
| `RemoveSpellConstructReqs` | Remove mana and skill requirements for spell construction and equipping | same |
| `SendSpellEffectValueToPlayers` | Add effect strengths to network state; enable on both host and clients because it changes the record format | same |
| `AddWeaponsFromDbToAllUnits` | Give all units their prototype's starting weapons and armor, not just heroes | same |
| `UnlockCorpses` | Allow searching a corpse already owned or searched by another unit | same |
| `AntiCrash` | Default: launcher value; override the two AntiCrash groups | same |
| `LogErrorsInFile`, `LogErrorsInConsole`, `LogErrorsInChat` | Send game-data error reports to `errors.log`, the console, or multiplayer chat | same |
| `EnableShowingScriptErrors` | Enable the script error display | same |
| `EnableLuaScripts` | Load the main Lua script and enable all nine [Lua](lua.md) groups | same |
| `LuaMainScriptPath` | String, default `main.lua`; path relative to the mod folder | same |
| `UsePerkLimit` | Enables and disables nothing | same |

| Section and key | Default | Effect | eix |
|---|---|---|---|
| `[AddonExp] ChargeMethod` (ExpCalc) | 0 | Share awarded experience among heroes: 1 multiplies by hero count, 2 gives each hero the whole amount, all other values divide by count | fixed: 2 gives the whole amount (the original adds the hero count to it) |
| `[AddonExp] MulFactor` (ExpCalc) | 1.0 | Multiply the resulting experience share | same |
| `[AddonExp] Limit` (ExpLimit) | -1 | Negative disables the cap; otherwise cap experience at this value times the hero's intelligence factor | fixed: no stray FPU pop on an empty stack |
| `[AddonPerk] Melee`, `Archery`, `Elemental`, `Sense`, `Astral` (PerkLimit) | 100 each | Training caps for the five skills; any value other than 100 enables the cap group; Backstab is a perk, not a sixth skill | same |
| `[DifficultyLevels] Absorption`, `Attack`, `Defence` | Empty | Float arrays of difficulty modifiers, one entry per level | same |

Lua experience modifiers replace the earlier experience-cap calculation. SpellAddon may replace overlapping gameplay
rules when it applies later; select one implementation of a rule when configuring a mod.

## Automatic groups

These need no individual switch. Mod-file overrides and the settings above supply the remaining group controls.
`MultipleLoadingMobs` also needs a selected mod and a nonempty `[CONFIG] ai.reg`. `SetEiPluginMode`,
`PlayerNumbers`, `AddNewScriptCommands` and `PersistentGlobalVars` run only when no incompatible EI_Plugin build
(`*.asi`) is in the game folder; a mod whose `AddonVersion` is 0.10 or newer then stops the game with a message.

| Group | Effect | eix |
|---|---|---|
| `AllowScriptArgsInLoops` | Make enclosing script locals available inside `For` and `ForIf` | same |
| `AllowScriptsInMainMenu` | Run the main-menu zone's script | same |
| `FixLeversActivate` | Correct signed lock/key level comparison and locks that require no skill | same |
| `FixSpellEffectsOverflow` | Limit accumulated effects, replace renewed effects, and expire effects on sleeping objects | fixed: a missing object in the sleep count-down is skipped (the original crashes); every renewed effect goes (the original keeps the second of two in a row) |
| `FixProgressNames` | Use the separate loading movies for the allods `gipat2` and `jigran` | same |
| `AddNewConsoleCommands`, `ImproveConsole` | Add commands below, clipboard shortcuts, and suppress consecutive duplicate history lines | fixed: `execs` without a file name does nothing (the original reads a stale argument); a paste over a selection puts the caret after the pasted text (the original puts it past the selection's end, less one); a repeated line leaves no selection (the original may leave one) |
| `NewStringEvents` | Add `gameover [title [text]]`; defaults are text keys `game_over` and `game_over_msg` | fixed: a failed allocation pushes no window (the original pushes null) |
| `FixSpellSelfCasting` | Correct offensive-spell classification and clicked-unit targeting | same |
| `FixProcessLogin` | Initialize the joining player's age field | same |
| `FixMoveObject` | Reject negative or invalid map coordinates instead of indexing an invalid sector | the refusal message goes to addon.log; coordinates round half to even, NaN prints `NaN` |
| `FixAddMapObject` | Report a missing unit prototype and use the fallback record | same |
| `FixPlaingBattleMusicFromAntimagic` | Stop Antimagic alone from triggering combat music | same |
| `FixRepairPrice` | Bound wear to 0..1 and include repair cost correctly when taking an item apart | fixed: an item with no maximum condition has no wear (the original's price is then not a number) |
| `FixRemoveMapObject` | Report an unknown object instead of crashing on removal | same |
| `ExtendedServerName` | Enlarge the multiplayer server-name field and wrap or shorten display text | fixed: a name of 90 bytes or more is cut to 89 (the original ends the process) |
| `NivalRegistry` | Use the starter's game-settings registry branch | same |
| `SetEiPluginMode` | Disable the EI plugin's duplicate script commands; eix supplies their replacements | same |
| `PlayerNumbers` | Give each new session player the next number after the highest existing player | same |
| `AddNewScriptCommands` | Add the script interface below and log script-load errors | same |
| `PersistentGlobalVars` | Preserve party variables with the case-sensitive `gvar.` prefix across network resets and saved-party reloads | same |
| `MultipleLoadingMobs` | When adding a mob to a populated zone, skip duplicate object IDs and assign unused IDs from 100 to anonymous objects | fixed: frees the entries it skips (the original leaks them); anonymous objects of one file each get their own ID (the original gives them one, so only the last appears) |

## Console and command line

The console supports Ctrl+A/C/X/V for selection, copy, cut and first-line paste.
Enter the game's cheat password before `addon`, `execs`, `listgvar` or `drawall`; `nogui` needs none.

| Console command | Effect |
|---|---|
| `addon info` | Show addon compatibility version and current mod |
| `addon debug log [level]`, `addon debug console [level]` | Read or set verbosity: 0 debug, 1 information, 2 warning, 3 error, 4 off |
| `execs <file>` | Load and run a script in the current zone; report a load error with its line |
| `listgvar` | List the zone script's globals, including strings, objects and groups |
| `drawall <value>` | 0 draws all objects; other values restore normal visibility |
| `nogui <mode>` | 0 shows panels and cursor, 1 hides panels, 2 hides panels and cursor |

Run these from the game folder (CommandLine; `aigraph` also GraphGenPatches):

| Command | Effect |
|---|---|
| `game.exe addon help` | Print command help |
| `game.exe addon version` | Print the addon compatibility version |
| `game.exe addon run` | Start the game |
| `game.exe addon check <path>...` | Check script text, `.mob` files, or directories of `.mob` files |
| `game.exe addon aigraph single <mob> [mpr]` | Generate the single-player AI graph |
| `game.exe addon aigraph multi <mob> [mpr]` | Generate the multiplayer AI graph |

Place `--moddir <name>` before the command to select a mod under the starter's `Mods` folder.
Without an `mpr` argument, the map name is derived from the mob name, dropping `-lmp`.
Script-check exit codes: 0 success, 2 unreadable file, 3 nothing checked, 4 script error or failed batch.

## Game script interface

addon.dll adds functions and commands to the game's script language, and replaces some of the game's own. The
[script reference](scripting.md) lists them with the rest of the language, tagged `eix`, with the differences from
addon.dll 0.12.4.2.

## Loader and start-up behavior

Only the differences; everything else behaves as the original.

| Behaviour | eix |
|---|---|
| `DirectInputCreateA`, `DirectInputCreateW`, `DirectInputCreateEx` | fixed: a missing system dinput.dll or export: a message box and `E_FAIL` (the original returns success with no interface and the game crashes) |
| Game build check | checks image base, size, time stamp and entry point (the original: the entry point alone); the box names the build found. The patches fit RU 1.06 byte for byte and would corrupt another build |
| Old-loader mode | not supported: an old loader reaching addon.dll gets an error box and the game exits. eix ships both DLLs, so the mode is never reached |
| Unknown game.exe in addon.dll | addon.dll checks the build again, logs it and stops with a box (the original addon.dll checks none) |
| Patch writes are verified | a site that does not hold the expected bytes skips the whole group, logged with site, expected and found bytes (the original logs and writes anyway) |
| config.reg and ai.reg | fixed: a file with a valid header and a broken body reads as empty, with an addon.log error (the original reads garbage or crashes); a section with no keys reads as empty (the original's lookup there crashes start-up) |
| addon.log: opening | an addon.log that cannot be opened gets a message box, and the game goes on (the original runs silently without it) |
| addon.log: when a line is written | fixed: each line is written at once (the original buffers, so a crash loses the last lines) |
| Exit of a command | fixed: ends with the same exit code and no box (the original stops on an `R6025 - pure virtual function call` box) |
| addon.log: what it records | also the eix build and commit, the Windows and Wine versions, the game.exe build, and the loaded modules (the original logs none of these) |
| Crash handler: a filter set later | fixed: a plugin that replaces the crash handler (Universal Mod's um.dll) no longer disables it: eix's handler runs the others first, then its own. A crash within a quarter of a second of the plugin setting its filter reaches only the plugin's |
| Crash handler: the record | addon.log gets the crash's code, address, access, registers and return addresses, each address with the patch group, fix or SpellAddon feature that put code there, and the loaded modules (the original writes only the dump) |
| Crash handler: the box | asks for a report at eix's issue tracker, naming addon.log and the dump (the original asks for the dump at its author's address) |
| `Tracer` | not reproduced: the original's per-patch hit counter reports wrong names and traps every hit |
| When built-in SpellAddon runs | applies when the sound system starts, where the original module loads; a separate `SpellAddon*.asi` is not loaded |
| spelladdon.ini choice | fixed: the mod folder's file, else the game folder's, else SpellAddon is off (the original reads `AddonPath`'s file unchecked, and with some registry setups a garbage path, so every key takes its default) |
| spelladdon.ini numeric parameters | fixed: `parammin`, `parammax`, `balance`, `exred`, `exgreen`, `exblue`, `hitdistmul` and `hitdistadd` keep fractions (the original truncates to integers); `framex` and `framey` keep their integer reading |
| spelladdon.ini digit parser | fixed: `runconst`, `runconstex`, `pow` and `expforint` without a decimal point read as whole numbers (the original divides by ten for each digit); points and commas keep their meaning |
| spelladdon.ini switches | keeps the signed low byte, with a warning for an out-of-range value naming the key, written value and used value (the original truncates silently) |
| spelladdon.ini aliases | accepts `avrgarmors` for `avgarmors`, and the gameplay-section spell AI, spell construction and multiplayer quest keys for their speed-section keys, with an info line; a canonical key wins, with a warning naming the ignored alias (the original ignores these aliases) |
| spelladdon.ini duplicate keys | the first value wins, with a warning naming the section, key, used and ignored values (the original keeps the first silently) |
| The fixed patch | the spell-name patch that the original always writes is written only when an INI is found |
| `SpellAddonInfo.txt` | not written (the original writes its documentation to the working folder) |
| Sites that do not hold | a feature whose sites do not hold the expected bytes, or whose original crashes, is logged and left out; the others apply (the original writes anyway, and a crash stops every later feature) |
