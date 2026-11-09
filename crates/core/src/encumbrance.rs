//! Encumbrance: the load a hero carries slows running, and above the maximum load walking too, and makes running cost
//! more mana, as eix.ini's `[Encumbrance]` section sets (`docs/eix-ini.md`).
//!
//! E is the load over the maximum load (the hero panel's encumbrance). Running speed is `1 - RunSlope x E`, walking
//! (and sneaking, crawling) speed `1 - WalkSlope x (E - 1)` above the maximum and unchanged below it, both at least
//! `MinSpeed`; the game's legs factor multiplies both. Running drains `1 / running factor` times the mana, scaled by
//! `RunDrain`.

/// The section of eix.ini.
pub const SECTION: &str = "Encumbrance";
/// The key of [`Settings::enabled`].
pub const ENABLED_KEY: &str = "Enabled";
/// The key of [`Settings::run_slope`].
pub const RUN_SLOPE_KEY: &str = "RunSlope";
/// The key of [`Settings::walk_slope`].
pub const WALK_SLOPE_KEY: &str = "WalkSlope";
/// The key of [`Settings::min_speed`].
pub const MIN_SPEED_KEY: &str = "MinSpeed";
/// The key of [`Settings::run_drain`].
pub const RUN_DRAIN_KEY: &str = "RunDrain";

/// The packed base speed byte is speed x 25.5 (`UnitStats::Pack` 0x522DF9).
const BASE_BYTE_SCALE: f32 = 25.5;

/// `[Encumbrance]`; percentages are whole numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// The feature; off leaves the game's movement and drain as they are.
    pub enabled: bool,
    /// Running speed lost per maximum load, in percent.
    pub run_slope: u32,
    /// Walking speed lost per maximum load above the maximum, in percent.
    pub walk_slope: u32,
    /// The floor of both speeds, in percent.
    pub min_speed: u32,
    /// The share of the extra running drain (`1 / running factor - 1`), in percent.
    pub run_drain: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            run_slope: 20,
            walk_slope: 20,
            min_speed: 50,
            run_drain: 100,
        }
    }
}

impl Settings {
    /// The settings from eix.ini's values, and a warning for each value out of its range, which takes its default.
    pub fn from_values(
        enabled: i32,
        run_slope: i32,
        walk_slope: i32,
        min_speed: i32,
        run_drain: i32,
    ) -> (Self, Vec<String>) {
        let d = Self::default();
        let mut warnings = Vec::new();
        let mut take =
            |key: &str, value: i32, lo: u32, hi: u32, default: u32| match u32::try_from(value) {
                Ok(v) if (lo..=hi).contains(&v) => v,
                _ => {
                    warnings.push(format!("{key} {value} is not {lo} to {hi}: {default}"));
                    default
                }
            };
        let enabled = take(ENABLED_KEY, enabled, 0, 1, u32::from(d.enabled)) == 1;
        let run_slope = take(RUN_SLOPE_KEY, run_slope, 0, 100, d.run_slope);
        let walk_slope = take(WALK_SLOPE_KEY, walk_slope, 0, 100, d.walk_slope);
        let min_speed = take(MIN_SPEED_KEY, min_speed, 10, 100, d.min_speed);
        let run_drain = take(RUN_DRAIN_KEY, run_drain, 0, 200, d.run_drain);
        (
            Self {
                enabled,
                run_slope,
                walk_slope,
                min_speed,
                run_drain,
            },
            warnings,
        )
    }

    fn floor(self) -> f64 {
        f64::from(self.min_speed) / 100.0
    }

    /// The running speed factor at encumbrance `e`, without the legs.
    pub fn run_factor(self, e: f64) -> f64 {
        (1.0 - f64::from(self.run_slope) / 100.0 * e).max(self.floor())
    }

    /// The walking speed factor at encumbrance `e`, without the legs: 1 up to the maximum load.
    pub fn walk_factor(self, e: f64) -> f64 {
        (1.0 - f64::from(self.walk_slope) / 100.0 * (e - 1.0).max(0.0)).max(self.floor())
    }

