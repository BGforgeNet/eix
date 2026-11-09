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

## Encumbrance

The load a hero carries slows running, and makes running cost more mana; above the hero's maximum load it slows
walking too. Off by default. Load and maximum load are the encumbrance figures of the hero panel; their ratio is the
encumbrance below (1.0 at the maximum).

| Key | Value |
|---|---|
| `Enabled` | 1: on; 0 (default): the game's movement |
| `RunSlope` | Running speed lost per maximum load, in percent, 0 to 100 (default 20) |
| `WalkSlope` | Walking speed lost per maximum load above the maximum, in percent, 0 to 100 (default 20) |
| `MinSpeed` | The lowest speed either reaches, in percent, 10 to 100 (default 50) |
| `RunDrain` | How much of the extra running drain applies, in percent, 0 to 200 (default 100) |

- Running speed is 100% - `RunSlope` x encumbrance: with the defaults 80% at the maximum load, 60% at twice it.
- Walking, sneaking and crawling keep the game's speed up to the maximum load, then lose `WalkSlope` per extra
  maximum load: 80% at twice the maximum.
- Running drains the mana the game's run system takes, divided by the load's running speed: at 80%, 1.25 times as
  much. Wounded legs slow the hero without raising the drain. `RunDrain` scales the extra: 0 keeps the game's drain,
  50 halves the extra.
- No load stops a hero from running; wounded legs still slow every movement and stop running, as in the game.
- The speeds follow the load when the game recomputes a hero's statistics (regeneration, damage, healing, effects),
  not at the moment an item is equipped.
- A value out of its range is logged and takes its default.
- With SpellAddon's `encumbrancemovespeed` also on, both slow movement; addon.log warns. SpellAddon's
  `heroalwayscanrun` changes the same byte as this section. Its run settings (`newrunsystem`, `runconst`) decide the
  drain that `RunDrain` scales.

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
| `Flac` | 1 (default): a track plays from its `.flac` when there is one ([FLAC music](addon.md#flac-music)); 0: only the game's own stream code runs |
