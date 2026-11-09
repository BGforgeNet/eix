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

In single player, the game pauses when a party member's health drops below a limit. It pauses once per drop: the
member's health must rise to the limit again before the next drop pauses the game. A member that is already low when
it comes into view (a zone loads, a new member joins) does not pause the game. A network game never pauses.

| Key | Value |
|---|---|
| `Health` | Hit points; 0 (default): off |
| `HealthPercent` | A percentage of the member's maximum hit points, 1 to 100; 0 (default): off |

With both set, either limit pauses the game. A negative value, or a percentage above 100, is logged and is off.

```ini
[AutoPause]
Health=0
HealthPercent=25
```
