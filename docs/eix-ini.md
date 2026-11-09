# eix.ini

eix's own settings, in `eix.ini` in the mod's folder (`Mods\<mod>`, beside its `config.reg`). Without the file, or
with no mod selected, every key takes its default. The package's copy holds every key at its default.

addon.log names the file read, and each value left out with the reason.

## EngineFixes

The switches of the [engine fixes](engine-fixes.md):

| Key | Value |
|---|---|
| `Enabled` | 0: all fixes off; else on (default 1) |
| `KeepWindowOnFocusLoss`, `SmoothCamera`, `PlayerListOnJoin`, `SkipDiscCheck`, `UnitPanelSpells` | 0: the game's behaviour for that fix; else on (default 1) |
| `UnitShadowResolution` | 64 (default): the game's; 128 or 256: sharper unit shadows; another value is logged and stays 64 |
| `KeepRunningOnFocusLoss`, `ClipCursorToWindow` | 0 (default): off; else on |

## AutoPause

In single player, the game pauses on the events below, each off by default. A network game never pauses. addon.log
names what paused the game.

| Key | Value |
|---|---|
| `Health` | Hit points; 0 (default): off |
| `HealthPercent` | A percentage of the member's maximum hit points, 1 to 100; 0 (default): off |
| `EnemySighted` | 1: an enemy comes into the party's view while none was in view |
| `EnemyAggro` | 1: an enemy turns on a party member while no enemy was aggressive to the party |
| `EnemyDead` | 1: an enemy in the party's view dies, whoever killed it |

- Health: a party member's health drops below `Health` or `HealthPercent`; with both set, either limit pauses the
  game. It pauses once per drop: the member's health must rise to the limit again before the next drop pauses the
  game. A member that is already low when it comes into view (a zone loads, a new member joins) does not pause the
  game. A negative value, or a percentage above 100, is logged and is off.
- An enemy is a living unit without a player that is hostile to the party's side. The party's view is what its
  members see, as the script `PlayerSee` gives it.
- `EnemySighted` and `EnemyAggro` pause once per encounter: the next pause comes after no enemy is left in view, or
  aggressive, and one appears again. Enemies already in view, or aggressive, when a game loads pause it at once.

```ini
[AutoPause]
Health=0
HealthPercent=25
EnemySighted=1
EnemyAggro=0
EnemyDead=1
```

## Music

| Key | Value |
|---|---|
| `Flac` | 1 (default): a track plays from its `.flac` when there is one ([FLAC music](addon.md#flac-music)); 0: only the game's own stream code runs |
