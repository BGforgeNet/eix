//! The rules of the bug-fix patch groups that need no game memory.

use core::ffi::CStr;

/// FixLeversActivate: what `LockKey::Opens` returns for a key of level `key` on a lock of level `lock`. A level-0
/// lock gives the key's level + 1 (so any key but level -1 opens it); otherwise the levels compare signed.
pub fn lock_opens(key: i32, lock: i32) -> u32 {
    if lock == 0 {
        key.wrapping_add(1) as u32
    } else {
        u32::from(key >= lock)
    }
}

/// FixProgressNames: the loading movie for an allod the game has no movie of its own for.
pub fn loading_movie(allod: &[u8]) -> Option<&'static CStr> {
    // The original compares case-sensitively, unlike the game's own choices before it.
    match allod {
        b"gipat2" => Some(c"progres2.bik"),
        b"jigran" => Some(c"progres5.bik"),
        _ => None,
    }
}

/// FixRepairPrice: an item's wear, 1 - condition / maximum, with a condition or maximum that is not above 0 taken
/// as 0 and the result held to 0..=1; NaN (0 / 0) passes through, as in the original.
pub fn repair_wear(condition: f32, max_condition: f32) -> f32 {
    let positive = |v: f32| if 0.0 < v { v } else { 0.0 };
    (1.0 - positive(condition) / positive(max_condition)).clamp(0.0, 1.0)
}

/// FixSpellSelfCasting: whether a spell of `spell_type` is offensive. Types up to 0x26 keep the game's answer;
/// 0x28 and 0x29 are added.
pub fn is_offensive(spell_type: u32) -> bool {
    const OFFENSIVE: [u32; 19] = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 17, 19, 25, 30, 32, 34, 36, 38, 0x28, 0x29,
    ];
    OFFENSIVE.contains(&spell_type)
}

/// FixMoveObject: whether a map object may move to x, y; a negative or NaN coordinate is refused.
pub fn move_allowed(x: f32, y: f32) -> bool {
    x >= 0.0 && y >= 0.0
}

/// FixSpellEffectsOverflow: a map object takes at most this many effects; one more is dropped.
pub const MAX_EFFECTS: i32 = 20;

/// The fields of an active effect the renewal compares.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectKey {
    /// The spell type.
    pub kind: u8,
    /// Updates left.
    pub ticks: i32,
    /// The effect's value.
    pub value: f32,
    /// Its strength.
    pub strength: f32,
    /// The end flag.
    pub ending: u8,
}

/// Whether `added` renews `old`: same type and end flag, lasting at least as long, a value not lower, and a strength
/// within 0.001.
pub fn renews(old: &EffectKey, added: &EffectKey) -> bool {
    old.kind == added.kind
        && old.ticks <= added.ticks
        && added.value >= old.value
        && old.ending == added.ending
        // The difference is rounded to a float, then compared with the double 0.001.
        && f64::from((old.strength - added.strength).abs()) < 0.001
}

