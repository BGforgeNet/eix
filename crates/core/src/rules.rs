//! The platform-free parts of the ai.reg patch groups.

/// NegativeMagicEffect's select: of `a` and `b`, the one with the larger magnitude; `a` on a tie, `b` when either is
/// NaN. The x87 code it stands for loads and stores the values, which turns a signalling NaN into a quiet one.
pub fn larger_magnitude(a: f32, b: f32) -> f32 {
    let pick = if a.is_nan() || b.is_nan() || b.abs() > a.abs() {
        b
    } else {
        a
    };
    if pick.is_nan() {
        f32::from_bits(pick.to_bits() | 0x0040_0000)
    } else {
        pick
    }
}

/// NewRuneCodes: whether a modifier's code letters are the cast-time rune's, "xa".
pub fn is_cast_time_rune(code: [u8; 2]) -> bool {
    code == *b"xa"
}

/// NewRuneCodes: the cast time after a rune of whole strength `n`, at least 1 (the sum wraps as the game's `add`).
pub fn cast_time_with_rune(cast_time: i32, n: i32) -> i32 {
    cast_time.wrapping_add(n).max(1)
}

/// MultipleLoadingMobs: the first id from 100 up that `taken` rejects.
pub fn free_id(mut taken: impl FnMut(i32) -> bool) -> i32 {
    let mut id = 100;
    while taken(id) {
        id = id.wrapping_add(1);
    }
    id
}

/// The game's text-key ids, which heroes and other named units have; ids below are database units.
pub const TEXT_KEY_IDS: i32 = 1_000_000_000;

/// PerkLimit: which limit a skill compares with, by its index in the trader's list (0 Melee, 1 Archery, 3 Elemental,
/// 4 Sense, 5 Astral); `None` for Backstab (2) and any other index.
pub fn perk_limit_slot(skill: u32) -> Option<usize> {
    match skill {
        0 => Some(0),
        1 => Some(1),
        3 => Some(2),
        4 => Some(3),
        5 => Some(4),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn larger_magnitude_keeps_the_sign() {
        assert_eq!(larger_magnitude(-5.0, 3.0), -5.0);
        assert_eq!(larger_magnitude(2.0, -3.0), -3.0);
        assert_eq!(larger_magnitude(0.0, 0.0), 0.0);
        // A tie is `a`: the sign tells which.
        assert_eq!(larger_magnitude(-2.0, 2.0).to_bits(), (-2.0f32).to_bits());
        assert!(larger_magnitude(1.0, f32::NAN).is_nan());
        assert_eq!(larger_magnitude(f32::NAN, 4.0), 4.0);
        let snan = f32::from_bits(0x7F80_0001);
        assert_eq!(larger_magnitude(1.0, snan).to_bits(), 0x7FC0_0001);
    }

    #[test]
    fn rune_cast_time_floor() {
        assert!(is_cast_time_rune(*b"xa"));
        assert!(!is_cast_time_rune(*b"xb"));
        assert_eq!(cast_time_with_rune(10, -5), 5);
        assert_eq!(cast_time_with_rune(10, -100), 1);
        assert_eq!(cast_time_with_rune(10, 3), 13);
        assert_eq!(cast_time_with_rune(i32::MAX, 1), 1);
    }

    #[test]
    fn free_ids_start_at_100() {
        assert_eq!(free_id(|_| false), 100);
        assert_eq!(free_id(|id| id < 103), 103);
    }

    #[test]
    fn backstab_has_no_limit() {
        let slots: Vec<_> = (0..7).map(perk_limit_slot).collect();
        assert_eq!(
            slots,
            [Some(0), Some(1), None, Some(2), Some(3), Some(4), None]
        );
    }
}