    /// What the running drain is multiplied by at encumbrance `e`.
    pub fn drain_multiplier(self, e: f64) -> f64 {
        1.0 + f64::from(self.run_drain) / 100.0 * (1.0 / self.run_factor(e) - 1.0)
    }

    /// The packed base speed and run bytes at encumbrance `e`, from the stats' `speed` (base x legs) and the bytes
    /// `UnitStats::Pack` wrote. Up to the maximum load the base byte stays Pack's own (the game rounds an f32
    /// product, 0x522DF9..0x522E0B, which an f64 recompute does not always match) and only the run byte scales;
    /// above it both change, the run byte making up for the base byte's rounding. Both stay within 1..=255.
    pub fn packed(self, e: f64, speed: f32, base: u8, run: u8) -> (u8, u8) {
        if e == 0.0 {
            return (base, run);
        }
        let (f, walk) = (self.run_factor(e), self.walk_factor(e));
        if walk == 1.0 {
            return (base, to_byte(f64::from(run) * f));
        }
        // The game's own f32 product, rounded as its `fistp` does.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the factor is at most 1: an f32 product, as the game's"
        )]
        let new_base = to_byte(f64::from(speed * walk as f32 * BASE_BYTE_SCALE));
        (
            new_base,
            to_byte(f64::from(run) * f64::from(base) * f / f64::from(new_base)),
        )
    }
}

/// `x` rounded half to even, as the game's `fistp` under its control word, then kept within 1..=255.
fn to_byte(x: f64) -> u8 {
    let r = x.round_ties_even();
    if r.is_nan() || r < 1.0 {
        1
    } else if r > 255.0 {
        255
    } else {
        // r is a whole number within 1..=255 here.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a whole number in 1..=255"
        )]
        let b = r as u8;
        b
    }
}

/// The encumbrance: `load / capacity`, 0 when the capacity is not above 0 or the result is not a finite number at
/// least 0.
pub fn encumbrance(load: f32, capacity: f32) -> f64 {
    let e = f64::from(load) / f64::from(capacity);
    if capacity > 0.0 && e.is_finite() && e >= 0.0 {
        e
    } else {
        0.0
    }
}

