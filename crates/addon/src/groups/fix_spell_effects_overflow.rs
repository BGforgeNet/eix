//! FixSpellEffectsOverflow: a map object's effects stop piling up. `CMapObject::AddEffectImpl` 0x50F1E0 takes at
//! most [`MAX_EFFECTS`] and, after adding one lasting more than a tick, drops the older effects it renews (core's
//! [`remove_renewed`]); `CWorldServer::UpdateUnits` 0x58D670 counts the effects of sleeping objects down too, and
//! wakes an object whose effect ran out, so its own update removes it.
#![expect(
    unsafe_code,
    reason = "walking the server's map objects and changing their effect arrays"
)]

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use eix_core::fixes::{EffectKey, MAX_EFFECTS, remove_renewed};
use eix_engine::game::unit::{FLAG_REMOVED, MapEffect, MapObject};
use eix_engine::game::world::{self, ObjectMap, SERVER_OBJECTS};
use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use super::{BuildError, Labelled, Setup};
use crate::log;

/// `lea edx, [esp + 0x14]; lea ecx, [ebp + 0x104]`: the start of the update of the sleeping server's awake objects,
/// reached only when the world lets objects sleep.
const TICK_SITE: Site = eix_sites::C_WORLD_SERVER_UPDATE_UNITS__1BE;
const TICK_ORIGINAL: [u8; 10] = [0x8D, 0x54, 0x24, 0x14, 0x8D, 0x8D, 0x04, 0x01, 0x00, 0x00];
/// `AddEffectImpl`'s first instructions: `push ebx; push ebp; mov ebp, ecx; push esi`.
const ADD_SITE: Site = eix_sites::C_MAP_OBJECT_ADD_EFFECT_IMPL;
const ADD_ORIGINAL: [u8; 5] = [0x53, 0x55, 0x8B, 0xE9, 0x56];

/// `void __thiscall CMapObject::AddEffectImpl(const MapEffect*)`, `ret 4`.
type AddEffect = unsafe extern "thiscall" fn(*mut MapObject, *const MapEffect);

/// The trampoline that runs the original `AddEffectImpl`.
static ORIGINAL_ADD: AtomicUsize = AtomicUsize::new(0);
/// How many refused effects were logged; the log stops after [`LOGGED_REFUSALS`].
static REFUSALS: AtomicU32 = AtomicU32::new(0);
const LOGGED_REFUSALS: u32 = 10;

fn key(effect: &MapEffect) -> EffectKey {
    EffectKey {
        kind: effect.kind,
        ticks: effect.ticks,
        value: effect.value,
        strength: effect.strength,
        ending: effect.ending,
    }
}

/// The object's effects as a slice, which the caller may change in place.
///
/// # Safety
/// `object` must be a live map object whose array nothing else changes while the slice lives.
unsafe fn effects<'a>(object: *mut MapObject) -> &'a mut [MapEffect] {
    // SAFETY: the caller guarantees the object.
    let array = unsafe { &(*object).effects };
    match usize::try_from(array.len) {
        Ok(len) if len > 0 && !array.data.is_null() => {
            // SAFETY: a constructed array holds `len` elements at `data`.
            unsafe { core::slice::from_raw_parts_mut(array.data, len) }
        }
        Ok(_) | Err(_) => &mut [],
    }
}

extern "thiscall" fn add_effect(this: *mut MapObject, effect: *const MapEffect) {
    // SAFETY: the game calls it on a live map object, on its thread.
    let count = unsafe { (*this).effects.len };
    if count >= MAX_EFFECTS {
        if REFUSALS.fetch_add(1, Ordering::SeqCst) < LOGGED_REFUSALS {
            // SAFETY: as above.
            let id = unsafe { (*this).transform.base.id };
            log::warn("fixes", &format!("Too many spell effects on unit {id}"), "");
        }
        return;
    }
    // SAFETY: the trampoline runs the original, which has this type.
    let original: AddEffect = unsafe { hook::as_fn_at(ORIGINAL_ADD.load(Ordering::SeqCst)) };
    // SAFETY: the game's arguments, passed on.
    unsafe { original(this, effect) };
    // SAFETY: the effect the game passed, unchanged by the call (it copies it).
    let added = key(unsafe { &*effect });
    if added.ticks <= 1 {
        return;
    }
    // SAFETY: the object is live and the game's thread is inside this call, so nothing else changes its array.
    let effects = unsafe { effects(this) };
    let count = remove_renewed(effects, &added, key);
    // SAFETY: as above; the count only shrinks, so the elements stay allocated.
    unsafe { (*this).effects.len = count as i32 };
}

/// Whether a server object sleeps with effects to count down: in use, not being removed, with effects and its
/// server half, and out of the update map.
fn counts_down(object: &MapObject) -> bool {
    object.transform.base.flags & FLAG_REMOVED == 0
        && object.transform.base.refs != 0
        && object.effects.len != 0
        && object.update_countdown <= 0
        && !object.companion.is_null()
}

extern "C" fn tick_sleeping(_: usize, _: &mut Context) {
    // SAFETY: on the game's thread in the server's update; the map does not change during the walk, as waking an
    // object only adds it to the update map.
    for node in unsafe { ObjectMap::nodes(SERVER_OBJECTS.as_ptr()) } {
        // SAFETY: a node of the map.
        let object = unsafe { (*node).object };
        if object.is_null() {
            continue;
        }
        // SAFETY: the map holds live objects.
        if !counts_down(unsafe { &*object }) {
            continue;
        }
        let mut expired = false;
        // SAFETY: a live object; nothing else runs on the game's thread meanwhile.
        for effect in unsafe { effects(object) } {
            effect.ticks = effect.ticks.wrapping_sub(1);
            expired |= effect.ticks <= 0;
        }
        if expired {
            // SAFETY: a live object of the server world, on its thread.
            unsafe { world::add_to_update_map(object) };
        }
    }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let (add, trampoline) =
        hook::replace_function(ADD_SITE, &ADD_ORIGINAL, add_effect as *const () as usize)?;
    ORIGINAL_ADD.store(trampoline, Ordering::SeqCst);
    Ok(vec![
        Labelled {
            label: "FixSpellEffectsOverflow #0".to_owned(),
            write: hook::context_hook(TICK_SITE, &TICK_ORIGINAL, Mode::After, tick_sleeping, 0)?,
        },
        Labelled {
            label: "FixSpellEffectsOverflow #1".to_owned(),
            write: add,
        },
    ])
}
