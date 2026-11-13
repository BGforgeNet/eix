# Evil Islands eXtender

Engine fixes and tweaks for Evil Islands: Curse of the Lost Soul.

EIX is an open source reimplementation of EI [addon.dll](docs/addon.md) and [SpellAddon](docs/spelladdon.md), with
additional fixes and features.

Reference pages: [eix site](https://bgforgenet.github.io/eix/).

<!-- ANCHOR: compat -->
- Mods and plugins work the same way: same `config.reg`, `ai.reg` and `spelladdon.ini`.
- Game: `game.exe` 1.06, EI starter's or GOG's, any language.
<!-- ANCHOR_END: compat -->

## For players

- [Engine fixes](docs/engine.md#fixes): multiple crash fixes and faster zone loading.
- [Widescreen](docs/addon.md#launcher-settings): the interface adapts to wide resolution.
- [Game options](docs/engine.md#game-options): keep running or stay on screen when the window loses focus, confine the
  cursor, smoother camera, sharper unit shadows, spells on the unit panel.
- [AutoPause](docs/engine.md#autopause): pause on low health, an enemy in sight or an attack, in single player.
- [Encumbrance rebalance](docs/engine.md#encumbrance): more stamina allows to run longer, but heavy load reduces speed
  and increases stamina spend.
- [FLAC music](docs/engine.md#flac-music): FLAC tracks in place of MP3.
- All SpellAddon features that a mod enables.

Everything is optional, see [eix.ini](docs/eix-ini.md) for configuration.

For a ready-made gameplay and quality-of-life mod, see [EItweaks](https://github.com/BGforgeNet/EItweaks).

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

## For modders

- Drop-in replacement for the starter's `addon.dll`, loader and SpellAddon: [addon and loader](docs/addon.md) and
  [SpellAddon options](docs/spelladdon.md) list every setting and its differences from the originals.
- Plugins: DLL plugins load from the mod folder, set in
  [config.reg](docs/addon.md#mod-configuration-configreg).
- [Lua scripting](docs/lua.md): callbacks into the game, with the [Lua reference](docs/lua-reference.md) of its
  functions and callbacks.
- [Script reference](docs/scripting.md): every function and command of the game's script language.
- [INI variables](docs/engine.md#ini-variables): map scripts read numbers from the mod's INI files.
- [Recipes](docs/recipes.md): common changes, with the file, keys and values.

<!-- ANCHOR: report -->

## Development

Building, testing and releasing: [development](docs/development.md).

## Related projects

[ei-rebuild](https://github.com/DoctorGester/ei-rebuild)
| [ei-universal-mod](https://github.com/Kyr4l/ei-universal-mod)
| [CursedLands](https://github.com/TheWWWorm/CursedLands)
