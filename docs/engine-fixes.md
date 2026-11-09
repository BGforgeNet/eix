# Engine fixes

Fixes of game.exe RU 1.06 defects that the original addon.dll and SpellAddon leave. Code: `crates/addon/src/fixes/`,
one module per fix, applied in table order after the patch groups.

- A fix changes nothing a player sees, except the visible ones and the mod setting below.
- No fix writes a place a SpellAddon feature patches, or changes what a multiplayer peer computes.
- addon.log lists each fix under `fixes`: applied, failed with the reason, or off. With `EngineFixes` 0 one line
  says no fix applied, and only the mod setting is listed.

## Settings

DWORD values under `HKCU\Software\Gipat.Ru\EI_Starter\Settings`:

| Value | Effect |
|---|---|
| `EngineFixes` | 0: all fixes off; else on |
| `KeepWindowOnFocusLoss`, `SmoothCamera`, `PlayerListOnJoin`, `SkipDiscCheck` | 0: the game's behaviour for that fix; else on |
| `UnitShadowResolution` | 64 (default): the game's; 128 or 256: sharper unit shadows; another value is logged and stays 64 |
| `KeepRunningOnFocusLoss`, `ClipCursorToWindow` | 0 (default): off; else on |

A mod's `config.reg` can set `[MOD] ShowTutorialDefault`: the "Show tutorial" option's value while the game has
none stored, as on a first start. The game stores the option at exit, so the player's later choice wins. Without the
key the game's default (on) stands. `EngineFixes` 0 does not turn it off.

## Fixes

| Fix | Before | After |
|---|---|---|
| HeapPadding | heap overruns crash under Wine (lobby, shops, looting) | blocks padded (about 14 % more heap) |
| TimerResolution | frame times snap to 15.6 ms on Windows 10 2004+ | 1 ms timer |
| CastTargetGone | a cast whose target leaves the world crashes the server | the order ends |
| KillScriptOutside | `KillScript` outside a script writes through null (AntiCrash on) | reported, game goes on |
| LowercaseTable | slow per-character lower-case conversion in parsing and lookups | cached; zone loads about 30 % faster |
| ViewDepthInit | uninitialised depth can crash the draw sort on a first zone load | depth starts at 0 |
| MappedFileMessage | a failed-open message over 210 bytes overflows its buffer | path cut at 210 |
| PresentFailure | a screen lock or UAC prompt in full screen ends the game | flip waits, frame skipped |
| FullScreenFallback | a refused full-screen mode crashes | next larger mode, game in its top left corner |
| ScreenshotRowPadding | screenshots at some widths read past their buffer | buffer sized right |
| BriefingText | a guest joining during a briefing crashes the host | dialogue goes on |
| UnitPanelText | the unit panel stalls the frame for its text | drawn in memory, copied on change |
| StreamWait | a music change spins the main thread | the wait sleeps |
| QuestSort | quests without a `#quest` entry crash or hang the journal | sorted first |
| ExitIndex | a zone with unloaded exits crashes the briefing screen | empty exit, warning |
| FatalErrorExit | a fatal error with AntiCrash off crashes on shutdown | clean exit, same code |
| UnknownScreen | an unknown game state from the host crashes the client | logged, ignored |
| DiplomacySides | a side outside 0..31 reads and writes past the tables | hostile, unchanged, warning |
| MissingPlayerMode | a party member with cleared modes crashes the party check | skipped |
| NoAudioDevice | no audio device: movies crash | runs silent, warning |
| EmptyMusicTheme | an empty theme track list divides by zero | plays nothing, warning |
| ZoneModeText | `%` in a theme name read as a format | printed as text |
| StreamFadeLock | a fade races the stream thread | takes its lock |
| PointLightOverflow | the 257th script light overruns the table | refused, warning |
| EmptyItemStrip | a missing constructor record crashes a camp or shop strip | empty list, warning |
| UnregisteredLight | a light removed twice or moved before added corrupts memory | ignored, warning |
| FindHandleLeak | each music track leaks a file search handle | closed |
| LoadFailureLeaks | a failed read and two failed texture loads leak | freed |
| TruncatedScript | a script ending mid-statement crashes or hangs the game | "Unexpected end of file" |
| VertexLighting | 3DfpFPU.dll lights vertices in slow x87 code | SSE, four at a time, same colours bit for bit, also with Universal Mod's um-engine.dll |

## Visible fixes

Each has its setting above; `UnitShadowResolution`, `KeepRunningOnFocusLoss` and `ClipCursorToWindow` are off by
default, the others on. The focus fixes change the window only, never full screen. `EngineFixes` 0 turns all of them
off.

| Fix | Before | After |
|---|---|---|
| KeepWindowOnFocusLoss | in a window, the game minimises on focus loss | stays on screen, paused |
| KeepRunningOnFocusLoss | in a window, the game pauses (tick, sound, input) and minimises on focus loss; a multiplayer client falls out of step | keeps running and playing, not minimised |
| ClipCursorToWindow | in a window, the cursor leaves it | confined to the client area while the window has the focus; let go on focus loss, minimising and closing |
| UnitShadowResolution | unit shadows are 64 by 64 pixels | 128 or 256: a sharper edge, more video memory and work per shadowed unit; rendering only |
| SmoothCamera | panning stutters: a tick's camera step lands in one frame | spread over frames, same speed |
| PlayerListOnJoin | a client may see an empty player list until players change | refreshed when its player arrives |
| SkipDiscCheck | the starter's game.exe asks for the game disc | starts without it, as GOG's exe |

`KeepRunningOnFocusLoss` covers `KeepWindowOnFocusLoss` in a window: with both on, the game never sees the focus loss,
so nothing pauses and nothing minimises. With `KeepRunningOnFocusLoss` 0 the second still keeps the window on the
screen, paused. Minimising the window yourself stays the game's: it pauses, and resumes when the window is restored.
