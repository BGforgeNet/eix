//! The script engine: its keyword table, compiled commands (`SSCCmd`), variables and the zone's script (`CScript`).
//!
//! Names follow ei-rebuild.
#![expect(
    unsafe_code,
    reason = "calls into game.exe's script code and resizing its argument arrays"
)]

use core::ffi::{CStr, c_void};
use core::mem::offset_of;

use super::mfc::{self, GameArray, GameString};
use super::object::EngineObject;
use super::{Global, code};

/// One entry of the script keyword table.
///
/// `KeywordName` 0x4D0BA0 walks the table by 0x10 to a null name, comparing `id`; `FunctionSignature` 0x4D0B10 reads
/// `function`, and 0x4D41C0 takes a command's parameter types from `command`, else from `function` past its result
/// type.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptKeyword {
    /// The keyword; null ends the table.
    pub name: *const u8,
    /// The keyword's id ([`ScriptCommand::id`]).
    pub id: i32,
    /// A function's result and parameter types, else null.
    pub function: *const u8,
    /// A command's parameter types, else null.
    pub command: *const u8,
}

const _: () = {
    assert!(offset_of!(ScriptKeyword, name) == 0x0);
    assert!(offset_of!(ScriptKeyword, id) == 0x4);
    assert!(offset_of!(ScriptKeyword, function) == 0x8);
    assert!(offset_of!(ScriptKeyword, command) == 0xC);
    assert!(size_of::<ScriptKeyword>() == 0x10);
};

/// The first entry of the game's keyword table, which the parser reads in place (0x4D0B10, 0x4D0BA0).
pub const SCRIPT_KEYWORDS: Global<ScriptKeyword> = Global::at(eix_sites::K_SCRIPT_KEYWORDS);

/// A counted reference to a world object: `ScriptObjectRef::Assign` 0x4B24D0 releases the old object and holds the
/// new one in its one word.
#[repr(C)]
#[derive(Debug)]
pub struct ScriptObjectRef {
    /// The object, or null.
    pub object: *mut EngineObject,
}

const _: () = {
    assert!(offset_of!(ScriptObjectRef, object) == 0x0);
    assert!(size_of::<ScriptObjectRef>() == 4);
};

/// A script group: an MFC array of object references (`CArray<ScriptObjectRef>::SetSize` 0x4C2050).
pub type ScriptGroup = GameArray<ScriptObjectRef>;

/// A set of named variables, one value array and one name array per type: a script's locals and the global
/// variables. `SSCCmd::SetParameters` 0x4B3F60 binds arguments to the values at +0x0, +0x28, +0x50 and +0x78.
#[repr(C)]
#[derive(Debug)]
pub struct ScriptVars {
    /// Float values (0x4B3F60 reads +0x4).
    pub floats: GameArray<f32>,
    /// Their names.
    pub float_names: GameArray<GameString>,
    /// String values (0x4B3F60 copies from +0x28).
    pub strings: GameArray<GameString>,
    /// Their names.
    pub string_names: GameArray<GameString>,
    /// Object values (0x4B3F60 takes the address of an element at +0x54, 4 bytes each).
    pub objects: GameArray<ScriptObjectRef>,
    /// Their names.
    pub object_names: GameArray<GameString>,
    /// Group values (0x4B3F60 takes the address of an element at +0x7C, 0x14 bytes each).
    pub groups: GameArray<ScriptGroup>,
    /// Their names.
    pub group_names: GameArray<GameString>,
}

const _: () = {
    assert!(offset_of!(ScriptVars, floats) == 0x0);
    assert!(offset_of!(ScriptVars, float_names) == 0x14);
    assert!(offset_of!(ScriptVars, strings) == 0x28);
    assert!(offset_of!(ScriptVars, string_names) == 0x3C);
    assert!(offset_of!(ScriptVars, objects) == 0x50);
    assert!(offset_of!(ScriptVars, object_names) == 0x64);
    assert!(offset_of!(ScriptVars, groups) == 0x78);
    assert!(offset_of!(ScriptVars, group_names) == 0x8C);
    assert!(size_of::<ScriptVars>() == 0xA0);
};

/// A compiled command or function call (`SSCCmd`). `CScript::LoadFunction` 0x4CFBC0 allocates 0x94 bytes for one
/// (0x4CFC35) and constructs it with `SSCCmd::Construct` 0x4D0A50, which sets up its seven arrays.
///
/// The argument arrays are what `SSCCmd::SetParameters` 0x4B3F60 binds variables into: floats and strings by value,
/// objects and groups as the address of the variable.
#[repr(C)]
#[derive(Debug)]
pub struct ScriptCommand {
    /// The keyword id: `SSCCmd::CalcFloat` 0x4B41E0 dispatches on it.
    pub id: i32,
    /// The float arguments (0x4B3F60 writes +0x8).
    pub float_args: GameArray<f32>,
    /// The string arguments (0x4B3F60 copies into +0x1C).
    pub string_args: GameArray<GameString>,
    /// The object arguments (0x4B3F60 writes +0x30).
    pub object_args: GameArray<*mut ScriptObjectRef>,
    /// The group arguments (0x4B3F60 writes +0x44).
    pub group_args: GameArray<*mut ScriptGroup>,
    _unknown54: [u8; 0x94 - 0x54],
}

