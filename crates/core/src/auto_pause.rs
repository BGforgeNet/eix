//! AutoPause: the single-player game pauses when a party member's health falls below a limit, an enemy comes into
//! view, an enemy turns on the party or an enemy in view dies, as eix.ini's `[AutoPause]` section sets
//! (`docs/engine.md`, `docs/eix-ini.md`).

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

/// The key of [`Triggers::enemy_sighted`].
pub const SIGHTED_KEY: &str = "EnemySighted";
/// The key of [`Triggers::enemy_aggro`].
pub const AGGRO_KEY: &str = "EnemyAggro";
/// The key of [`Triggers::enemy_dead`].
pub const DEAD_KEY: &str = "EnemyDead";

/// Everything that pauses the game.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Triggers {
    /// The health limits.
    pub limits: Limits,
    /// An enemy comes into the party's view while none was in view.
    pub enemy_sighted: bool,
    /// An enemy's aggression turns on a party member while no enemy's was.
    pub enemy_aggro: bool,
    /// An enemy in the party's view dies.
    pub enemy_dead: bool,
}

impl Triggers {
    /// Whether anything pauses the game.
    pub fn any(self) -> bool {
        self.limits.any() || self.enemy_sighted || self.enemy_aggro || self.enemy_dead
    }

    /// Whether a trigger needs the enemies in the party's view.
    pub fn needs_view(self) -> bool {
        self.enemy_sighted || self.enemy_dead
    }
}

/// Whether a unit with the side masks `hostile` and `hostile_limit` is an enemy of side `side`: the side's bit is
/// set in both, as the game's hostility tests take it (the shift counts modulo 32).
pub fn is_enemy(hostile: u32, hostile_limit: u32, side: u32) -> bool {
    1u32.wrapping_shl(side) & hostile & hostile_limit != 0
}

/// The start of an encounter: a set of enemies (those in view, or those aggressive) that was empty becomes non-empty.
#[derive(Debug, Default)]
pub struct Onset {
    active: bool,
}

impl Onset {
    /// No enemy yet.
    pub const fn new() -> Self {
        Self { active: false }
    }

    /// Takes whether the set holds an enemy now and tells whether that starts an encounter.
    pub fn check(&mut self, any: bool) -> bool {
        let start = any && !self.active;
        self.active = any;
        start
    }
}

/// The first of the units that died, `deaths`, that was among the enemies in view, `in_view`.
pub fn death_in_view(deaths: &[i32], in_view: &[i32]) -> Option<i32> {
    deaths.iter().copied().find(|id| in_view.contains(id))
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

    #[test]
    fn an_enemy_has_the_sides_bit_in_both_masks() {
        assert!(is_enemy(0b1, u32::MAX, 0));
        assert!(!is_enemy(0b10, u32::MAX, 0));
        assert!(is_enemy(0b10, u32::MAX, 1));
        assert!(!is_enemy(0b1, 0b10, 0));
        assert!(!is_enemy(0, u32::MAX, 0));
        // The game's shift takes the count modulo 32.
        assert!(is_enemy(0b1, u32::MAX, 32));
        assert!(is_enemy(1 << 31, u32::MAX, 31));
    }

    #[test]
    fn an_encounter_starts_once_until_no_enemy_is_left() {
        let mut onset = Onset::new();
        assert!(!onset.check(false));
        assert!(onset.check(true));
        assert!(!onset.check(true));
        assert!(!onset.check(false));
        assert!(onset.check(true));
    }

    #[test]
    fn only_a_death_in_view_counts() {
        assert_eq!(death_in_view(&[5, 7], &[1, 7, 9]), Some(7));
        assert_eq!(death_in_view(&[5], &[1, 7]), None);
        assert_eq!(death_in_view(&[], &[1]), None);
        assert_eq!(death_in_view(&[3, 1], &[1, 3]), Some(3));
    }

    #[test]
    fn any_trigger_switches_autopause_on() {
        assert!(!Triggers::default().any());
        let on = |t: Triggers| (t.any(), t.needs_view());
        let none = Triggers::default();
        assert_eq!(
            on(Triggers {
                limits: HEALTH_50,
                ..none
            }),
            (true, false)
        );
        assert_eq!(
            on(Triggers {
                enemy_sighted: true,
                ..none
            }),
            (true, true)
        );
        assert_eq!(
            on(Triggers {
                enemy_aggro: true,
                ..none
            }),
            (true, false)
        );
        assert_eq!(
            on(Triggers {
                enemy_dead: true,
                ..none
            }),
            (true, true)
        );
    }
}
