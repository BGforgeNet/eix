//! The engine's object base (`CEngineObject`), its transform-object extension and their vector types.
//!
//! Names follow ei-rebuild.
#![expect(unsafe_code, reason = "virtual calls into game.exe objects")]

use core::ffi::c_void;
use core::mem::offset_of;

/// Three floats: a position or a direction.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vector3 {
    /// X.
    pub x: f32,
    /// Y.
    pub y: f32,
    /// Z (up).
    pub z: f32,
}

/// A rotation, `w` first: `CTransformObject::ConstructBase` 0x453400 stores 1.0 at the first word of each rotation.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Quaternion {
    /// W.
    pub w: f32,
    /// X.
    pub x: f32,
    /// Y.
    pub y: f32,
    /// Z.
    pub z: f32,
}

const _: () = {
    assert!(size_of::<Vector3>() == 0xC);
    assert!(size_of::<Quaternion>() == 0x10);
};

/// The first slots of every engine object's vtable.
#[repr(C)]
#[derive(Debug)]
pub struct EngineObjectVtable {
    _slots0: [*const c_void; 4],
    /// Slot 4: drops one reference; at zero the owning manager releases the object (`Release` 0x455980, the slot in
    /// the item vtable 0x743DA4).
    pub release: unsafe extern "thiscall" fn(*mut EngineObject),
    _slots5: [*const c_void; 6],
    /// Slot 11: the class's name (ei-rebuild's `EngineObject::ClassName`, the slot before the class id).
    pub class_name: unsafe extern "thiscall" fn(*mut EngineObject) -> *const u8,
    /// Slot 12: the class id; `CWorld::FindUnit` 0x55FB00 keeps an object whose slot 12 returns [`CLASS_UNIT`].
    pub class_id: unsafe extern "thiscall" fn(*mut EngineObject) -> u32,
}

const _: () = {
    assert!(offset_of!(EngineObjectVtable, release) == 0x10);
    assert!(offset_of!(EngineObjectVtable, class_name) == 0x2C);
    assert!(offset_of!(EngineObjectVtable, class_id) == 0x30);
};

/// [`EngineObjectVtable::class_id`] of a unit (`CUnit`): `CWorld::FindUnit` 0x55FB00.
pub const CLASS_UNIT: u32 = 0x50;
/// [`EngineObjectVtable::class_id`] of a unit's client controller (`CUnitClientSpecific`): `CUnit::GetWeapon`
/// 0x5318BE reads a player's items for a unit whose controller has it.
pub const CLASS_UNIT_CLIENT_SPECIFIC: u32 = 0x3D;
/// [`EngineObjectVtable::class_id`] of a map object's server half (`CMapObjectServer`): `CMapObject::PackVitals`
/// 0x50E970 packs the vitals only from such a companion.
pub const CLASS_MAP_OBJECT_SERVER: u32 = 0x39;

impl EngineObject {
    /// Drops one reference through vtable slot 4, as `UnitAction::Reset` 0x54B245 releases its target.
    ///
    /// # Safety
    /// `this` must point at a live engine object of a supported game.exe holding a reference the caller owns, on the
    /// game's thread; the object may be gone afterwards.
    pub unsafe fn release(this: *mut Self) {
        // SAFETY: a live object's first word is its vtable.
        let vtable = unsafe { (*this).vtable };
        // SAFETY: every engine object's vtable has slot 4, a thiscall without arguments.
        let release = unsafe { (*vtable).release };
        // SAFETY: the caller guarantees the object, the reference and the thread.
        unsafe { release(this) }
    }
}

/// `CEngineObject`: the base of units, items, spells, managers and the rest.
///
/// The fields as `CTransformObject::ConstructBase` 0x453400 clears them (refs, id -1, prototype, type, flags,
/// owner) and `CEngineObject::CopyFromImpl` 0x402D40 copies them (flags, type, prototype).
#[repr(C)]
#[derive(Debug)]
pub struct EngineObject {
    /// The vtable.
    pub vtable: *const EngineObjectVtable,
    /// Reference count: `Release` 0x455980 decrements it.
    pub refs: i32,
    /// Flag bits: `CUnit::SetSide` 0x530120 sets 0x80000.
    pub flags: u32,
    /// The object's id, -1 until registered: a counted reference saves it (`AIRef::Save` 0x50E290).
    pub id: i32,
    /// The class-specific kind (an item's kind: `CCampPages::Price` 0x60FAB0 compares it with 0x3004 and 0x3005).
    pub kind: u32,
    /// The manager that owns the object: `CEngineObject::SetOwner` 0x455970; `Release` calls its slot 24.
    pub owner: *mut c_void,
    /// The database prototype; a spell keeps its type in the high word (`CSpell::IsOffensive` 0x68B770).
    pub prototype: u32,
}