const _: () = {
    assert!(offset_of!(ScriptCommand, id) == 0x0);
    assert!(offset_of!(ScriptCommand, float_args) == 0x4);
    assert!(offset_of!(ScriptCommand, string_args) == 0x18);
    assert!(offset_of!(ScriptCommand, object_args) == 0x2C);
    assert!(offset_of!(ScriptCommand, group_args) == 0x40);
    assert!(offset_of!(ScriptCommand, _unknown54) == 0x54);
    assert!(size_of::<ScriptCommand>() == 0x94);
};

/// `CScript`: a zone's script (blocks, definitions, globals, running instances).
#[repr(C)]
#[derive(Debug)]
pub struct Script {
    _unknown0: [u8; 0xA0],
    /// The global variables: `SSCCmd::CalcFloat` 0x4B41E0 binds the global bindings to +0xA0.
    pub globals: ScriptVars,
    _unknown140: [u8; 0x1EC - 0x140],
    /// The last load error's text: `ScriptError` 0x4CF470 assigns it.
    pub error_text: GameString,
    /// The last load error's code (0x4CF470).
    pub error_code: i32,
    /// The last load error's line (0x4CF470).
    pub error_line: i32,
}

const _: () = {
    assert!(offset_of!(Script, _unknown0) == 0x0);
    assert!(offset_of!(Script, globals) == 0xA0);
    assert!(offset_of!(Script, _unknown140) == 0x140);
    assert!(offset_of!(Script, error_text) == 0x1EC);
    assert!(offset_of!(Script, error_code) == 0x1F0);
    assert!(offset_of!(Script, error_line) == 0x1F4);
    assert!(size_of::<Script>() == 0x1F8);
};

/// The zone's script, the server world's +0x12874: `CWorldServer::Update` runs it with `ScriptTick` 0x4BF000
/// (0x58DF99).
pub const MISSION_SCRIPT: Global<Script> = Global::at(eix_sites::DATA_7C27D4);

impl Script {
    /// Loads a script file into the script: `CScript::LoadFile` 0x4D36F0 (thiscall, the path on the stack, `ret 4`);
    /// false on an error, which [`error_text`](Self::error_text) and [`error_line`](Self::error_line) then hold.
    ///
    /// # Safety
    /// The process must be a supported game.exe, on the game's thread; `this` must be a constructed script.
    pub unsafe fn load_file(this: *mut Self, path: &CStr) -> bool {
        // SAFETY: 0x4D36F0 is `int __thiscall (const char*)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *const u8) -> i32>(
                eix_sites::C_SCRIPT_LOAD_FILE,
            )
        };
        // SAFETY: the caller guarantees the script and the thread; `path` is NUL-terminated.
        unsafe { f(this, path.as_ptr().cast()) != 0 }
    }

    /// Runs the script's waiting commands once: `ScriptTick` 0x4BF000 (thiscall, plain `ret`).
    ///
    /// # Safety
    /// As for [`load_file`](Self::load_file).
    pub unsafe fn tick(this: *mut Self) {
        // SAFETY: 0x4BF000 is `void __thiscall ()`.
        let f = unsafe { code::<unsafe extern "thiscall" fn(*mut Self)>(eix_sites::SCRIPT_TICK) };
        // SAFETY: the caller guarantees the script and the thread.
        unsafe { f(this) }
    }

    /// Constructs an empty script in place: `CScript::Construct` 0x4BDE40 (thiscall, plain `ret`), which the server
    /// world's constructor runs on its two embedded scripts (0x589F6A, 0x589F7A).
    ///
    /// # Safety
    /// The process must be a supported game.exe, on the game's thread; `this` must be writable for at least
    /// `size_of::<Script>()` bytes and hold no constructed script.
    pub unsafe fn construct(this: *mut Self) {
        // SAFETY: 0x4BDE40 is `CScript* __thiscall ()`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self) -> *mut Self>(
                eix_sites::C_SCRIPT_CONSTRUCT,
            )
        };
        // SAFETY: the caller guarantees the memory and the thread.
        unsafe { f(this) };
    }

    /// Empties the script before a new load: `ScriptReset` 0x4BE6E0 (thiscall, plain `ret`).
    ///
    /// # Safety
    /// As for [`load_file`](Self::load_file).
    pub unsafe fn reset(this: *mut Self) {
        // SAFETY: 0x4BE6E0 is `void __thiscall ()`.
        let f = unsafe { code::<unsafe extern "thiscall" fn(*mut Self)>(eix_sites::SCRIPT_RESET) };
        // SAFETY: the caller guarantees the script and the thread.
        unsafe { f(this) }
    }

    /// Compiles script `text` into the script: `CScript::Load` 0x4D23A0 (thiscall, two stack arguments, `ret 8`).
    /// It first sets [`error_text`](Self::error_text) to "No errors found" (0x78DA0C) and clears the error code and
    /// line (0x4D243A-0x4D2459); a compile error sets them. `flag` is the second argument, which the addon's `check`
    /// passes as 0.
    ///
    /// # Safety
    /// As for [`load_file`](Self::load_file); `text` must be a constructed game string.
    pub unsafe fn load_text(this: *mut Self, text: *const GameString, flag: i32) -> i32 {
        // SAFETY: 0x4D23A0 is `int __thiscall (const CString*, int)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *const GameString, i32) -> i32>(
                eix_sites::C_SCRIPT_LOAD,
            )
        };
        // SAFETY: the caller guarantees the script, the string and the thread.
        unsafe { f(this, text, flag) }
    }

    /// Destroys the script's contents, leaving its memory to the caller: `CScript::Destruct` 0x58A140 (thiscall,
    /// plain `ret`).
    ///
    /// # Safety
    /// As for [`load_file`](Self::load_file); the script is not used afterwards.
    pub unsafe fn destruct(this: *mut Self) {
        // SAFETY: 0x58A140 is `void __thiscall ()`.
        let f =
            unsafe { code::<unsafe extern "thiscall" fn(*mut Self)>(eix_sites::C_SCRIPT_DESTRUCT) };
        // SAFETY: the caller guarantees the script and the thread.
        unsafe { f(this) }
    }
}

