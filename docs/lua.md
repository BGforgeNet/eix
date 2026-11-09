# Lua

With `[Addon] EnableLuaScripts` set in the mod's ai.reg, `addon.dll` runs the mod's Lua script and calls it from nine
patch groups. The other ai.reg settings are under [addon and loader](addon.md#gameplay-aireg).

The last column of the table below gives eix's difference from addon.dll 0.12.4.2, with what the original does in
parentheses.

## Host

Set `[Addon] EnableLuaScripts` nonzero and supply `LuaMainScriptPath` (default `main.lua`). The host is LuaJIT 2.1,
with `bit` and `jit` loaded as globals; load `ffi` with `require("ffi")`. Module search paths include the mod root
and its `lua` folder. Define all callbacks that enabled gameplay paths call.
A reported Lua error shows a message, logs its traceback, and exits.
Pointers are passed as signed 32-bit integers; raw memory operations require valid game memory.

| Behaviour | eix |
|---|---|
| LuaJIT build | a newer LuaJIT 2.1 (the original: 2.1.0-beta3) |
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
2. Get `eix.lua`: the `docs` folder of the release package has it, and so does
   [the site](https://bgforgenet.github.io/eix/eix.lua). Keep it outside the mod folder, so the mod does not ship it.
3. In the mod folder (the editor's workspace), create `.luarc.json` naming the file:

   ```json
   {
     "runtime.version": "LuaJIT",
     "workspace.library": ["C:/EvilIslands/eix/eix.lua"]
   }
   ```

   The path is an example: use where you saved the file. A folder holding it works too.
4. Reopen the folder.

After an eix update, replace `eix.lua` with the one from the new release.
