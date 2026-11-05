//! DiplomacySides: a side outside 0..=31 neither reads nor writes past the diplomacy tables. `CDiplomacy` holds two
//! 32-entry tables of masks (allies +0x00, hostiles +0x80); `GetRelation` 0x455630 and `SetRelation` 0x4554E0 index
//! them with the first side and test no range, so a script's `SetDiplomacy` or a .mob's record with such a side wrote
//! past them. `GetHostileMask` 0x455660 already answers -1 (hostile to all) outside the range; ours answers likewise:
//! `GetRelation` gives 2 (hostile), `SetRelation` changes nothing. The second side only selects a bit of a mask, so
//! it stays as the game takes it.
#![expect(
    unsafe_code,
    reason = "calls the game's diplomacy functions through their trampolines"
)]

use core::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use eix_engine::hook;

use super::Warnings;
use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// `mov eax, ecx; mov edx, 1`.
const GET_RELATION: usize = eix_sites::C_DIPLOMACY_GET_RELATION;
const GET_PROLOGUE: [u8; 7] = [0x8B, 0xC1, 0xBA, 0x01, 0x00, 0x00, 0x00];
/// `push -1; push 0x713C58`: its exception frame.
const SET_RELATION: usize = eix_sites::C_DIPLOMACY_SET_RELATION;
const SET_PROLOGUE: [u8; 7] = [0x6A, 0xFF, 0x68, 0x58, 0x3C, 0x71, 0x00];
/// The sides the tables have.
const SIDES: core::ops::Range<i32> = 0..32;
/// `GetRelation`'s answer for hostile.
const HOSTILE: u32 = 2;
type Get = unsafe extern "thiscall" fn(*mut c_void, i32, i32) -> u32;
type Set = unsafe extern "thiscall" fn(*mut c_void, i32, i32, i32, i32);

static GET: AtomicUsize = AtomicUsize::new(0);
static SET: AtomicUsize = AtomicUsize::new(0);
static WARNINGS: Warnings = Warnings::new("DiplomacySides");

extern "thiscall" fn get_relation(diplomacy: *mut c_void, side: i32, other: i32) -> u32 {
    if !SIDES.contains(&side) {
        WARNINGS.warn(|| format!("relation of side {side} to {other} asked: hostile"));
        return HOSTILE;
    }
    // SAFETY: GET holds the trampoline `build` made, of this type; the arguments are the game's.
    let original = unsafe { hook::as_fn::<Get>(GET.load(Ordering::Relaxed)) };
    // SAFETY: as above.
    unsafe { original(diplomacy, side, other) }
}

extern "thiscall" fn set_relation(
    diplomacy: *mut c_void,
    side: i32,
    other: i32,
    relation: i32,
    notify: i32,
) {
    if !SIDES.contains(&side) {
        WARNINGS.warn(|| format!("relation of side {side} to {other} set to {relation}: ignored"));
        return;
    }
    // SAFETY: SET holds the trampoline `build` made, of this type; the arguments are the game's.
    let original = unsafe { hook::as_fn::<Set>(SET.load(Ordering::Relaxed)) };
    // SAFETY: as above.
    unsafe { original(diplomacy, side, other, relation, notify) }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let (get, get_trampoline) = hook::replace_function(
        GET_RELATION,
        &GET_PROLOGUE,
        get_relation as *const () as usize,
    )?;
    let (set, set_trampoline) = hook::replace_function(
        SET_RELATION,
        &SET_PROLOGUE,
        set_relation as *const () as usize,
    )?;
    GET.store(get_trampoline, Ordering::Relaxed);
    SET.store(set_trampoline, Ordering::Relaxed);
    Ok(vec![
        Labelled {
            label: "DiplomacySides #0".to_owned(),
            write: get,
        },
        Labelled {
            label: "DiplomacySides #1".to_owned(),
            write: set,
        },
    ])
}
