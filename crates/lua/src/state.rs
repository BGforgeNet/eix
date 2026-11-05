//! The state as addon.dll 0.12.4.2 creates it: `luaL_openlibs`, nothing else changed.
#![expect(
    unsafe_code,
    reason = "the debug library needs mlua's unsafe constructor, and ffi's opener is a raw C function"
)]

use mlua::{Function, Lua, LuaOptions, StdLib, Table};

/// A Lua state with LuaJIT's `luaL_openlibs` set: base, `package`, `table`, `io`, `os`, `string`, `math`, `debug`,
/// `bit` and `jit` opened, and `ffi` in `package.preload`, so `require("ffi")` loads it and no `ffi` global exists.
pub fn new_state() -> mlua::Result<Lua> {
    let libs = StdLib::ALL ^ StdLib::FFI;
    // `catch_rust_panics` stays on: turning it off replaces `pcall` and `xpcall` with C functions, across which a
    // coroutine cannot yield.
    // SAFETY: mod scripts get `debug`, `ffi` and `package.loadlib` as under the original addon; a script runs with
    // the game process's full rights either way.
    let lua = unsafe { Lua::unsafe_new_with(libs, LuaOptions::new()) };
    // SAFETY: `luaopen_ffi` follows the Lua C function protocol; `require` calls it as a loader.
    let open_ffi: Function = unsafe { lua.create_c_function(mlua::ffi::luaopen_ffi) }?;
    let package: Table = lua.globals().get("package")?;
    let preload: Table = package.get("preload")?;
    preload.set("ffi", open_ffi)?;
    Ok(lua)
}
