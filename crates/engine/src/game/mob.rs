//! The object records a mob file holds (`Spec` and its kinds), as the mob reader handles them.
//!
//! Names follow ei-rebuild.
#![expect(
    unsafe_code,
    reason = "virtual calls into game.exe's mob records and updating their ID table"
)]

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
    assert!(offset_of!(MobSpec, vtable) == 0x0);
    assert!(offset_of!(MobSpec, _unknown4) == 0x4);
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
    assert!(offset_of!(MobSpecVtable, _slots0) == 0x0);
    assert!(offset_of!(MobSpecVtable, delete) == 0x28);
};

impl MobSpec {
    /// Destroys the record and frees it, as `LoadMobFile` does once a record has made its object.
    ///
    /// # Safety
    /// `this` must be a live record of a supported game.exe that nothing else refers to, on the game's thread.
    pub unsafe fn delete(this: *mut Self) {
        // SAFETY: a live record's first word is its vtable.
        let vtable = unsafe { (*this).vtable };
        // SAFETY: every record vtable has slot 10, a thiscall taking the delete flag.
        let delete = unsafe { (*vtable).delete };
        // SAFETY: the caller guarantees the record, that it is not referred to, and the thread.
        unsafe { delete(this, 1) };
    }
}

/// A node in the mob reader's ID table.
#[repr(C)]
#[derive(Debug)]
struct MobNode {
    next: *mut MobNode,
    id: i32,
    spec: *mut MobSpec,
}

const _: () = {
    assert!(offset_of!(MobNode, next) == 0);
    assert!(offset_of!(MobNode, id) == 4);
    assert!(offset_of!(MobNode, spec) == 8);
};

/// The mob reader's table of records awaiting insertion into the world, hashed by unsigned ID modulo bucket count.
#[repr(C)]
#[derive(Debug)]
pub struct MobTable {
    _unknown0: u32,
    buckets: *mut *mut MobNode,
    buckets_end: *mut *mut MobNode,
    _unknown_c: [u8; 8],
    max_id: i32,
}

const _: () = {
    assert!(offset_of!(MobTable, _unknown0) == 0);
    assert!(offset_of!(MobTable, buckets) == 4);
    assert!(offset_of!(MobTable, buckets_end) == 8);
    assert!(offset_of!(MobTable, _unknown_c) == 0xC);
    assert!(offset_of!(MobTable, max_id) == 0x14);
};

impl MobTable {
    /// Moves a record to an unused ID, keeping its hash chain and the record's ID consistent.
    ///
    /// # Safety
    /// `table` must be the live mob reader's table, with a record at `old` and none at `new`, on the game's thread.
    pub unsafe fn reassign(table: *mut Self, old: i32, new: i32) {
        // SAFETY: the caller owns the live reader's table.
        let table = unsafe { &mut *table };
        // SAFETY: both pointers delimit the table's allocated bucket array.
        let count = unsafe { table.buckets_end.offset_from(table.buckets) } as usize;
        // SAFETY: a populated table has buckets, and the remainder selects one of them.
        let mut link = unsafe { table.buckets.add(old as u32 as usize % count) };
        // SAFETY: `link` is a bucket slot or a node's next pointer in this table.
        let mut node = unsafe { *link };
        // SAFETY: the caller guarantees that the chain contains `old`.
        while unsafe { (*node).id } != old {
            // SAFETY: a live node in the chain that precedes `old`.
            link = unsafe { &raw mut (*node).next };
            // SAFETY: the next slot of that live node.
            node = unsafe { *link };
        }
        // SAFETY: the found node is live and belongs to `link`'s chain.
        let node_ref = unsafe { &mut *node };
        // SAFETY: `link` is the slot pointing at this node, separate from its fields.
        unsafe { *link = node_ref.next };
        // SAFETY: the remainder selects a bucket in the same table.
        let destination = unsafe { table.buckets.add(new as u32 as usize % count) };
        // SAFETY: the destination bucket is a live slot, now without this node even if it is the same bucket.
        node_ref.next = unsafe { *destination };
        node_ref.id = new;
        // SAFETY: every node owns its live record, which has not yet entered the world.
        unsafe { (*node_ref.spec).id = new };
        // SAFETY: the destination is a live bucket slot.
        unsafe { *destination = node };
        table.max_id = table.max_id.max(new);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr::null_mut;

    fn spec(id: i32) -> MobSpec {
        MobSpec {
            vtable: core::ptr::null(),
            _unknown4: [0; 0x10],
            parent: null_mut(),
            id,
        }
    }

    #[test]
    fn reassigned_records_keep_both_hash_chains_and_their_ids() {
        let mut anonymous = spec(100);
        let mut first_spec = spec(103);
        let mut last_spec = spec(106);
        let mut other_spec = spec(104);
        let mut last = MobNode {
            next: null_mut(),
            id: 106,
            spec: &raw mut last_spec,
        };
        let mut moving = MobNode {
            next: &raw mut last,
            id: 100,
            spec: &raw mut anonymous,
        };
        let mut first = MobNode {
            next: &raw mut moving,
            id: 103,
            spec: &raw mut first_spec,
        };
        let mut other = MobNode {
            next: null_mut(),
            id: 104,
            spec: &raw mut other_spec,
        };
        let mut buckets = [null_mut(), &raw mut first, &raw mut other];
        let range = buckets.as_mut_ptr_range();
        let mut table = MobTable {
            _unknown0: 0,
            buckets: range.start,
            buckets_end: range.end,
            _unknown_c: [0; 8],
            max_id: 106,
        };
        // SAFETY: private live table with ID 100, no ID 101, and all nodes and records live through the call.
        unsafe { MobTable::reassign(&raw mut table, 100, 101) };
        assert_eq!(buckets, [null_mut(), &raw mut first, &raw mut moving]);
        assert_eq!(first.next, &raw mut last);
        assert_eq!(moving.next, &raw mut other);
        assert_eq!((moving.id, anonymous.id, table.max_id), (101, 101, 106));
        // SAFETY: the same table now has ID 101 and no ID 107; both hash to the same bucket.
        unsafe { MobTable::reassign(&raw mut table, 101, 107) };
        assert_eq!(buckets[2], &raw mut moving);
        assert_eq!(moving.next, &raw mut other);
        assert_eq!((moving.id, anonymous.id, table.max_id), (107, 107, 107));
        // SAFETY: the same table now has ID 107 and no ID 108.
        unsafe { MobTable::reassign(&raw mut table, 107, 108) };
        assert_eq!(buckets, [&raw mut moving, &raw mut first, &raw mut other]);
        assert!(moving.next.is_null());
        assert_eq!((moving.id, anonymous.id, table.max_id), (108, 108, 108));
        assert_eq!((first.id, first_spec.id), (103, 103));
        assert_eq!((last.id, last_spec.id), (106, 106));
        assert_eq!((other.id, other_spec.id), (104, 104));
    }
}
