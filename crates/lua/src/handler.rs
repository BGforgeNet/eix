//! The message handler of every protected call: lua.c 5.2's traceback handler, on LuaJIT's own
//! `luaL_traceback`.
#![expect(
    unsafe_code,
    reason = "a raw C function, so that the traceback starts at the erroring frame"
)]

use core::ffi::{c_char, c_int};

use mlua::ffi::{self, lua_State};
use mlua::{Function, Lua};

unsafe extern "C-unwind" {
    /// LuaJIT's `luaL_traceback`. mlua-sys exposes a Rust port of Lua 5.3's under the same name, whose frame names
    /// differ from LuaJIT's (it names functions found in `package.loaded`).
    #[link_name = "luaL_traceback"]
    fn luajit_traceback(l: *mut lua_State, l1: *mut lua_State, msg: *const c_char, level: c_int);
}

/// The handler as a Lua function.
pub(crate) fn create(lua: &Lua) -> mlua::Result<Function> {
    // SAFETY: `message_handler` follows the Lua C function protocol.
    unsafe { lua.create_c_function(message_handler) }
}

/// A string or number error value becomes `luaL_traceback(L, L, msg, 1)`; another non-nil value whose
/// `__tostring` returns a string becomes the traceback of that string; anything else is returned unchanged.
///
/// # Safety
/// Called by Lua as a message handler, with the error value at index 1.
unsafe extern "C-unwind" fn message_handler(l: *mut lua_State) -> c_int {
    // SAFETY: index 1 holds the error value (or none, which reads as not a string).
    let is_string = unsafe { ffi::lua_isstring(l, 1) } != 0;
    if is_string {
        // SAFETY: index 1 is a string or a number, which `lua_tolstring` converts.
        let msg = unsafe { ffi::lua_tolstring(l, 1, core::ptr::null_mut()) };
        // SAFETY: `msg` stays alive on the stack during the call; level 1 is the frame that raised the error.
        unsafe { luajit_traceback(l, l, msg, 1) };
        return 1;
    }
    // SAFETY: index 1 is a valid or acceptable index.
    let kind = unsafe { ffi::lua_type(l, 1) };
    if kind > ffi::LUA_TNIL {
        // SAFETY: index 1 holds a value; the metamethod's own error ends in LuaJIT's "error in error handling".
        let called = unsafe { ffi::luaL_callmeta(l, 1, c"__tostring".as_ptr()) } != 0;
        // SAFETY: when `called`, the metamethod's result is on top.
        if called && unsafe { ffi::lua_type(l, -1) } == ffi::LUA_TSTRING {
            // SAFETY: the top is a string.
            let msg = unsafe { ffi::lua_tolstring(l, -1, core::ptr::null_mut()) };
            // SAFETY: as above; the string stays on the stack during the call.
            unsafe { luajit_traceback(l, l, msg, 1) };
            return 1;
        }
    }
    // SAFETY: index 1 exists or reads as nil; the stack is set to hold just it.
    unsafe { ffi::lua_settop(l, 1) };
    1
}
