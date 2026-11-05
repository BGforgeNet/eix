# Evil Islands eXtender

Engine fixes and tweaks for Evil Islands: Curse of the Lost Soul.

EIX implements most of the features provided by EI [addon.dll](docs/addon.md), as well as
[SpellAddon](docs/spelladdon.md), and adds [more](docs/engine-fixes.md). [Lua](docs/lua.md) scripting is available.

- Mods and plugins work the same way: same `config.reg`, `ai.reg` and `spelladdon.ini`.
- Game: `game.exe` RU 1.06 (EI starter's and GOG's Russian builds). Other builds not supported yet.

## Install

1. Install EIStarter-UM from [Universal Mod](https://github.com/Kyr4l/ei-universal-mod/releases). Just the starter,
   not the mod.
2. Download eix's zip from [releases](https://github.com/BGforgeNet/eix/releases).
3. Replace the starter's `dinput.dll` (beside `game.exe`) and `addon.dll` (in the starter's folder) with eix's. Keep
   the starter's two files to switch back.
4. Put `spelladdon.ini` in the game folder. A mod's own `spelladdon.ini`, in its folder, takes precedence.

To uninstall, put the starter's `dinput.dll` and `addon.dll` back.

## Reporting a problem

Open an [issue](https://github.com/BGforgeNet/eix/issues) with `addon.log` from the starter's folder. After a crash,
also attach the newest file from `Dumps` beside it.

## Development

Building, testing and releasing: [development](docs/development.md).

## Related projects

[ei-rebuild](https://github.com/DoctorGester/ei-rebuild)
| [ei-universal-mod](https://github.com/Kyr4l/ei-universal-mod)
| [CursedLands](https://github.com/TheWWWorm/CursedLands)
