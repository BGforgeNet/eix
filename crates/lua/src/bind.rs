//! The binding of registered functions: argument checks with LuaJIT's own argument errors, raised only after the
//! Rust implementation has returned.
//!
//! Each global is a C closure (the trampoline) over an mlua function (the implementation). The implementation
//! returns a status, then its results; only mlua's own errors (out of memory) are raised by mlua, after its scopes
//! end. The trampoline holds no Rust value with a destructor, so raising
//! there skips nothing; LuaJIT then words the error and names the function, as for the original's
//! `luaL_checkinteger` (`bad argument #1 to 'Round' (number expected, got string)`).
#![expect(
    unsafe_code,
    reason = "the trampoline is a raw C function that raises the argument error"
)]

use core::ffi::c_int;

use eix_engine::float::ftol;
use mlua::ffi::{self, lua_State};
use mlua::{Function, IntoLuaMulti, Lua, MultiValue, Value};

use crate::convert::to_number;

/// An argument that failed its check: its 1-based position and the type it should have had.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArgError {
    /// `number expected`.
    Number(usize),
    /// `string expected`.
    String(usize),
}

/// Why an implementation did not return its results.
#[derive(Debug)]
pub(crate) enum Fail {
    /// An argument failed its check; the trampoline raises LuaJIT's argument error.
    Arg(ArgError),
    /// An mlua call failed (out of memory); mlua raises it.
    Lua(mlua::Error),
}

impl From<ArgError> for Fail {
    fn from(e: ArgError) -> Self {
        Self::Arg(e)
    }
}

impl From<mlua::Error> for Fail {
    fn from(e: mlua::Error) -> Self {
        Self::Lua(e)
    }
}

/// Status values the implementation returns first.
const STATUS_OK: ffi::lua_Integer = 0;
const STATUS_NUMBER: ffi::lua_Integer = 1;
const STATUS_STRING: ffi::lua_Integer = 2;

/// The arguments of one call, checked as the original's C functions check them. Extra arguments are ignored.
pub(crate) struct Args<'a> {
    lua: &'a Lua,
    values: &'a MultiValue,
}

impl Args<'_> {
    fn get(&self, position: usize) -> &Value {
        self.values.get(position - 1).unwrap_or(&Value::Nil)
    }

    /// `n`: a number or a numeric string.
    pub(crate) fn number(&self, position: usize) -> Result<f64, ArgError> {
        to_number(self.lua, self.get(position)).ok_or(ArgError::Number(position))
    }

    /// `i`: `luaL_checkinteger`, a number or numeric string truncated as the original does.
    pub(crate) fn int(&self, position: usize) -> Result<i32, ArgError> {
        self.number(position).map(ftol)
    }

    /// `s`: `luaL_checklstring`, a string or a number converted to one; the bytes up to the first NUL, as the C
    /// functions read them.
    pub(crate) fn bytes(&self, position: usize) -> Result<Vec<u8>, ArgError> {
        let value = self.get(position);
        let string = if let Value::String(s) = value {
            Some(s.clone())
        } else if matches!(value, Value::Number(_) | Value::Integer(_)) {
            self.lua.coerce_string(value.clone()).ok().flatten()
        } else {
            None
        };
        let Some(string) = string else {
            return Err(ArgError::String(position));
        };
        let bytes = string.as_bytes();
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        Ok(bytes[..end].to_vec())
    }
}

/// Makes `name` a global calling `f` through the trampoline.
pub(crate) fn register<R, F>(lua: &Lua, name: &str, f: F) -> mlua::Result<()>
where
    R: IntoLuaMulti,
    F: Fn(&Lua, &Args<'_>) -> Result<R, Fail> + 'static,
{
    let implementation = lua.create_function(move |lua, values: MultiValue| {
        let args = Args {
            lua,
            values: &values,
        };
        match f(lua, &args) {
            Ok(results) => {
                let mut out = results.into_lua_multi(lua)?;
                out.push_front(Value::Integer(STATUS_OK));
                Ok(out)
            }
            Err(Fail::Arg(ArgError::Number(position))) => {
                (STATUS_NUMBER, position).into_lua_multi(lua)
            }
            Err(Fail::Arg(ArgError::String(position))) => {
                (STATUS_STRING, position).into_lua_multi(lua)
            }
            Err(Fail::Lua(e)) => Err(e),
        }
    })?;
    let close_over = |state: *mut lua_State| {
        // SAFETY: the implementation is the one value on the stack; it becomes the upvalue.
        unsafe { ffi::lua_pushcclosure(state, trampoline, 1) };
    };
    // SAFETY: `close_over` pops its one argument and leaves the C closure as the result.
    let global: Function = unsafe { lua.exec_raw(implementation, close_over) }?;
    lua.globals().set(name, global)
}

/// Calls the implementation (upvalue 1) with copies of the arguments, so that an argument error can still name the
/// type it got; returns its results or raises its argument error.
///
/// # Safety
/// Called by Lua as the C closure `register` builds.
unsafe extern "C-unwind" fn trampoline(l: *mut lua_State) -> c_int {
    // SAFETY: a running C function's stack.
    let nargs = unsafe { ffi::lua_gettop(l) };
    // SAFETY: raises LuaJIT's stack overflow error when the stack cannot grow; nothing here needs dropping.
    unsafe { ffi::luaL_checkstack(l, nargs + 1, core::ptr::null()) };
    // SAFETY: upvalue 1 is the implementation.
    unsafe { ffi::lua_pushvalue(l, ffi::lua_upvalueindex(1)) };
    for i in 1..=nargs {
        // SAFETY: `i` is an argument index; the stack has room for the copies.
        unsafe { ffi::lua_pushvalue(l, i) };
    }
    // SAFETY: function and arguments are on top; an error raised inside (only an out-of-memory one) passes through
    // this frame, which holds nothing to drop.
    unsafe { ffi::lua_call(l, nargs, ffi::LUA_MULTRET) };
    let status_index = nargs + 1;
    // SAFETY: the implementation always returns its status first.
    let status = unsafe { ffi::lua_tointeger(l, status_index) };
    if status == STATUS_OK {
        // SAFETY: a running C function's stack.
        let top = unsafe { ffi::lua_gettop(l) };
        return top - status_index;
    }
    // SAFETY: an error status is followed by the argument position.
    let position = unsafe { ffi::lua_tointeger(l, status_index + 1) };
    let expected = if status == STATUS_STRING {
        c"string"
    } else {
        c"number"
    };
    // Positions come from the implementation's own small argument lists.
    let position = c_int::try_from(position).unwrap_or(c_int::MAX);
    // Leaves only the arguments, so that a missing one reads as "no value" rather than the status above them.
    // SAFETY: `nargs` is at most the current top.
    unsafe { ffi::lua_settop(l, nargs) };
    // SAFETY: raises the argument error with the arguments at their positions for LuaJIT's "got ..." part.
    unsafe { ffi::luaL_typerror(l, position, expected.as_ptr()) }
}
