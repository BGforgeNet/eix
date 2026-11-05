//! The FromMem / ToMem family and `StrFromMem`: raw reads and writes at the address a script passes.
#![expect(
    unsafe_code,
    reason = "scripts read and write game memory at addresses they choose, as under the original"
)]

use core::ffi::{CStr, c_char};

use mlua::{Lua, Value};

use crate::bind::register;
use crate::convert::lua_to_ptr;

/// Reads a `T` at `addr`, unaligned as x86 allows.
///
/// # Safety
/// `addr` points to readable memory holding a `T`. As in the original, nothing checks it: a script passing a wrong
/// address crashes the game, just as it can through `ffi`.
unsafe fn read<T: Copy>(addr: i32) -> T {
    let ptr = core::ptr::with_exposed_provenance::<T>(lua_to_ptr(addr) as usize);
    // SAFETY: the caller vouches for the address.
    unsafe { ptr.read_unaligned() }
}

/// Writes `value` at `addr`, unaligned as x86 allows.
///
/// # Safety
/// `addr` points to writable memory with room for a `T`; see [`read`].
unsafe fn write<T: Copy>(addr: i32, value: T) {
    let ptr = core::ptr::with_exposed_provenance_mut::<T>(lua_to_ptr(addr) as usize);
    // SAFETY: the caller vouches for the address.
    unsafe { ptr.write_unaligned(value) };
}

/// Registers functions 39 to 55, in the original's order.
pub(crate) fn register_all(lua: &Lua) -> mlua::Result<()> {
    register(lua, "StrFromMem", |lua, a| {
        let addr = a.int(1)?;
        // SAFETY: the script's address; it holds a `char*` (the function dereferences, it does not read characters).
        let ptr: *const c_char = unsafe { read::<*const c_char>(addr) };
        if ptr.is_null() {
            return Ok(Value::Nil);
        }
        // SAFETY: a non-null `char*` from game memory points to a NUL-terminated string, as `lua_pushstring` assumes.
        let bytes = unsafe { CStr::from_ptr(ptr) }.to_bytes();
        Ok(Value::String(lua.create_string(bytes)?))
    })?;
    register(lua, "FloatFromMem", |_, a| {
        let addr = a.int(1)?;
        // SAFETY: the script's address.
        Ok(f64::from(unsafe { read::<f32>(addr) }))
    })?;
    register(lua, "DoubleFromMem", |_, a| {
        let addr = a.int(1)?;
        // SAFETY: the script's address.
        Ok(unsafe { read::<f64>(addr) })
    })?;
    // UInt32FromMem is Int32FromMem in the original (one function, two names): values >= 2^31 come back negative.
    register(lua, "UInt32FromMem", |_, a| {
        let addr = a.int(1)?;
        // SAFETY: the script's address.
        Ok(unsafe { read::<i32>(addr) })
    })?;
    register(lua, "UInt16FromMem", |_, a| {
        let addr = a.int(1)?;
        // SAFETY: the script's address.
        Ok(i32::from(unsafe { read::<u16>(addr) }))
    })?;
    register(lua, "UInt8FromMem", |_, a| {
        let addr = a.int(1)?;
        // SAFETY: the script's address.
        Ok(i32::from(unsafe { read::<u8>(addr) }))
    })?;
    register(lua, "Int32FromMem", |_, a| {
        let addr = a.int(1)?;
        // SAFETY: the script's address.
        Ok(unsafe { read::<i32>(addr) })
    })?;
    register(lua, "Int16FromMem", |_, a| {
        let addr = a.int(1)?;
        // SAFETY: the script's address.
        Ok(i32::from(unsafe { read::<i16>(addr) }))
    })?;
    register(lua, "Int8FromMem", |_, a| {
        let addr = a.int(1)?;
        // SAFETY: the script's address.
        Ok(i32::from(unsafe { read::<i8>(addr) }))
    })?;
    register(lua, "FloatToMem", |_, a| {
        let addr = a.int(1)?;
        let value = a.number(2)? as f32;
        // SAFETY: the script's address.
        unsafe { write(addr, value) };
        Ok(())
    })?;
    register(lua, "DoubleToMem", |_, a| {
        let addr = a.int(1)?;
        let value = a.number(2)?;
        // SAFETY: the script's address.
        unsafe { write(addr, value) };
        Ok(())
    })?;
    // The UInt*ToMem functions are the Int*ToMem ones in the original; the value goes through the same int
    // conversion, so 4294967295 writes 0x80000000. Narrower stores keep the low bits.
    for name in [
        "UInt32ToMem",
        "UInt16ToMem",
        "UInt8ToMem",
        "Int32ToMem",
        "Int16ToMem",
        "Int8ToMem",
    ] {
        if name.contains("32") {
            register(lua, name, |_, a| {
                let addr = a.int(1)?;
                let value = a.int(2)?;
                // SAFETY: the script's address.
                unsafe { write(addr, value) };
                Ok(())
            })?;
        } else if name.contains("16") {
            register(lua, name, |_, a| {
                let addr = a.int(1)?;
                let value = a.int(2)? as u16;
                // SAFETY: the script's address.
                unsafe { write(addr, value) };
                Ok(())
            })?;
        } else {
            register(lua, name, |_, a| {
                let addr = a.int(1)?;
                let value = a.int(2)? as u8;
                // SAFETY: the script's address.
                unsafe { write(addr, value) };
                Ok(())
            })?;
        }
    }
    Ok(())
}
