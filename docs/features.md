# Features

eix's own additions to the game, beside the originals' features and the [fixes](fixes.md). Each is
set in [eix.ini](eix-ini.md): AutoPause and encumbrance are off by default, FLAC music on.

## Game options

Window, display and start-up changes. Each has its own key in [eix.ini](eix-ini.md#enginefixes)'s `[EngineFixes]`;
`UnitShadowResolution`, `KeepRunningOnFocusLoss` and `ClipCursorToWindow` are off by default, the others on. The
focus options change the window only, never full screen. `Enabled` 0 there turns all of them off. addon.log lists each
under `fixes`.

| Option | Before | After |
|---|---|---|
| KeepWindowOnFocusLoss | in a window, the game minimises on focus loss | stays on screen, paused |
| KeepRunningOnFocusLoss | in a window, the game pauses (tick, sound, input) and minimises on focus loss; a multiplayer client falls out of step | keeps running and playing, not minimised |
| ClipCursorToWindow | in a window, the cursor leaves it | confined to the client area while the window has the focus; let go on focus loss, minimising and closing |
| UnitShadowResolution | unit shadows are 64 by 64 pixels | 128 or 256: a sharper edge, more video memory and work per shadowed unit; rendering only |
| SmoothCamera | panning stutters: a tick's camera step lands in one frame | spread over frames, same speed |
| PlayerListOnJoin | a client may see an empty player list until players change | refreshed when its player arrives |
| SkipDiscCheck | the starter's game.exe asks for the game disc | starts without it, as GOG's exe |
| UnitPanelSpells | the unit panel's values page shows no spells of a unit outside the party | each spell below the values: its icon and "Effect:" with its value, as the spell windows show it; a page taller than the panel gets the game's scroll bar (arrows, thumb, and the mouse wheel over the page instead of the camera's zoom); single player and the host only |

`KeepRunningOnFocusLoss` covers `KeepWindowOnFocusLoss` in a window: with both on, the game never sees the focus loss,
so nothing pauses and nothing minimises. With `KeepRunningOnFocusLoss` 0 the second still keeps the window on the
screen, paused. Minimising the window yourself stays the game's: it pauses, and resumes when the window is restored.

## AutoPause

In single player, the game pauses when a party member's health drops low, an enemy comes into view, an enemy turns on
the party, or an enemy in view dies; each event has its own [setting](eix-ini.md#autopause). A network game never
pauses. addon.log names what paused the game.

- Health: a party member's health drops below `Health` or `HealthPercent`; with both set, either limit pauses the
  game. It pauses once per drop: the member's health must rise to the limit again before the next drop pauses the
  game. A member that is already low when it comes into view (a zone loads, a new member joins) does not pause the
  game.
- An enemy is a living unit without a player that is hostile to the party's side. The party's view is what its
  members see, as the script `PlayerSee` gives it.
- `EnemySighted` and `EnemyAggro` pause once per encounter: the next pause comes after no enemy is left in view, or
  aggressive, and one appears again. Enemies already in view, or aggressive, when a game loads pause it at once.

## Encumbrance

The load a hero carries slows running, and makes running cost more mana; above the hero's maximum load it slows
walking too. Load and maximum load are the encumbrance figures of the hero panel; their ratio is the encumbrance below
(1.0 at the maximum). The rates are [settings](eix-ini.md#encumbrance).

- Running speed is 100% - `RunSlope` x encumbrance: with the defaults 80% at the maximum load, 60% at twice it.
- Walking, sneaking and crawling keep the game's speed up to the maximum load, then lose `WalkSlope` per extra
  maximum load: 80% at twice the maximum.
- Turning while walking or standing slows with the running speed: the game turns a walking or standing unit at its
  running pace.
- Running drains the mana the game's run system takes, divided by the load's running speed: at 80%, 1.25 times as
  much. Wounded legs slow the hero without raising the drain. `RunDrain` scales the extra: 0 keeps the game's drain,
  50 halves the extra.
- No load stops a hero from running; wounded legs still slow every movement and stop running, as in the game.
- The speeds follow the load when the game recomputes a hero's statistics (regeneration, damage, healing, effects),
  not at the moment an item is equipped.
- With SpellAddon's `encumbrancemovespeed` also on, both slow movement; addon.log warns. SpellAddon's
  `heroalwayscanrun` changes the same byte as this feature. Its run settings (`newrunsystem`, `runconst`) decide the
  drain that `RunDrain` scales.

## FLAC music

eix plays a music or streamed-sound track from `Stream\<name>.flac` when there is one, in place of `<name>.mp3`: the
mod's `Stream` folder first, then the game's. The order is the mod's `.flac`, the mod's `.mp3`, the game's `.flac`,
the game's `.mp3`.

- The track keeps its `.mp3` name in the game: `music.reg` names and `streamsn.reg` sections (`<name>.mp3`) stay.
- Mono or stereo, any sample rate, up to 32 bits a sample (played as 16 bits). Packs can mix FLAC and MP3 tracks.
- addon.log names each FLAC file played. A file that does not decode plays silence, with a warning.
- Briefing speech (`speech.res`) stays MP3.
- On by default; [eix.ini](eix-ini.md#music)'s `[Music] Flac=0` turns it off.
