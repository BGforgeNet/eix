//! RandomMagicDamage: a unit's cast does `damage * (1 + MagicRandom * u)`, u drawn from the game's `rand` in
//! [-1, 1). `CUnitServer::FinishCast`'s call of `CWorldServer::CastSpell` at 0x54EFB9 goes to [`cast_spell`], which
//! scales the spell's damage for the call and puts it back after.
#![expect(
    unsafe_code,
    reason = "calling the game's CastSpell and changing the spell it casts"
)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use eix_engine::game::crt;
use eix_engine::game::spell::Spell;
use eix_engine::hook;
use eix_engine::x87::Ext;

use super::{BuildError, Labelled, Setup};

/// `MagicRandom`, as `f32` bits.
static FACTOR: AtomicU32 = AtomicU32::new(0);
/// `CWorldServer::CastSpell` 0x58E850, the call's old target.
static CAST_SPELL: AtomicUsize = AtomicUsize::new(0);

/// `CWorldServer::CastSpell`: thiscall, the spell and five more words, `ret 0x18`.
type CastSpell =
    unsafe extern "thiscall" fn(*mut c_void, *mut Spell, u32, u32, u32, u32, u32) -> u32;

/// `(r - 16384) * saved * factor * (1 / 16384) + saved`, in the original's x87 order.
fn scaled(saved: f32, r: i32, factor: f32) -> f32 {
    Ext::new(r.into())
        .fsub(16384.0)
        .fmul(saved.into())
        .fmul(factor.into())
        .fmul(1.0 / 16384.0)
        .fadd(saved.into())
        .to_f32()
}

extern "thiscall" fn cast_spell(
    world: *mut c_void,
    spell: *mut Spell,
    a: u32,
    b: u32,
    c: u32,
    d: u32,
    e: u32,
) -> u32 {
    let factor = f32::from_bits(FACTOR.load(Ordering::Relaxed));
    // SAFETY: the game's CRT generator, on the game's thread (FinishCast runs on it).
    let r = unsafe { crt::rand() };
    // SAFETY: FinishCast passes a live spell as CastSpell's first argument.
    let saved = unsafe { (*spell).damage };
    // SAFETY: as above.
    unsafe { (*spell).damage = scaled(saved, r, factor) };
    // SAFETY: CAST_SPELL holds the call's old target, 0x58E850, of this type, stored before the site was written.
    let original: CastSpell = unsafe { hook::as_fn_at(CAST_SPELL.load(Ordering::Relaxed)) };
    // SAFETY: the arguments are FinishCast's, passed on unchanged.
    let result = unsafe { original(world, spell, a, b, c, d, e) };
    // SAFETY: the spell outlives the cast: FinishCast still holds it.
    unsafe { (*spell).damage = saved };
    result
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    FACTOR.store(
        setup.options.ai.addon.magic_random.to_bits(),
        Ordering::Relaxed,
    );
    let (write, old) = hook::redirect_call(
        eix_sites::C_UNIT_SERVER_FINISH_CAST__1A9,
        [0xE8, 0x92, 0xF8, 0x03, 0x00],
        cast_spell as *const () as usize,
    )?;
    CAST_SPELL.store(old.here(), Ordering::Relaxed);
    Ok(vec![Labelled {
        label: "RandomMagicDamage #0".to_owned(),
        write,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_factor_spans_minus_one_to_one() {
        assert_eq!(scaled(10.0, 16384, 0.5), 10.0);
        assert_eq!(scaled(10.0, 0, 0.5), 5.0);
        assert_eq!(scaled(10.0, 0, 1.0), 0.0);
        assert!(scaled(10.0, 32767, 0.5) < 15.0);
    }
}