/// FixSpellEffectsOverflow: after `added` (lasting more than one tick) was appended to `effects`, removes every
/// earlier effect it renews. Returns the new count; the elements past it are left as they are.
///
/// As in the original, the effect moved into a removed one's place is not compared, and the last one (the effect
/// just added) never is.
pub fn remove_renewed<T: Copy>(
    effects: &mut [T],
    added: &EffectKey,
    key: impl Fn(&T) -> EffectKey,
) -> usize {
    let mut count = effects.len();
    let mut i = 0;
    while i + 1 < count {
        if renews(&key(&effects[i]), added) {
            effects.copy_within(i + 1..count, i);
            count -= 1;
        }
        i += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_level_0_lock_answers_the_key_level_plus_one() {
        assert_eq!(lock_opens(3, 0), 4);
        assert_eq!(lock_opens(-1, 0), 0);
        assert_eq!(lock_opens(i32::MAX, 0), 0x8000_0000);
        // Signed: a negative key is below any positive lock, and a level -1 lock opens to any key from -1 up.
        assert_eq!(lock_opens(-2, 1), 0);
        assert_eq!(lock_opens(2, 2), 1);
        assert_eq!(lock_opens(1, 2), 0);
        assert_eq!(lock_opens(0, -1), 1);
        assert_eq!(lock_opens(-2, -1), 0);
    }

    #[test]
    fn two_allods_get_their_movies() {
        assert_eq!(loading_movie(b"gipat2"), Some(c"progres2.bik"));
        assert_eq!(loading_movie(b"jigran"), Some(c"progres5.bik"));
        assert_eq!(loading_movie(b"Gipat2"), None);
        assert_eq!(loading_movie(b""), None);
    }

    #[test]
    fn wear_is_held_to_0_and_1() {
        assert_eq!(repair_wear(25.0, 100.0), 0.75);
        assert_eq!(repair_wear(150.0, 100.0), 0.0);
        assert_eq!(repair_wear(-5.0, 100.0), 1.0);
        assert_eq!(repair_wear(f32::NAN, 100.0), 1.0);
        // A maximum of 0 gives an infinite ratio, so no wear; both 0 is 0 / 0.
        assert_eq!(repair_wear(5.0, 0.0), 0.0);
        assert!(repair_wear(0.0, 0.0).is_nan());
        assert!(repair_wear(0.0, -1.0).is_nan());
    }

    #[test]
    fn offensive_types_are_the_games_plus_0x28_and_0x29() {
        let offensive: Vec<u32> = (0..0x40).filter(|&t| is_offensive(t)).collect();
        assert_eq!(
            offensive,
            [
                0, 1, 2, 3, 4, 5, 6, 7, 8, 17, 19, 25, 30, 32, 34, 36, 38, 0x28, 0x29
            ]
        );
    }

    #[test]
    fn negative_or_nan_coordinates_do_not_move() {
        assert!(move_allowed(0.0, 0.0));
        assert!(move_allowed(-0.0, 5.0));
        assert!(!move_allowed(-0.5, 5.0));
        assert!(!move_allowed(5.0, -0.5));
        assert!(!move_allowed(f32::NAN, 5.0));
        assert!(!move_allowed(5.0, f32::NAN));
    }

    fn effect(kind: u8, ticks: i32, value: f32, strength: f32) -> EffectKey {
        EffectKey {
            kind,
            ticks,
            value,
            strength,
            ending: 0,
        }
    }

    #[test]
    fn renewal_needs_every_field() {
        let new = effect(5, 10, 2.0, 1.0);
        assert!(renews(&effect(5, 10, 2.0, 1.0), &new));
        assert!(renews(&effect(5, 3, 1.0, 1.0005), &new));
        assert!(!renews(&effect(6, 3, 1.0, 1.0), &new));
        assert!(!renews(&effect(5, 11, 1.0, 1.0), &new));
        assert!(!renews(&effect(5, 3, 2.5, 1.0), &new));
        assert!(!renews(&effect(5, 3, f32::NAN, 1.0), &new));
        assert!(!renews(&effect(5, 3, 1.0, 1.01), &new));
        assert!(!renews(
            &EffectKey {
                ending: 1,
                ..effect(5, 3, 1.0, 1.0)
            },
            &new
        ));
    }

    #[test]
    fn renewed_effects_go_but_the_one_moved_into_place_is_skipped() {
        let added = effect(1, 10, 1.0, 1.0);
        // Two renewed ones in a row: the second moves into the first's place and is stepped over.
        let mut effects = [
            effect(1, 5, 1.0, 1.0),
            effect(1, 5, 1.0, 1.0),
            effect(2, 5, 1.0, 1.0),
            effect(1, 5, 1.0, 1.0),
            added,
        ];
        let count = remove_renewed(&mut effects, &added, |e| *e);
        assert_eq!(count, 3);
        assert_eq!(
            effects[..count],
            [effect(1, 5, 1.0, 1.0), effect(2, 5, 1.0, 1.0), added]
        );
        // The added one itself, last, stays; a single effect has nothing to compare.
        let mut one = [added];
        assert_eq!(remove_renewed(&mut one, &added, |e| *e), 1);
    }
}