impl ScriptObjectRef {
    /// Holds `object` (null for none) in place of the current one: `ScriptObjectRef::Assign` 0x4B24D0 (thiscall,
    /// `ret 4`).
    ///
    /// # Safety
    /// The process must be a supported game.exe, on the game's thread; `this` must be a live reference and `object` null
    /// or a live engine object.
    pub unsafe fn assign(this: *mut Self, object: *mut EngineObject) {
        // SAFETY: 0x4B24D0 is `void __thiscall (CEngineObject*)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *mut EngineObject)>(
                eix_sites::SCRIPT_OBJECT_REF_ASSIGN,
            )
        };
        // SAFETY: the caller guarantees the reference, the object and the thread.
        unsafe { f(this, object) }
    }
}

/// Adds `object` to `group` unless null or present, keeping it sorted: `ScriptGroup::Insert` 0x4B2EF0 (thiscall,
/// `ret 4`).
///
/// # Safety
/// As for [`ScriptObjectRef::assign`]; `group` must be a live group.
pub unsafe fn group_insert(group: *mut ScriptGroup, object: *mut EngineObject) {
    // SAFETY: 0x4B2EF0 is `void __thiscall (CEngineObject*)`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut ScriptGroup, *mut EngineObject)>(
            eix_sites::SCRIPT_GROUP_INSERT,
        )
    };
    // SAFETY: the caller guarantees the group, the object and the thread.
    unsafe { f(group, object) }
}

/// Empties `group`, releasing its references: `CArray<ScriptObjectRef>::SetSize(0, -1)` 0x4C2050 (thiscall, `ret 8`).
///
/// # Safety
/// As for [`group_insert`].
pub unsafe fn group_clear(group: *mut ScriptGroup) {
    // SAFETY: 0x4C2050 is MFC's `void __thiscall SetSize(int nNewSize, int nGrowBy)`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut ScriptGroup, i32, i32)>(
            eix_sites::OBJECT_REFS_SET_SIZE,
        )
    };
    // SAFETY: the caller guarantees the group and the thread.
    unsafe { f(group, 0, -1) }
}