const _: () = {
    assert!(offset_of!(EngineObject, vtable) == 0x0);
    assert!(offset_of!(EngineObject, refs) == 0x4);
    assert!(offset_of!(EngineObject, flags) == 0x8);
    assert!(offset_of!(EngineObject, id) == 0xC);
    assert!(offset_of!(EngineObject, kind) == 0x10);
    assert!(offset_of!(EngineObject, owner) == 0x14);
    assert!(offset_of!(EngineObject, prototype) == 0x18);
    assert!(size_of::<EngineObject>() == 0x1C);
};

impl EngineObject {
    /// The class id from vtable slot 12.
    ///
    /// # Safety
    /// `this` must point at a live engine object of a supported game.exe, on the game's thread.
    pub unsafe fn class_id(this: *mut Self) -> u32 {
        // SAFETY: a live object's first word is its vtable.
        let vtable = unsafe { (*this).vtable };
        // SAFETY: every engine object's vtable has slot 12, a thiscall without arguments.
        let class_id = unsafe { (*vtable).class_id };
        // SAFETY: the caller guarantees a live object on the game's thread.
        unsafe { class_id(this) }
    }

    /// The class's name from vtable slot 11, a static NUL-terminated string.
    ///
    /// # Safety
    /// As for [`class_id`](Self::class_id).
    pub unsafe fn class_name(this: *mut Self) -> *const u8 {
        // SAFETY: a live object's first word is its vtable.
        let vtable = unsafe { (*this).vtable };
        // SAFETY: every engine object's vtable has slot 11, a thiscall without arguments.
        let class_name = unsafe { (*vtable).class_name };
        // SAFETY: the caller guarantees a live object on the game's thread.
        unsafe { class_name(this) }
    }

    /// A map object's dead flag from vtable slot 55 (`UnitGetFlag274` 0x446880 for a unit: its +0x274).
    ///
    /// # Safety
    /// As for [`class_id`](Self::class_id); the object must be a map object, whose vtable has the slot.
    pub unsafe fn is_dead(this: *mut Self) -> i32 {
        // SAFETY: a live object's first word is its vtable.
        let vtable = unsafe { (*this).vtable }.cast::<usize>();
        // SAFETY: a map object's vtable has slot 55.
        let address = unsafe { *vtable.wrapping_add(55) };
        // SAFETY: slot 55 is `int __thiscall ()`.
        let f = unsafe {
            crate::hook::as_fn_at::<unsafe extern "thiscall" fn(*mut Self) -> i32>(address)
        };
        // SAFETY: the caller guarantees a live object on the game's thread.
        unsafe { f(this) }
    }
}

/// `CTransformObject`: an engine object with a position and a rotation (map objects, the camera).
///
/// `CTransformObject::CopyTransformImpl` 0x401490 copies the four blocks and `SaveTransformImpl` 0x401510 writes the
/// local position (12 bytes) and rotation (16 bytes).
#[repr(C)]
#[derive(Debug)]
pub struct TransformObject {
    /// The engine object.
    pub base: EngineObject,
    /// The position.
    pub position: Vector3,
    /// The position after the parent's transform.
    pub world_position: Vector3,
    /// The rotation.
    pub rotation: Quaternion,
    /// The rotation after the parent's transform.
    pub world_rotation: Quaternion,
}

const _: () = {
    assert!(offset_of!(TransformObject, position) == 0x1C);
    assert!(offset_of!(TransformObject, world_position) == 0x28);
    assert!(offset_of!(TransformObject, rotation) == 0x34);
    assert!(offset_of!(TransformObject, world_rotation) == 0x44);
    assert!(size_of::<TransformObject>() == 0x54);
};
