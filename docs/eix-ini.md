# eix.ini

eix's own settings, in `eix.ini` in the mod's folder (`Mods\<mod>`, beside its `config.reg`). Without the file, or
with no mod selected, every key takes its default. The package's copy holds every key at its default.

addon.log names the file read, and each value left out with the reason.

## EngineFixes

The switches of the [fixes](fixes.md) and the [game options](features.md#game-options):

| Key | Value |
|---|---|
| `Enabled` | 0: all fixes off; else on (default 1) |
| `KeepWindowOnFocusLoss`, `SmoothCamera`, `PlayerListOnJoin`, `SkipDiscCheck`, `UnitPanelSpells` | 0: the game's behaviour for that option; else on (default 1) |
| `UnitShadowResolution` | 64 (default): the game's; 128 or 256: sharper unit shadows; another value is logged and stays 64 |
| `KeepRunningOnFocusLoss`, `ClipCursorToWindow` | 0 (default): off; else on |

## AutoPause

The events that pause single player ([AutoPause](features.md#autopause)), each off by default:

| Key | Value |
|---|---|
| `Health` | Hit points; 0 (default): off |
| `HealthPercent` | A percentage of the member's maximum hit points, 1 to 100; 0 (default): off |
| `EnemySighted` | 1: an enemy comes into the party's view while none was in view |
| `EnemyAggro` | 1: an enemy turns on a party member while no enemy was aggressive to the party |
| `EnemyDead` | 1: an enemy in the party's view dies, whoever killed it |

A negative `Health`, or a `HealthPercent` above 100, is logged and is off.

```ini
[AutoPause]
Health=0
HealthPercent=25
EnemySighted=1
EnemyAggro=0
EnemyDead=1
```

## Encumbrance

The rates of [encumbrance](features.md#encumbrance), off by default. Encumbrance is load over maximum load (1.0 at
the maximum).

| Key | Value |
|---|---|
| `Enabled` | 1: on; 0 (default): the game's movement |
| `RunSlope` | Running speed lost per maximum load, in percent, 0 to 100 (default 20) |
| `WalkSlope` | Walking speed lost per maximum load above the maximum, in percent, 0 to 100 (default 20) |
| `MinSpeed` | The lowest speed either reaches, in percent, 10 to 100 (default 50) |
| `RunDrain` | How much of the extra running drain applies, in percent, 0 to 200 (default 100) |

A value out of its range is logged and takes its default.

```ini
[Encumbrance]
Enabled=1
RunSlope=20
WalkSlope=20
MinSpeed=50
RunDrain=100
```

## Music

| Key | Value |
|---|---|
| `Flac` | 1 (default): a track plays from its `.flac` when there is one ([FLAC music](features.md#flac-music)); 0: only the game's own stream code runs |