/// Makes room for `size` elements of `array`, as the addon's own `SetSize` copies do: a shrink to 0 frees the block;
/// a growth past the capacity allocates `max(len + grow, size)` elements from the game's heap (grow is the array's
/// grow-by, else 4) and moves the elements over. Returns the elements from the old length on, to fill.
///
/// # Safety
/// The process must be a supported game.exe; `array` must be a constructed array whose block comes from the game's
/// allocator, of elements that may be moved bytewise.
unsafe fn reserve<T>(array: *mut GameArray<T>, size: i32) -> Option<(*mut T, usize)> {
    // SAFETY: the caller guarantees a constructed array.
    let a = unsafe { &mut *array };
    let size = size.max(0);
    if size <= a.len {
        if size == 0 {
            // SAFETY: the block is the game's (or null), not used after this.
            unsafe { mfc::delete(a.data.cast::<c_void>()) };
            a.data = core::ptr::null_mut();
            a.capacity = 0;
        }
        return None;
    }
    let old = usize::try_from(a.len).unwrap_or(0);
    if a.capacity < size {
        let grow = if a.grow_by != 0 { a.grow_by } else { 4 };
        let capacity = a.len.saturating_add(grow).max(size);
        let bytes = usize::try_from(capacity).unwrap_or(0) * size_of::<T>();
        // SAFETY: the game's allocator; the block is freed only by the game's `delete`.
        let block = unsafe { mfc::new(bytes) }.cast::<T>();
        if block.is_null() {
            return None;
        }
        if !a.data.is_null() {
            // SAFETY: the old block holds `old` elements; the new one has room for more.
            unsafe { core::ptr::copy_nonoverlapping(a.data, block, old) };
        }
        // SAFETY: the old block came from the game's allocator; its elements were moved out.
        unsafe { mfc::delete(a.data.cast::<c_void>()) };
        a.data = block;
        a.capacity = capacity;
    }
    let new = usize::try_from(size).unwrap_or(0) - old;
    // SAFETY: the block has room for `size` elements.
    let tail = unsafe { a.data.add(old) };
    // SAFETY: the tail is the block's own, `new` elements long.
    unsafe { core::ptr::write_bytes(tail, 0, new) };
    Some((tail, new))
}

/// Sets the length of a float array to `size`; new elements are 0.0.
///
/// # Safety
/// A constructed game array, on the game's thread.
pub unsafe fn resize_floats(array: *mut GameArray<f32>, size: i32) {
    // SAFETY: passed on; zeroed bytes are 0.0.
    let grew = unsafe { reserve(array, size) }.is_some();
    // SAFETY: the caller guarantees the array.
    finish_resize(unsafe { &mut *array }, size, grew);
}

/// Sets the length of a pointer array to `size`; new elements are null.
///
/// # Safety
/// As for [`resize_floats`].
pub unsafe fn resize_pointers<T>(array: *mut GameArray<*mut T>, size: i32) {
    // SAFETY: passed on; zeroed bytes are null pointers.
    let grew = unsafe { reserve(array, size) }.is_some();
    // SAFETY: the caller guarantees the array.
    finish_resize(unsafe { &mut *array }, size, grew);
}

/// Sets the length of a string array to `size`: dropped strings are destroyed, new ones are empty.
///
/// # Safety
/// As for [`resize_floats`]; the strings are constructed game strings.
pub unsafe fn resize_strings(array: *mut GameArray<GameString>, size: i32) {
    let size = size.max(0);
    // SAFETY: the caller guarantees a constructed array.
    let a = unsafe { &*array };
    let (len, data) = (a.len, a.data);
    for i in size..len {
        let at = data.wrapping_add(usize::try_from(i).unwrap_or(0));
        // SAFETY: index `i` is below the length, a constructed string not used again.
        unsafe { mfc::destroy(at) };
    }
    // SAFETY: passed on.
    let growth = unsafe { reserve(array, size) };
    if let Some((tail, new)) = growth {
        for i in 0..new {
            // SAFETY: the new slots are writable and hold no string.
            unsafe { mfc::construct_empty(tail.wrapping_add(i)) };
        }
    }
    // SAFETY: the caller guarantees the array.
    finish_resize(unsafe { &mut *array }, size, growth.is_some());
}

fn finish_resize<T>(array: &mut GameArray<T>, size: i32, grew: bool) {
    let size = size.max(0);
    if grew || size <= array.len {
        array.len = size;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_growth_keeps_the_allocated_length() {
        let mut data = [1.0f32, 2.0, 9.0];
        let mut array = GameArray {
            vtable: core::ptr::null(),
            data: data.as_mut_ptr(),
            len: 2,
            capacity: 3,
            grow_by: 0,
        };
        // A failed reserve leaves the old block and counts intact.
        finish_resize(&mut array, 4, false);
        assert_eq!(array.len, 2);
        assert_eq!(array.capacity, 3);
        assert_eq!(data, [1.0, 2.0, 9.0]);
        // SAFETY: the array holds two elements and has capacity for three; no allocation is needed.
        unsafe { resize_floats(&raw mut array, 3) };
        assert_eq!(array.len, 3);
        assert_eq!(data, [1.0, 2.0, 0.0]);
        // SAFETY: a nonzero shrink keeps the same valid allocation.
        unsafe { resize_floats(&raw mut array, 1) };
        assert_eq!(array.len, 1);
    }
}
