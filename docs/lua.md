# Lua

With `[Addon] EnableLuaScripts` set in the mod's ai.reg, `addon.dll` runs the mod's Lua script and calls it from nine
patch groups. The other ai.reg settings are under [addon and loader](addon.md#gameplay-aireg).

The last column of the table below gives eix's difference from addon.dll 0.12.4.2, with what the original does in
parentheses.

## Host

Set `[Addon] EnableLuaScripts` nonzero and supply `LuaMainScriptPath` (default `main.lua`). The host is LuaJIT 2.1,
with `bit` and `jit` loaded as globals; load `ffi` with `require("ffi")`. Module search paths include the mod root
and its `lua` folder. Define only the callbacks the mod changes: once the main script has run, each group hooks the
game where the script defines a callback, and the game's own code runs for the rest. A callback defined later is
not hooked. addon.log lists the callbacks found. A reported Lua error shows a message, logs its traceback, and exits.
Pointers are passed as signed 32-bit integers; raw memory operations require valid game memory. An address is the
running game's own: on the English/German game.exe much of game.exe's code and data lies at other addresses than on
the Russian one, so a script that reads or writes game.exe at fixed addresses works on the build it was written for.
Pointers the game hands to the script, such as units, work on both.

| Behaviour | eix |
|---|---|
| LuaJIT build | a newer LuaJIT 2.1 (the original: 2.1.0-beta3) |
| A callback the script does not define | the game's own code runs there (the original hooks every site of the nine groups and stops with a Lua error at the first call) |
| A Lua error | the message is shown as text (the original treats it as a printf format, so `%d` is consumed); the box is titled `addon.dll`; the message and its traceback also go to addon.log (the original logs nothing) |
| Lua state at exit | not closed at exit, since a fatal error box exits from inside a Lua call |

## Callbacks and functions

The [Lua reference](lua-reference.md) lists the callbacks the mod defines, with the patch group that calls each, and the
functions eix supplies. Each entry has its LuaLS signature and its difference from addon.dll 0.12.4.2. The same
declarations ship as the definitions file [eix.lua](eix.lua) for the Lua language server.

## Editor setup

With the definitions file, an editor running the Lua language server (LuaLS) completes eix's functions, shows their
documentation on hover and checks the arguments of each call.

1. Install the language server's plugin: in VS Code, the **Lua** extension (`sumneko.lua`); other editors run
   `lua-language-server`.
2. From the `docs` folder of the release package, put `eix.lua` in the game folder, beside `game.exe`, and
   `.luarc.json` in the mod folder (`Mods\<mod>`). Open the mod folder in the editor. The site has
   [eix.lua](https://bgforgenet.github.io/eix/eix.lua) too.

The `.luarc.json` names the definitions file relative to the mod folder:

```json
{
  "$schema": "https://raw.githubusercontent.com/LuaLS/vscode-lua/master/setting/schema.json",
  "runtime.version": "LuaJIT",
  "workspace.library": ["../../eix.lua"]
}
```

A mod folder outside the game's `Mods` folder names the file by its full path instead. A mod that has a `.luarc.json`
already takes the two settings into it. After an eix update, replace `eix.lua` with the one from the new release.
