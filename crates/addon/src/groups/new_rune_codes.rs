//! NewRuneCodes: a modifier with the code "xa" changes the built spell's cast time by its whole strength (at least 1
//! tick remains) and skips the game's letter switch in `CSpellContainer::Build`.
#![expect(
    unsafe_code,
    reason = "reading the modifier record and the container at the hooked site"
)]

use eix_core::rules::{cast_time_with_rune, is_cast_time_rune};
use eix_engine::float::ftol;
use eix_engine::game::spell::{ModifierRecord, SpellContainer};
use eix_engine::hook::{self, Context, Mode};

use super::{BuildError, Labelled, Setup};

/// `Build`'s common tail after the switch, which counts the modifier (complexity and skill needed).
const TAIL: usize = eix_sites::C_SPELL_CONTAINER_BUILD__2BC;

/// At 0x68C049 (the switch on the code letter): EDI is the modifier record, ESI the container.
extern "C" fn rune(_: usize, ctx: &mut Context) {
    let record = core::ptr::with_exposed_provenance::<ModifierRecord>(ctx.edi as usize);
    let container = core::ptr::with_exposed_provenance_mut::<SpellContainer>(ctx.esi as usize);
    // SAFETY: at 0x68C049 EDI is the record `Build` just read the code of, on the game's thread.
    let record = unsafe { &*record };
    if !is_cast_time_rune(record.code) {
        return;
    }
    let strength = record.strength;
    // SAFETY: ESI is the container `Build` builds into.
    let container = unsafe { &mut *container };
    container.cast_time = cast_time_with_rune(container.cast_time, ftol(strength.into()));
    ctx.resume = TAIL;
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "NewRuneCodes #0".to_owned(),
        write: hook::context_hook(
            eix_sites::C_SPELL_CONTAINER_BUILD__E9,
            &[0x83, 0xC0, 0x9F, 0x83, 0xF8, 0x13],
            Mode::After,
            rune,
            0,
        )?,
    }])
}
