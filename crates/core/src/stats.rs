//! Unit statistics arithmetic of addon.dll 0.12.4.2's Lua functions and Lua patch groups, the parts that
//! need no game memory.
//!
//! The original keeps intermediate values on the x87 stack and stores floats where noted; these functions round to
//! `f32` at the same points and compute the rest in `f64`.

use core::cmp::Ordering;

/// The number of effect types the effect maxima cover.
pub const EFFECT_TYPES: usize = 40;

/// Whether a unit id is a text-key unit (heroes and other units named by a text key): `1000000000 <= id <
/// 2000000000`, the original's `id + 0xc4653600 <= 0x3b9ac9ff`.
pub fn is_text_key(id: i32) -> bool {
    id.cast_unsigned().wrapping_add(0xC465_3600) <= 0x3B9A_C9FF
}

/// The largest value per effect type of a unit's effects, types 40 and above ignored: `m[t] = value < m[t] ? m[t] :
/// value`, so a NaN value wins.
pub fn effect_maxima(effects: impl IntoIterator<Item = (u8, f32)>) -> [f32; EFFECT_TYPES] {
    let mut m = [0.0f32; EFFECT_TYPES];
    for (kind, value) in effects {
        // Not `value < slot`: true also when either is NaN.
        if let Some(slot) = m.get_mut(usize::from(kind))
            && value.partial_cmp(slot) != Some(Ordering::Less)
        {
            *slot = value;
        }
    }
    m
}

/// A scale component after the Lua RaiseDexterity replacement: `max(0, x - 0.2)`, a NaN difference kept.
/// The constant is `0.2f32` widened to a double.
pub fn narrowed_scale(x: f32) -> f32 {
    let y = f64::from(x) - f64::from(0.2f32);
    if 0.0 > y { 0.0 } else { y as f32 }
}

/// A scale component after RaiseStrength: `x + 0.2`, unclamped, as the game's own (0x5291cc). The addon's replacement
/// copies RaiseDexterity's narrowing here; ours keeps the game's widening.
pub fn widened_scale(x: f32) -> f32 {
    // Two floats' sum is exact in an f64 at these magnitudes, so one rounding to f32 matches the x87's.
    (f64::from(x) + f64::from(0.2f32)) as f32
}

/// Experience fields of a unit's statistics, as the replacements change them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Experience {
    /// `UnitStats::experience`.
    pub experience: f32,
    /// `UnitStats::spent_experience`.
    pub spent: f32,
    /// `UnitStats::pending_experience`.
    pub pending: f32,
}

impl Experience {
    /// AddScaledExperience's replacement: `total = (float)(multiplier * amount) + pending` stored as the
    /// new pending amount; once it is above 0 it moves into the experience.
    pub fn add_scaled(&mut self, multiplier: f32, amount: f32) {
        let gained = (f64::from(multiplier) * f64::from(amount)) as f32;
        let total = (f64::from(gained) + f64::from(self.pending)) as f32;
        self.pending = total;
        if total > 0.0 {
            self.experience = (f64::from(total) + f64::from(self.experience)) as f32;
            self.pending = 0.0;
        }
    }

    /// RaiseSkill / RaiseAttribute's replacement: `total = (float)(cost + pending)` stored as the new
    /// pending amount; once it is above 0 it is added to both the experience and the spent experience.
    pub fn add_cost(&mut self, cost: i32) {
        let total = (f64::from(cost) + f64::from(self.pending)) as f32;
        self.pending = total;
        if total > 0.0 {
            self.experience = (f64::from(self.experience) + f64::from(total)) as f32;
            self.spent = (f64::from(self.spent) + f64::from(total)) as f32;
            self.pending = 0.0;
        }
    }
}

/// `RegenMp`'s new mana, or `None` when it stays: only while `mana < max` (or either is NaN), `mana + max *
/// fraction` kept below `max`, else `max`.
pub fn regenerated_mana(mana: f32, max: f32, fraction: f32) -> Option<f32> {
    if max <= mana {
        return None;
    }
    let v = f64::from(max) * f64::from(fraction) + f64::from(mana);
    Some(if f64::from(max) > v { v as f32 } else { max })
}