/// The mana after a drain of `old - new` scaled by `multiplier`.
pub fn rescaled(old: f32, new: f32, multiplier: f64) -> f32 {
    #[expect(clippy::cast_possible_truncation, reason = "the mana is an f32")]
    let v = (f64::from(old) - (f64::from(old) - f64::from(new)) * multiplier) as f32;
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_spec() {
        let s = Settings::default();
        assert_eq!(
            (
                s.enabled,
                s.run_slope,
                s.walk_slope,
                s.min_speed,
                s.run_drain
            ),
            (false, 20, 20, 50, 100)
        );
    }

    #[test]
    fn the_island_sets_at_strength_26() {
        let s = Settings::default();
        // (load, max load, running, walking); the drain is 1 / running at RunDrain 100.
        for (load, cap, run, walk) in [
            (234.0f32, 312.0f32, 0.85, 1.0),
            (
                398.0,
                312.0,
                1.0 - 0.2 * 398.0 / 312.0,
                1.0 - 0.2 * (398.0 / 312.0 - 1.0),
            ),
            (
                485.0,
                312.0,
                1.0 - 0.2 * 485.0 / 312.0,
                1.0 - 0.2 * (485.0 / 312.0 - 1.0),
            ),
            (
                485.0,
                468.0,
                1.0 - 0.2 * 485.0 / 468.0,
                1.0 - 0.2 * (485.0 / 468.0 - 1.0),
            ),
            (234.0, 624.0, 0.925, 1.0),
        ] {
            let e = encumbrance(load, cap);
            assert!((s.run_factor(e) - run).abs() < 1e-6, "run {load}/{cap}");
            assert!((s.walk_factor(e) - walk).abs() < 1e-6, "walk {load}/{cap}");
            assert!(
                (s.drain_multiplier(e) - 1.0 / run).abs() < 1e-6,
                "drain {load}/{cap}"
            );
        }
    }

    #[test]
    fn floors_and_half_drain() {
        let s = Settings::default();
        assert_eq!(s.run_factor(3.0), 0.5);
        assert!((s.walk_factor(3.0) - 0.6).abs() < 1e-9);
        assert_eq!(s.walk_factor(10.0), 0.5);
        assert!((s.drain_multiplier(3.0) - 2.0).abs() < 1e-9);
        let half = Settings { run_drain: 50, ..s };
        assert!((half.drain_multiplier(1.0) - (1.0 + 0.5 * (1.0 / 0.8 - 1.0))).abs() < 1e-9);
        let none = Settings { run_drain: 0, ..s };
        assert_eq!(none.drain_multiplier(1.5), 1.0);
    }

    #[test]
    fn no_capacity_is_no_encumbrance() {
        assert_eq!(encumbrance(100.0, 0.0), 0.0);
        assert_eq!(encumbrance(100.0, f32::NAN), 0.0);
        assert_eq!(encumbrance(f32::NAN, 312.0), 0.0);
        assert_eq!(encumbrance(-5.0, 312.0), 0.0);
        assert!(encumbrance(62.0, 1_000_000.0) < 1e-4);
    }

    #[test]
    fn packed_bytes_follow_the_rules() {
        let s = Settings::default();
        // Ingos steel with Light Lift: E 485/468, speed 1.0, Pack's bytes 26 and 60.
        assert_eq!(s.packed(f64::from(485.0f32 / 468.0), 1.0, 26, 60), (25, 49));
        // Hadagan without Lift: E 398/312.
        assert_eq!(s.packed(f64::from(398.0f32 / 312.0), 1.0, 26, 60), (24, 48));
        // A non-hero: E about 0 keeps Pack's bytes.
        assert_eq!(
            s.packed(encumbrance(62.0, 1_000_000.0), 1.0, 26, 60),
            (26, 60)
        );
        assert_eq!(s.packed(0.0, 1.0, 26, 60), (26, 60));
        // Below max load the game's base byte stays, even where an f64 recompute would round differently
        // (f32 1/3 x 25.5 packs as 8).
        assert_eq!(s.packed(0.5, 0.333_333_34, 8, 60), (8, 54));
    }

    #[test]
    fn packed_bytes_clamp() {
        let s = Settings {
            min_speed: 10,
            ..Settings::default()
        };
        // Badly wounded legs: speed 0.01 still moves.
        assert_eq!(s.packed(2.0, 0.01, 1, 60).0, 1);
        // A run slope below the walk slope pushes the run byte to 255, not past it.
        let odd = Settings {
            run_slope: 0,
            walk_slope: 90,
            min_speed: 10,
            ..Settings::default()
        };
        assert_eq!(odd.packed(2.0, 1.0, 26, 250).1, 255);
    }

    #[test]
    fn rescaled_drain() {
        assert_eq!(rescaled(10.0, 9.5, 1.0), 9.5);
        assert_eq!(rescaled(10.0, 9.5, 2.0), 9.0);
        assert_eq!(rescaled(10.0, 10.0, 1.5), 10.0);
    }

    #[test]
    fn out_of_range_values_warn_and_take_the_default() {
        let (s, w) = Settings::from_values(1, 150, -1, 5, 300);
        assert_eq!(
            (
                s.enabled,
                s.run_slope,
                s.walk_slope,
                s.min_speed,
                s.run_drain
            ),
            (true, 20, 20, 50, 100)
        );
        assert_eq!(
            w,
            [
                "RunSlope 150 is not 0 to 100: 20",
                "WalkSlope -1 is not 0 to 100: 20",
                "MinSpeed 5 is not 10 to 100: 50",
                "RunDrain 300 is not 0 to 200: 100",
            ]
        );
        let (s, w) = Settings::from_values(2, 20, 20, 50, 100);
        assert!(!s.enabled);
        assert_eq!(w, ["Enabled 2 is not 0 to 1: 0"]);
    }
}
