//! The object records a mob file holds (`Spec` and its kinds), as the mob reader handles them.
//!
//! Names follow ei-rebuild.
#![expect(unsafe_code, reason = "virtual calls into game.exe's mob records")]

use core::ffi::c_void;
use core::mem::offset_of;

/// The head of every object record: a chunk reader (vtable, header, the parent chunk at +0x14), then the record's
/// id. Each kind has its own size; never held by value.
#[repr(C)]
#[derive(Debug)]
pub struct MobSpec {
    /// The vtable: slot 10 is the deleting destructor ([`delete`](Self::delete)).
    pub vtable: *const MobSpecVtable,
    _unknown4: [u8; 0x14 - 4],
    /// The chunk being read from: `ReadMobChunks` 0x482132 points it at its own chunk before reading the record.
    pub parent: *mut c_void,
    /// The object's id, -1 for none: `ReadMobChunks` 0x48213D files the record under it.
    pub id: i32,
}

const _: () = {
    assert!(offset_of!(MobSpec, parent) == 0x14);
    assert!(offset_of!(MobSpec, id) == 0x18);
};

/// The record vtable, as far as the destructor.
#[repr(C)]
#[derive(Debug)]
pub struct MobSpecVtable {
    _slots0: [*const c_void; 10],
    /// Slot 10: the deleting destructor; `LoadMobFile` 0x481CD7 deletes each record with it (argument 1).
    pub delete: unsafe extern "thiscall" fn(*mut MobSpec, u32) -> *mut c_void,
}

const _: () = {
    assert!(offset_of!(MobSpecVtable, delete) == 0x28);
};

impl MobSpec {
    /// Destroys the record and frees it, as `LoadMobFile` does once a record has made its object.
    ///
    /// # Safety
    /// `this` must be a live record of game.exe RU 1.06 that nothing else refers to, on the game's thread.
    pub unsafe fn delete(this: *mut Self) {
        // SAFETY: a live record's first word is its vtable.
        let vtable = unsafe { (*this).vtable };
        // SAFETY: every record vtable has slot 10, a thiscall taking the delete flag.
        let delete = unsafe { (*vtable).delete };
        // SAFETY: the caller guarantees the record, that it is not referred to, and the thread.
        unsafe { delete(this, 1) };
    }
}