/// One slot under `DistributeHp`: `maximum = maxHp / share1 * share` (stored as a float), and the slot's hit points
/// `current` kept at most that, a NaN maximum taken. Returns `(maximum, current)`.
pub fn distributed_slot(max_hp: f32, share1: f32, share: f32, current: f32) -> (f32, f32) {
    let maximum = (f64::from(max_hp) / f64::from(share1) * f64::from(share)) as f32;
    let current = if maximum > current { current } else { maximum };
    (maximum, current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_keys_are_the_ten_digit_ids_below_two_billion() {
        assert!(!is_text_key(999_999_999));
        assert!(is_text_key(1_000_000_000));
        assert!(is_text_key(1_999_999_999));
        assert!(!is_text_key(2_000_000_000));
        assert!(!is_text_key(-1));
        assert!(!is_text_key(0));
    }

    #[test]
    fn maxima_keep_the_largest_and_let_nan_win() {
        let m = effect_maxima([
            (3, 1.0),
            (3, 5.0),
            (3, 2.0),
            (39, -1.0),
            (40, 9.0),
            (7, f32::NAN),
        ]);
        assert_eq!(m[3], 5.0);
        // A negative value never beats the zeroed start.
        assert_eq!(m[39], 0.0);
        assert!(m[7].is_nan());
        assert_eq!(m.iter().filter(|v| **v != 0.0).count(), 2);
        // After a NaN, `value < NaN` is false, so the next value replaces it.
        assert_eq!(effect_maxima([(1, f32::NAN), (1, 2.0)])[1], 2.0);
    }

    #[test]
    fn scale_narrows_by_a_fifth_and_stops_at_zero() {
        assert_eq!(narrowed_scale(1.0), 0.8);
        assert_eq!(narrowed_scale(0.1), 0.0);
        assert!(narrowed_scale(f32::NAN).is_nan());
    }

    #[test]
    fn strength_widens_the_scale_by_a_fifth_without_a_clamp() {
        assert_eq!(widened_scale(0.35), 0.55);
        assert_eq!(widened_scale(-1.0), -0.8);
        assert!(widened_scale(f32::NAN).is_nan());
    }

    #[test]
    fn experience_waits_while_the_total_is_not_positive() {
        let mut e = Experience {
            experience: 100.0,
            spent: 10.0,
            pending: -5.0,
        };
        e.add_scaled(2.0, 2.0);
        assert_eq!(
            e,
            Experience {
                experience: 100.0,
                spent: 10.0,
                pending: -1.0
            }
        );
        e.add_scaled(0.5, 4.0);
        assert_eq!(
            e,
            Experience {
                experience: 101.0,
                spent: 10.0,
                pending: 0.0
            }
        );
        e.add_cost(3);
        assert_eq!(
            e,
            Experience {
                experience: 104.0,
                spent: 13.0,
                pending: 0.0
            }
        );
        e.add_cost(-2);
        assert_eq!(e.pending, -2.0);
        e.add_scaled(f32::NAN, 1.0);
        assert!(e.pending.is_nan());
        assert_eq!(e.experience, 104.0);
    }

    #[test]
    fn mana_regenerates_up_to_the_maximum() {
        assert_eq!(regenerated_mana(10.0, 20.0, 0.25), Some(15.0));
        assert_eq!(regenerated_mana(18.0, 20.0, 0.25), Some(20.0));
        assert_eq!(regenerated_mana(20.0, 20.0, 0.25), None);
        assert_eq!(regenerated_mana(10.0, 20.0, f32::NAN), Some(20.0));
        assert_eq!(regenerated_mana(f32::NAN, 20.0, 0.1), Some(20.0));
    }

    #[test]
    fn slots_follow_their_share() {
        assert_eq!(distributed_slot(100.0, 0.5, 0.25, 80.0), (50.0, 50.0));
        assert_eq!(distributed_slot(100.0, 0.5, 0.25, 20.0), (50.0, 20.0));
        let (m, c) = distributed_slot(100.0, 0.0, 0.0, 20.0);
        assert!(m.is_nan() && c.is_nan());
    }
}
