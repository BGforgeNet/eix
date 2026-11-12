# Evil Islands eXtender

Engine fixes and tweaks for Evil Islands: Curse of the Lost Soul.

EIX is an open source reimplementation of EI [addon.dll](docs/addon.md) and [SpellAddon](docs/spelladdon.md).
It also provides additional [fixes](docs/engine.md#fixes) and [features](docs/engine.md#features). LUA [scripting](docs/lua.md) is available.

Reference pages: [eix site](https://bgforgenet.github.io/eix/).

<!-- ANCHOR: compat -->
- Mods and plugins work the same way: same `config.reg`, `ai.reg` and `spelladdon.ini`.
- Game: `game.exe` 1.06, Russian (EI starter's and GOG's) or English and German (GOG's, one file).
<!-- ANCHOR_END: compat -->

<!-- ANCHOR: install -->

## Install

1. Install EIStarter-UM from [Universal Mod](https://github.com/Kyr4l/ei-universal-mod/releases). Just the starter,
   not the mod.
2. Download eix's zip from [releases](https://github.com/BGforgeNet/eix/releases).
3. Replace the starter's `dinput.dll` (beside `game.exe`) and `addon.dll` (in the starter's folder) with eix's. Keep
   the starter's two files to switch back.
4. Put `spelladdon.ini` in the game folder. A mod's own `spelladdon.ini`, in its folder, takes precedence.
5. Put `eix.ini` in the mod's folder (`Mods\<mod>`): eix's [own settings](docs/eix-ini.md), such as pausing on a
   party member's low health.

To uninstall, put the starter's `dinput.dll` and `addon.dll` back.
<!-- ANCHOR_END: install -->

<!-- ANCHOR: report -->

## Development

Building, testing and releasing: [development](docs/development.md).

## Related projects

[ei-rebuild](https://github.com/DoctorGester/ei-rebuild)
| [ei-universal-mod](https://github.com/Kyr4l/ei-universal-mod)
| [CursedLands](https://github.com/TheWWWorm/CursedLands)
