//! AutoPause: the single-player game pauses when a party member's health falls below a limit, set in eix.ini's
//! `[AutoPause]` section (`docs/eix-ini.md`).

/// The section of eix.ini.
pub const SECTION: &str = "AutoPause";
/// The key of [`Limits::health`].
pub const HEALTH_KEY: &str = "Health";
/// The key of [`Limits::percent`].
pub const PERCENT_KEY: &str = "HealthPercent";

/// The two limits; 0 turns a limit off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limits {
    /// Hit points.
    pub health: u32,
    /// A percentage of the unit's maximum hit points, 0 to 100.
    pub percent: u32,
}

impl Limits {
    /// The limits from eix.ini's values, and a warning for each value left out: a negative one, or a percentage
    /// above 100.
    pub fn from_values(health: i32, percent: i32) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let health = u32::try_from(health).unwrap_or_else(|_| {
            warnings.push(format!("{HEALTH_KEY} {health} is negative: off"));
            0
        });
        let percent = match u32::try_from(percent) {
            Ok(p) if p <= 100 => p,
            _ => {
                warnings.push(format!("{PERCENT_KEY} {percent} is not 0 to 100: off"));
                0
            }
        };
        (Self { health, percent }, warnings)
    }

    /// Whether either limit is on.
    pub fn any(self) -> bool {
        self.health != 0 || self.percent != 0
    }

    /// Whether `hit_points` of `max_hit_points` is below a limit that is on.
    pub fn is_low(self, hit_points: f32, max_hit_points: f32) -> bool {
        let (hit_points, max_hit_points) = (f64::from(hit_points), f64::from(max_hit_points));
        (self.health != 0 && hit_points < f64::from(self.health))
            || (self.percent != 0 && hit_points * 100.0 < max_hit_points * f64::from(self.percent))
    }
}

/// The party members seen at the last check, by unit id, and whether each was low then.
#[derive(Debug, Default)]
pub struct Watch {
    seen: Vec<(i32, bool)>,
}

impl Watch {
    /// No member seen yet.
    pub const fn new() -> Self {
        Self { seen: Vec::new() }
    }

    /// Takes the party's units as `(id, hit points, maximum hit points)` and returns the first that pauses the game
    /// now: a member seen at the last check, not low then, is low now. A member seen for the first time (a zone
    /// loaded, a new member) only sets its state, so a unit already low does not pause the game; a member stays low
    /// until its health is back at the limits, so one drop pauses once.
    pub fn check(
        &mut self,
        limits: Limits,
        units: impl IntoIterator<Item = (i32, f32, f32)>,
    ) -> Option<i32> {
        let mut pause = None;
        let seen: Vec<(i32, bool)> = units
            .into_iter()
            .map(|(id, hit_points, max_hit_points)| {
                let low = limits.is_low(hit_points, max_hit_points);
                let was_low = self.seen.iter().find(|&&(i, _)| i == id).map(|&(_, l)| l);
                if low && was_low == Some(false) {
                    pause = pause.or(Some(id));
                }
                (id, low)
            })
            .collect();
        self.seen = seen;
        pause
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEALTH_50: Limits = Limits {
        health: 50,
        percent: 0,
    };
    const PERCENT_25: Limits = Limits {
        health: 0,
        percent: 25,
    };

    #[test]
    fn values_out_of_range_are_off_with_a_warning() {
        assert_eq!(
            Limits::from_values(40, 30),
            (
                Limits {
                    health: 40,
                    percent: 30
                },
                vec![]
            )
        );
        let (limits, warnings) = Limits::from_values(-1, 101);
        assert_eq!(limits, Limits::default());
        assert_eq!(
            warnings,
            [
                "Health -1 is negative: off",
                "HealthPercent 101 is not 0 to 100: off"
            ]
        );
        assert_eq!(Limits::from_values(0, -5).0, Limits::default());
    }

    #[test]
    fn either_limit_makes_a_unit_low() {
        let both = Limits {
            health: 50,
            percent: 25,
        };
        assert!(both.is_low(49.0, 1000.0));
        assert!(both.is_low(99.0, 400.0));
        assert!(!both.is_low(100.0, 400.0));
        assert!(!HEALTH_50.is_low(50.0, 60.0));
        assert!(PERCENT_25.is_low(24.9, 100.0));
        assert!(!PERCENT_25.is_low(25.0, 100.0));
        assert!(!Limits::default().is_low(0.0, 100.0));
        assert!(!Limits::default().any());
    }

    #[test]
    fn a_drop_below_the_limit_pauses_once() {
        let mut watch = Watch::default();
        assert_eq!(
            watch.check(HEALTH_50, [(1, 80.0, 100.0), (2, 90.0, 100.0)]),
            None
        );
        assert_eq!(
            watch.check(HEALTH_50, [(1, 40.0, 100.0), (2, 90.0, 100.0)]),
            Some(1)
        );
        assert_eq!(
            watch.check(HEALTH_50, [(1, 30.0, 100.0), (2, 90.0, 100.0)]),
            None
        );
        assert_eq!(
            watch.check(HEALTH_50, [(1, 0.0, 100.0), (2, 90.0, 100.0)]),
            None
        );
    }

    #[test]
    fn of_two_drops_at_once_the_first_is_named() {
        let mut watch = Watch::default();
        watch.check(HEALTH_50, [(1, 80.0, 100.0), (2, 90.0, 100.0)]);
        assert_eq!(
            watch.check(HEALTH_50, [(1, 40.0, 100.0), (2, 10.0, 100.0)]),
            Some(1)
        );
    }

    #[test]
    fn healing_back_to_the_limit_arms_it_again() {
        let mut watch = Watch::default();
        watch.check(HEALTH_50, [(1, 80.0, 100.0)]);
        assert_eq!(watch.check(HEALTH_50, [(1, 40.0, 100.0)]), Some(1));
        assert_eq!(watch.check(HEALTH_50, [(1, 50.0, 100.0)]), None);
        assert_eq!(watch.check(HEALTH_50, [(1, 45.0, 100.0)]), Some(1));
    }

    #[test]
    fn a_member_first_seen_low_does_not_pause() {
        let mut watch = Watch::default();
        assert_eq!(watch.check(PERCENT_25, [(1, 10.0, 100.0)]), None);
        assert_eq!(
            watch.check(PERCENT_25, [(1, 10.0, 100.0), (2, 5.0, 100.0)]),
            None
        );
        let three = [(1, 10.0, 100.0), (2, 5.0, 100.0), (3, 90.0, 100.0)];
        assert_eq!(watch.check(PERCENT_25, three), None);
        assert_eq!(watch.check(PERCENT_25, [(3, 20.0, 100.0)]), Some(3));
    }

    #[test]
    fn a_member_gone_and_back_starts_afresh() {
        let mut watch = Watch::default();
        watch.check(HEALTH_50, [(1, 80.0, 100.0)]);
        watch.check(HEALTH_50, []);
        assert_eq!(watch.check(HEALTH_50, [(1, 10.0, 100.0)]), None);
    }
}
