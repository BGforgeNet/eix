//! Values crossing the boundary: numbers to 32-bit ints as the original converts them, and the readings
//! of a callback's result.

use eix_engine::float::ftol;
use mlua::{Lua, Value};

/// `lua_tonumber`: a number, or a string LuaJIT converts to one; anything else is `None`.
pub(crate) fn to_number(lua: &Lua, value: &Value) -> Option<f64> {
    if let Value::Number(n) = value {
        Some(*n)
    } else if let Value::Integer(i) = value {
        Some(f64::from(*i))
    } else if let Value::String(_) = value {
        // A failed coercion is no number, as for `lua_tonumber`.
        lua.coerce_number(value.clone()).ok().flatten()
    } else {
        None
    }
}

/// An integer result: `lua_tointeger` (a number or numeric string, truncated; anything else 0).
pub(crate) fn int_result(lua: &Lua, value: &Value) -> i32 {
    to_number(lua, value).map_or(0, ftol)
}

/// A float result: a number or a string LuaJIT converts, else 0.0; narrowed to float.
pub(crate) fn float_result(lua: &Lua, value: &Value) -> f32 {
    to_number(lua, value).unwrap_or(0.0) as f32
}

/// A boolean result: true unless nil or false.
pub(crate) fn bool_result(value: &Value) -> bool {
    !matches!(value, Value::Nil | Value::Boolean(false))
}

/// A game pointer as Lua sees it: `lua_pushinteger` of its 32 bits. With a
/// large-address-aware game.exe a pointer above 2 GB is negative; [`lua_to_ptr`] gives the same 32 bits back.
pub(crate) const fn ptr_to_lua(ptr: u32) -> i32 {
    ptr.cast_signed()
}

/// The address a script passed, as the 32 bits it names.
pub(crate) const fn lua_to_ptr(value: i32) -> u32 {
    value.cast_unsigned()
}
