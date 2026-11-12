//! MultipleLoadingMobs: a mob file loaded into a zone that already has objects skips the records whose ids the world
//! holds, and gives records without an id (-1) a free one from 100 up.
#![expect(
    unsafe_code,
    reason = "reading the mob record at the hooked site and asking the server world for ids"
)]

use std::collections::BTreeSet;
use std::sync::{Mutex, PoisonError};

use eix_core::rules::free_id;
use eix_engine::game::mob::{MobSpec, MobTable};
use eix_engine::game::world::{SERVER_WORLD, World};
use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use super::memory::word;
use super::{BuildError, Labelled, Setup};

/// Where `ReadMobChunks` goes on with the next record once one is filed (it clears ESI first).
const NEXT_RECORD: Site = eix_sites::READ_MOB_CHUNKS__2B8;

/// IDs in the file being read, before its records enter the world; reset at the reader's entry and return.
static FILE_IDS: Mutex<FileIds> = Mutex::new(FileIds::new());

struct FileIds {
    used: BTreeSet<i32>,
    anonymous: BTreeSet<i32>,
}

impl FileIds {
    const fn new() -> Self {
        Self {
            used: BTreeSet::new(),
            anonymous: BTreeSet::new(),
        }
    }

    fn assign(
        &mut self,
        id: i32,
        mut taken: impl FnMut(i32) -> bool,
        reassign: impl FnOnce(i32, i32),
    ) -> i32 {
        if id != -1 {
            if self.anonymous.remove(&id) {
                let new = free_id(|n| taken(n) || self.used.contains(&n));
                reassign(id, new);
                self.used.insert(new);
                self.anonymous.insert(new);
            }
            self.used.insert(id);
            return id;
        }
        let id = free_id(|n| taken(n) || self.used.contains(&n));
        self.used.insert(id);
        self.anonymous.insert(id);
        id
    }
}

extern "C" fn file_boundary(_: usize, _: &mut Context) {
    clear_file();
}

fn clear_file() {
    *FILE_IDS.lock().unwrap_or_else(PoisonError::into_inner) = FileIds::new();
}

/// Whether the server world has an object with `id`.
fn taken(id: i32) -> bool {
    // SAFETY: mob files load on the game's thread into the constructed server world.
    !unsafe { World::find_object(SERVER_WORLD.as_ptr(), id) }.is_null()
}

/// At 0x48213A, after a record (ESI) was read and before it is filed in the table at [EBP+8].
extern "C" fn record(_: usize, ctx: &mut Context) {
    let spec = core::ptr::with_exposed_provenance_mut::<MobSpec>(ctx.esi as usize);
    // SAFETY: at 0x48213A ESI is the record just read, owned by the reader.
    let id = unsafe { (*spec).id };
    if id != -1 && taken(id) {
        // The original leaves the record allocated; nothing refers to it, so it is freed.
        // SAFETY: the reader drops its only pointer to the record at NEXT_RECORD.
        unsafe { MobSpec::delete(spec) };
        ctx.resume = NEXT_RECORD;
        return;
    }
    // The replaced `mov edi, [ebp+8]; mov eax, [esi+0x18]`.
    // SAFETY: EBP is ReadMobChunks' frame; its argument at +8 is the record table.
    ctx.edi = unsafe { word(ctx.ebp.wrapping_add(8) as usize) };
    let table = core::ptr::with_exposed_provenance_mut::<MobTable>(ctx.edi as usize);
    let id = FILE_IDS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .assign(id, taken, |old, new| {
            // SAFETY: the earlier anonymous record is already in this reader's table; `new` is unused.
            unsafe { MobTable::reassign(table, old, new) };
        });
    // SAFETY: the record just read is owned by this reader.
    unsafe { (*spec).id = id };
    ctx.eax = id as u32;
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![
        Labelled {
            label: "MultipleLoadingMobs #0".to_owned(),
            write: hook::context_hook(
                eix_sites::READ_MOB_CHUNKS__23A,
                &[0x8B, 0x7D, 0x08, 0x8B, 0x46, 0x18],
                Mode::Drop,
                record,
                0,
            )?,
        },
        Labelled {
            label: "MultipleLoadingMobs #1".to_owned(),
            write: hook::context_hook(
                eix_sites::READ_MOB_CHUNKS,
                &[0x55, 0x8B, 0xEC, 0x6A, 0xFF],
                Mode::Before,
                file_boundary,
                0,
            )?,
        },
        Labelled {
            label: "MultipleLoadingMobs #2".to_owned(),
            write: hook::context_hook(
                eix_sites::READ_MOB_CHUNKS__519,
                &[0x8B, 0x4D, 0xF4, 0x8A, 0x45, 0xF3],
                Mode::Before,
                file_boundary,
                0,
            )?,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_explicit_id_waiting_for_the_world_is_reserved() {
        let mut ids = FileIds::new();
        assert_eq!(ids.assign(100, |_| false, |_, _| unreachable!()), 100);
        assert_eq!(ids.assign(-1, |_| false, |_, _| unreachable!()), 101);
        assert_eq!(ids.assign(-1, |id| id == 102, |_, _| unreachable!()), 103);
    }

    #[test]
    fn a_later_explicit_id_moves_the_earlier_anonymous_record() {
        let mut ids = FileIds::new();
        let mut moves = Vec::new();
        assert_eq!(ids.assign(-1, |_| false, |_, _| unreachable!()), 100);
        assert_eq!(
            ids.assign(100, |id| id == 101, |old, new| moves.push((old, new))),
            100
        );
        assert_eq!(moves, [(100, 102)]);
        assert_eq!(ids.assign(-1, |_| false, |_, _| unreachable!()), 101);
        assert_eq!(ids.assign(-1, |_| false, |_, _| unreachable!()), 103);
    }

    #[test]
    fn the_file_boundary_releases_explicit_and_assigned_ids() {
        {
            let mut ids = FILE_IDS.lock().unwrap();
            ids.assign(100, |_| false, |_, _| unreachable!());
            ids.assign(-1, |_| false, |_, _| unreachable!());
            assert_eq!(ids.used.len(), 2);
        }
        clear_file();
        let mut ids = FILE_IDS.lock().unwrap();
        assert!(ids.used.is_empty());
        assert!(ids.anonymous.is_empty());
        assert_eq!(ids.assign(-1, |_| false, |_, _| unreachable!()), 100);
        drop(ids);
        clear_file();
    }
}
