#![expect(
    unsafe_code,
    reason = "the control test raises a Lua error from a raw C function"
)]

use std::cell::Cell;
use std::ffi::c_int;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

use eix_lua::new_state;

#[test]
fn runs_luajit_2_1() {
    let lua = new_state().unwrap();
    let version: String = lua.load("return jit.version").eval().unwrap();
    assert!(version.starts_with("LuaJIT 2.1"), "{version}");
}

#[test]
fn pairs_walks_string_keys_in_the_same_order_in_every_state() {
    let order = || -> String {
        let lua = new_state().unwrap();
        lua.load(
            r#"
            local t, keys = {}, {}
            for i = 1, 64 do t["key" .. i] = i end
            for k in pairs(t) do keys[#keys + 1] = k end
            return jit.security("prng") .. ":" .. table.concat(keys, ",")
            "#,
        )
        .eval()
        .unwrap()
    };
    let first = order();
    assert!(first.starts_with("0:"), "built with a seeded PRNG: {first}");
    assert_eq!(first, order());
}

#[test]
fn has_bit_library() {
    let lua = new_state().unwrap();
    let x: i32 = lua.load("return bit.bxor(5, 3)").eval().unwrap();
    assert_eq!(x, 6);
}

#[test]
fn ffi_round_trips_a_value() {
    let lua = new_state().unwrap();
    let x: i32 = lua
        .load(
            r#"
            local ffi = require("ffi")
            ffi.cdef("typedef struct { int a; } probe_t;")
            local p = ffi.new("probe_t[1]")
            p[0].a = 41
            return p[0].a + 1
            "#,
        )
        .eval()
        .unwrap();
    assert_eq!(x, 42);
}

/// Counts its drops; a Lua error that skipped Rust destructors would leave the count at 0.
struct Guard(Rc<Cell<u32>>);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn failing_callback_drops_its_values() {
    let lua = new_state().unwrap();
    let drops = Rc::new(Cell::new(0));
    let d = drops.clone();
    let fail = lua
        .create_function(move |_, ()| {
            let _guard = Guard(d.clone());
            Err::<(), _>(mlua::Error::RuntimeError("boom".into()))
        })
        .unwrap();
    lua.globals().set("fail", fail).unwrap();
    let (ok, msg): (bool, String) = lua
        .load("local ok, e = pcall(fail) return ok, tostring(e)")
        .eval()
        .unwrap();
    assert!(!ok);
    assert!(msg.contains("boom"), "{msg}");
    assert_eq!(drops.get(), 1);
}

#[test]
fn lua_error_inside_callback_drops_its_values() {
    let lua = new_state().unwrap();
    let drops = Rc::new(Cell::new(0));
    let d = drops.clone();
    let reenter = lua
        .create_function(move |lua, ()| {
            let _guard = Guard(d.clone());
            lua.load("error('inner')").exec()?;
            Ok(())
        })
        .unwrap();
    lua.globals().set("reenter", reenter).unwrap();
    let (ok, msg): (bool, String) = lua
        .load("local ok, e = pcall(reenter) return ok, tostring(e)")
        .eval()
        .unwrap();
    assert!(!ok);
    assert!(msg.contains("inner"), "{msg}");
    assert_eq!(drops.get(), 1);
}

static RAW_DROPS: AtomicU32 = AtomicU32::new(0);

struct RawGuard;

impl Drop for RawGuard {
    fn drop(&mut self) {
        RAW_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

/// Raises a Lua error while a Rust value is alive, the way a raw binding without mlua's discipline would.
unsafe extern "C-unwind" fn raw_fail(state: *mut mlua::lua_State) -> c_int {
    let _guard = RawGuard;
    // SAFETY: `state` is the live state that called us; the string is NUL-terminated.
    unsafe { mlua::ffi::lua_pushstring(state, c"raw".as_ptr()) };
    // SAFETY: raising is what this control does; the caller's `pcall` catches it.
    unsafe { mlua::ffi::lua_error(state) }
}

/// Control: shows the tests above can see the hazard. LuaJIT raises errors as SEH on 32-bit Windows, which skips
/// the Rust frame's destructors, so a raw binding drops nothing.
#[test]
fn raw_lua_error_skips_destructors() {
    let lua = new_state().unwrap();
    // SAFETY: `raw_fail` follows the Lua C function protocol.
    let raw = unsafe { lua.create_c_function(raw_fail) }.unwrap();
    lua.globals().set("raw", raw).unwrap();
    let (ok, msg): (bool, String) = lua
        .load("local ok, e = pcall(raw) return ok, tostring(e)")
        .eval()
        .unwrap();
    assert!(!ok);
    assert!(msg.contains("raw"), "{msg}");
    assert_eq!(RAW_DROPS.load(Ordering::SeqCst), 0);
}
