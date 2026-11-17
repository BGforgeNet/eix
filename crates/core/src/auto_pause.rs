//! AutoPause: the single-player game pauses when a party member's health falls below a limit, an enemy comes into
//! view, an enemy turns on the party or an enemy in view dies, as eix.ini's `[AutoPause]` section sets
//! (`docs/engine.md`, `docs/eix-ini.md`).

use core::fmt;

/// The section of eix.ini.
pub const SECTION: &str = "AutoPause";
/// The key of [`Limits::health`].
pub const HEALTH_KEY: &str = "Health";
/// The key of [`Limits::percent`].
pub const PERCENT_KEY: &str = "HealthPercent";

/// The most limits one key keeps; a longer list loses its tail, with a warning. A fixed capacity keeps [`Limits`],
/// [`Triggers`] and eix.ini's settings `Copy`, and the addon's `static` state needs no allocation.
pub const MAX_LIMITS: usize = 8;

/// The values of one key, in the order given, without 0 and without repeats.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Ladder {
    values: [u32; MAX_LIMITS],
    len: usize,
}

impl Ladder {
    /// No value.
    pub const fn new() -> Self {
        Self {
            values: [0; MAX_LIMITS],
            len: 0,
        }
    }

    /// The values kept.
    pub fn values(&self) -> &[u32] {
        &self.values[..self.len]
    }

    /// The comma-separated list `text` of key `key`, each value at most `max`, and a warning for each entry left
    /// out: one that is not a number, is negative, is above `max` or past [`MAX_LIMITS`]. An empty entry and a 0 add
    /// nothing, silently.
    pub fn parse(key: &str, text: &str, max: u32) -> (Self, Vec<String>) {
        let mut ladder = Self::new();
        let mut warnings = Vec::new();
        for entry in text.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let value = match entry.parse::<i64>() {
                Err(_) => {
                    warnings.push(format!("{key} entry \"{entry}\" is not a number: left out"));
                    continue;
                }
                Ok(v) if v < 0 => {
                    warnings.push(format!("{key} entry {v} is negative: left out"));
                    continue;
                }
                Ok(v) => match u32::try_from(v) {
                    Ok(v) if v <= max => v,
                    _ => {
                        let range = if max == 100 {
                            "not 0 to 100"
                        } else {
                            "too large"
                        };
                        warnings.push(format!("{key} entry {v} is {range}: left out"));
                        continue;
                    }
                },
            };
            if value == 0 || ladder.values().contains(&value) {
                continue;
            }
            if ladder.len == MAX_LIMITS {
                warnings.push(format!(
                    "{key} has more than {MAX_LIMITS} limits: entry {value} left out"
                ));
                continue;
            }
            ladder.values[ladder.len] = value;
            ladder.len += 1;
        }
        (ladder, warnings)
    }
}

impl fmt::Display for Ladder {
    /// The values with commas, or `0` for none.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.len == 0 {
            return f.write_str("0");
        }
        for (i, v) in self.values().iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            write!(f, "{v}")?;
        }
        Ok(())
    }
}

/// The health limits: both keys' lists form one set, in hit points from each member's maximum.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limits {
    /// Hit points.
    pub health: Ladder,
    /// Percentages of the unit's maximum hit points, 1 to 100.
    pub percent: Ladder,
}

/// The limit a member's health fell below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crossed {
    /// The key of the list the limit is in.
    pub key: &'static str,
    /// The limit as that list gives it.
    pub value: u32,
}

impl fmt::Display for Crossed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.key, self.value)
    }
}

impl Limits {
    /// No limit.
    pub const OFF: Self = Self {
        health: Ladder::new(),
        percent: Ladder::new(),
    };

    /// The limits from eix.ini's two lists, and a warning for each entry left out.
    pub fn parse(health: &str, percent: &str) -> (Self, Vec<String>) {
        let (health, mut warnings) = Ladder::parse(HEALTH_KEY, health, u32::MAX);
        let (percent, more) = Ladder::parse(PERCENT_KEY, percent, 100);
        warnings.extend(more);
        (Self { health, percent }, warnings)
    }

    /// Whether any limit is on.
    pub fn any(self) -> bool {
        !self.health.values().is_empty() || !self.percent.values().is_empty()
    }

    /// The lowest limit that a member went below, from `before` hit points at the last check to `now` of
    /// `max_hit_points`: one it was at or above and is now under. Several crossed at once name the lowest. A
    /// percentage and a health limit that are equal in hit points count once (the health limit is named).
    pub fn crossed(self, before: f32, now: f32, max_hit_points: f32) -> Option<Crossed> {
        let (before, now, max) = (
            f64::from(before) * 100.0,
            f64::from(now) * 100.0,
            f64::from(max_hit_points),
        );
        // Each limit in hit points times 100, so a percentage needs no division.
        let health = self.health.values().iter().map(|&v| {
            (
                f64::from(v) * 100.0,
                Crossed {
                    key: HEALTH_KEY,
                    value: v,
                },
            )
        });
        let percent = self.percent.values().iter().map(|&v| {
            (
                max * f64::from(v),
                Crossed {
                    key: PERCENT_KEY,
                    value: v,
                },
            )
        });
        let mut lowest: Option<(f64, Crossed)> = None;
        for (limit, crossed) in health.chain(percent) {
            if before >= limit && now < limit && lowest.is_none_or(|(l, _)| limit < l) {
                lowest = Some((limit, crossed));
            }
        }
        lowest.map(|(_, crossed)| crossed)
    }
}

/// The party members seen at the last check, by unit id, with their hit points then.
#[derive(Debug, Default)]
pub struct Watch {
    seen: Vec<(i32, f32)>,
}

impl Watch {
    /// No member seen yet.
    pub const fn new() -> Self {
        Self { seen: Vec::new() }
    }

    /// Takes the party's units as `(id, hit points, maximum hit points)` and returns the first that pauses the game
    /// now with the limit it crossed: a member seen at the last check, at or above a limit then, is below it now.
    /// One check that crosses several limits pauses once. A limit re-arms when the member's health is back at or
    /// above it. A member seen for the first time (a zone loaded, a new member) only sets its state, so a unit
    /// already low does not pause the game.
    pub fn check(
        &mut self,
        limits: Limits,
        units: impl IntoIterator<Item = (i32, f32, f32)>,
    ) -> Option<(i32, Crossed)> {
        let mut pause = None;
        let seen: Vec<(i32, f32)> = units
            .into_iter()
            .map(|(id, hit_points, max_hit_points)| {
                let before = self.seen.iter().find(|&&(i, _)| i == id).map(|&(_, hp)| hp);
                if let Some(before) = before
                    && pause.is_none()
                {
                    pause = limits
                        .crossed(before, hit_points, max_hit_points)
                        .map(|crossed| (id, crossed));
                }
                (id, hit_points)
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

/// The client ticks an encounter's start waits at most for its readiness test (about four seconds); the start then
/// counts anyway, so a test that never passes cannot lose the pause. Client ticks, not checks: the server can tick
/// while the client does not draw (a zone loading), and the test reads what the client draws.
pub const MAX_WAIT: u32 = 60;

/// How a waiting start of an encounter came to be told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Told {
    /// The readiness test passed.
    Ready,
    /// The test had not passed after [`MAX_WAIT`] client ticks.
    Capped,
}

/// A start that waits: the client ticks counted so far, and the client's tick count at the last check.
#[derive(Debug, Clone, Copy)]
struct Wait {
    ticks: u32,
    last: i32,
}

/// An [`Onset`] that tells only once a readiness test passes, as long as the encounter lasts.
#[derive(Debug, Default)]
pub struct ReadyOnset {
    onset: Onset,
    /// The wait of a start not told yet, or `None` when nothing waits.
    waited: Option<Wait>,
}

impl ReadyOnset {
    /// No enemy yet.
    pub const fn new() -> Self {
        Self {
            onset: Onset::new(),
            waited: None,
        }
    }

    /// Takes whether the set holds an enemy now and the client's tick count `client_ticks`, and tells whether and how
    /// the start of its encounter is told: `ready` runs only while a start waits, and the wait ends when it passes,
    /// after [`MAX_WAIT`] client ticks or when no enemy is left. A tick count that falls (a zone loaded anew) adds
    /// nothing.
    pub fn check(
        &mut self,
        any: bool,
        client_ticks: i32,
        ready: impl FnOnce() -> bool,
    ) -> Option<Told> {
        match self.check_provoked(any, client_ticks, || false, ready) {
            Some(Start::Told(told)) => Some(told),
            Some(Start::Provoked) | None => None,
        }
    }

    /// As [`check`](Self::check), but a start for which `provoked` holds (asked once, as the encounter starts) is
    /// [`Start::Provoked`] at once: the encounter counts as started, and nothing waits or is told for it.
    pub fn check_provoked(
        &mut self,
        any: bool,
        client_ticks: i32,
        provoked: impl FnOnce() -> bool,
        ready: impl FnOnce() -> bool,
    ) -> Option<Start> {
        if self.onset.check(any) {
            if provoked() {
                self.waited = None;
                return Some(Start::Provoked);
            }
            self.waited = Some(Wait {
                ticks: 0,
                last: client_ticks,
            });
        }
        let wait = self.waited?;
        if !any {
            self.waited = None;
            return None;
        }
        let ticks = wait
            .ticks
            .saturating_add(u32::try_from(client_ticks.saturating_sub(wait.last)).unwrap_or(0));
        if ready() {
            self.waited = None;
            return Some(Start::Told(Told::Ready));
        }
        if ticks >= MAX_WAIT {
            self.waited = None;
            return Some(Start::Told(Told::Capped));
        }
        self.waited = Some(Wait {
            ticks,
            last: client_ticks,
        });
        None
    }
}

/// How the start of an encounter that the party may have provoked comes out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// Told, as [`ReadyOnset::check`] tells it.
    Told(Told),
    /// The party struck first: the encounter started, untold.
    Provoked,
}

/// The server ticks a party unit's fight with a unit still counts once the fight's action is over (about two
/// seconds): a spell flies on after its cast, and the unit hit turns on its attacker at its own AI's next update.
pub const ENGAGED_TICKS: u32 = 30;

/// The units the party fights, or fought within the last [`ENGAGED_TICKS`] server ticks.
#[derive(Debug, Default)]
pub struct Engaged {
    /// The server ticks counted.
    now: u32,
    /// Each unit fought, by id, and the tick its fight was last seen.
    seen: Vec<(i32, u32)>,
}

impl Engaged {
    /// No fight yet.
    pub const fn new() -> Self {
        Self {
            now: 0,
            seen: Vec::new(),
        }
    }

    /// Takes the ids of the units the party fights at this server tick.
    pub fn tick(&mut self, fought: impl IntoIterator<Item = i32>) {
        self.now = self.now.wrapping_add(1);
        let now = self.now;
        self.seen
            .retain(|&(_, at)| now.wrapping_sub(at) <= ENGAGED_TICKS);
        for id in fought {
            match self.seen.iter_mut().find(|(seen, _)| *seen == id) {
                Some(entry) => entry.1 = now,
                None => self.seen.push((id, now)),
            }
        }
    }

    /// Whether the party fights unit `id`, or fought it within the last [`ENGAGED_TICKS`] server ticks.
    pub fn contains(&self, id: i32) -> bool {
        self.seen.iter().any(|&(seen, _)| seen == id)
    }
}

/// The first of the units that died, `deaths`, that was among the enemies in view, `in_view`.
pub fn death_in_view(deaths: &[i32], in_view: &[i32]) -> Option<i32> {
    deaths.iter().copied().find(|id| in_view.contains(id))
}

/// What a unit's fade-in gains each client tick: the game adds 1/15 a tick, so a fade-in from 0 lasts 15 ticks.
pub const FADE_PER_TICK: f32 = 1.0 / 15.0;

/// A unit's fade-in `fade` after `elapsed` of paused frames, at the pace the game's client ticks would give it:
/// [`FADE_PER_TICK`] each `period_ms` (the game speed's milliseconds per tick), up to 1. A period that is not
/// positive leaves the fade as it is.
pub fn paused_fade(fade: f32, elapsed: core::time::Duration, period_ms: i32) -> f32 {
    if period_ms <= 0 {
        return fade;
    }
    let ticks = elapsed.as_secs_f64() * 1000.0 / f64::from(period_ms);
    (f64::from(fade) + ticks * f64::from(FADE_PER_TICK)).min(1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The limits of two eix.ini values, with no warning.
    fn lim(health: &str, percent: &str) -> Limits {
        let (limits, warnings) = Limits::parse(health, percent);
        assert_eq!(warnings, Vec::<String>::new());
        limits
    }

    fn health_50() -> Limits {
        lim("50", "")
    }

    fn percents(percent: &str) -> Limits {
        lim("", percent)
    }

    #[test]
    fn a_list_takes_spaces_and_one_value_as_before() {
        assert_eq!(lim("40", "30").health.values(), [40]);
        assert_eq!(lim("40", "30").percent.values(), [30]);
        assert_eq!(percents("75,50,10").percent.values(), [75, 50, 10]);
        assert_eq!(percents("  75 ,50 ,   10 ").percent.values(), [75, 50, 10]);
        assert_eq!(lim("100, 50", "").health.values(), [100, 50]);
    }

    #[test]
    fn an_empty_value_and_a_0_are_off() {
        for text in ["", "  ", "0", " 0 ", "0,0", ",", "75,,"] {
            let (limits, warnings) = Limits::parse(text, text);
            assert!(warnings.is_empty(), "{text:?}: {warnings:?}");
            assert_eq!(limits.any(), text.contains("75"), "{text:?}");
        }
        assert_eq!(Limits::parse("", "").0, Limits::OFF);
        // A 0 inside a list adds no limit; the rest applies.
        assert_eq!(percents("0,50,0,25").percent.values(), [50, 25]);
        assert_eq!(percents("75,,50").percent.values(), [75, 50]);
    }

    #[test]
    fn a_bad_entry_is_left_out_with_a_warning() {
        let (limits, warnings) = Limits::parse("-1, 20, abc", "75, 101, x1, 50, 1.5, 10");
        assert_eq!(limits.health.values(), [20]);
        assert_eq!(limits.percent.values(), [75, 50, 10]);
        assert_eq!(
            warnings,
            [
                "Health entry -1 is negative: left out",
                "Health entry \"abc\" is not a number: left out",
                "HealthPercent entry 101 is not 0 to 100: left out",
                "HealthPercent entry \"x1\" is not a number: left out",
                "HealthPercent entry \"1.5\" is not a number: left out",
            ]
        );
        let (limits, warnings) = Limits::parse("99999999999", "-5");
        assert!(!limits.any());
        assert_eq!(
            warnings,
            [
                "Health entry 99999999999 is too large: left out",
                "HealthPercent entry -5 is negative: left out"
            ]
        );
    }

    #[test]
    fn repeats_count_once_and_the_list_has_a_cap() {
        assert_eq!(percents("50,75,50,75").percent.values(), [50, 75]);
        let long = (1..=MAX_LIMITS as u32 + 2)
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let (limits, warnings) = Limits::parse(&long, "");
        assert_eq!(limits.health.values().len(), MAX_LIMITS);
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].starts_with("Health has more than 8 limits: entry 9"));
    }

    #[test]
    fn the_lists_show_as_written_or_0() {
        let limits = lim("", " 75 , 50,10");
        assert_eq!(limits.health.to_string(), "0");
        assert_eq!(limits.percent.to_string(), "75,50,10");
    }

    #[test]
    fn either_key_makes_a_limit() {
        let both = lim("50", "25");
        assert!(both.crossed(100.0, 49.0, 1000.0).is_some());
        assert!(both.crossed(100.0, 99.0, 400.0).is_some());
        assert_eq!(both.crossed(200.0, 100.0, 400.0), None);
        assert_eq!(health_50().crossed(60.0, 50.0, 60.0), None);
        assert_eq!(
            percents("25").crossed(30.0, 24.9, 100.0),
            Some(Crossed {
                key: PERCENT_KEY,
                value: 25
            })
        );
        assert_eq!(percents("25").crossed(30.0, 25.0, 100.0), None);
        assert_eq!(Limits::OFF.crossed(100.0, 0.0, 100.0), None);
        assert!(!Limits::OFF.any());
    }

    #[test]
    fn a_drop_below_the_limit_pauses_once() {
        let mut watch = Watch::default();
        let hp50 = Crossed {
            key: HEALTH_KEY,
            value: 50,
        };
        assert_eq!(
            watch.check(health_50(), [(1, 80.0, 100.0), (2, 90.0, 100.0)]),
            None
        );
        assert_eq!(
            watch.check(health_50(), [(1, 40.0, 100.0), (2, 90.0, 100.0)]),
            Some((1, hp50))
        );
        assert_eq!(
            watch.check(health_50(), [(1, 30.0, 100.0), (2, 90.0, 100.0)]),
            None
        );
        assert_eq!(
            watch.check(health_50(), [(1, 0.0, 100.0), (2, 90.0, 100.0)]),
            None
        );
    }

    #[test]
    fn of_two_drops_at_once_the_first_is_named() {
        let mut watch = Watch::default();
        watch.check(health_50(), [(1, 80.0, 100.0), (2, 90.0, 100.0)]);
        assert_eq!(
            watch
                .check(health_50(), [(1, 40.0, 100.0), (2, 10.0, 100.0)])
                .map(|p| p.0),
            Some(1)
        );
    }

    #[test]
    fn healing_back_to_the_limit_arms_it_again() {
        let mut watch = Watch::default();
        watch.check(health_50(), [(1, 80.0, 100.0)]);
        assert!(watch.check(health_50(), [(1, 40.0, 100.0)]).is_some());
        assert_eq!(watch.check(health_50(), [(1, 50.0, 100.0)]), None);
        assert!(watch.check(health_50(), [(1, 45.0, 100.0)]).is_some());
    }

    /// The percentage limit a check of the ladder `75,50,10` on a maximum of 100 names, if any.
    fn step(watch: &mut Watch, limits: Limits, hp: f32) -> Option<u32> {
        watch
            .check(limits, [(1, hp, 100.0)])
            .map(|(_, crossed)| crossed.value)
    }

    #[test]
    fn each_limit_of_a_ladder_pauses_once() {
        let l = percents("75,50,10");
        let mut watch = Watch::default();
        assert_eq!(step(&mut watch, l, 100.0), None);
        assert_eq!(step(&mut watch, l, 80.0), None);
        assert_eq!(step(&mut watch, l, 70.0), Some(75));
        assert_eq!(step(&mut watch, l, 60.0), None);
        assert_eq!(step(&mut watch, l, 49.0), Some(50));
        assert_eq!(step(&mut watch, l, 30.0), None);
        assert_eq!(step(&mut watch, l, 9.0), Some(10));
        assert_eq!(step(&mut watch, l, 1.0), None);
    }

    #[test]
    fn several_limits_crossed_in_one_check_pause_once() {
        let l = percents("75,50,10");
        let mut watch = Watch::default();
        step(&mut watch, l, 80.0);
        // 80 to 40 passes 75 and 50: one pause, naming the lowest.
        assert_eq!(step(&mut watch, l, 40.0), Some(50));
        // Both are spent: no pause until the next limit.
        assert_eq!(step(&mut watch, l, 30.0), None);
        assert_eq!(step(&mut watch, l, 5.0), Some(10));
    }

    #[test]
    fn healing_re_arms_only_the_limits_it_reaches() {
        let l = percents("75,50,10");
        let mut watch = Watch::default();
        step(&mut watch, l, 80.0);
        assert_eq!(step(&mut watch, l, 45.0), Some(50));
        // 45 healed to 60 re-arms 50, not 75.
        assert_eq!(step(&mut watch, l, 60.0), None);
        assert_eq!(step(&mut watch, l, 55.0), None);
        assert_eq!(step(&mut watch, l, 49.0), Some(50));
        // Back at 75 re-arms that too.
        assert_eq!(step(&mut watch, l, 75.0), None);
        assert_eq!(step(&mut watch, l, 74.0), Some(75));
    }

    #[test]
    fn the_two_keys_form_one_set_of_limits() {
        // Health 30 is 30 hit points; 50 percent of 100 is 50; 25 percent of 200 is 50 too.
        let l = lim("30", "50");
        let mut watch = Watch::default();
        watch.check(l, [(1, 100.0, 100.0), (2, 200.0, 200.0)]);
        let first = watch.check(l, [(1, 40.0, 100.0), (2, 190.0, 200.0)]);
        assert_eq!(
            first,
            Some((
                1,
                Crossed {
                    key: PERCENT_KEY,
                    value: 50
                }
            ))
        );
        // Unit 1 now crosses its health limit; unit 2 its 50 percent (100 of 200).
        let second = watch.check(l, [(1, 20.0, 100.0), (2, 90.0, 200.0)]);
        assert_eq!(
            second,
            Some((
                1,
                Crossed {
                    key: HEALTH_KEY,
                    value: 30
                }
            ))
        );
        let third = watch.check(l, [(1, 20.0, 100.0), (2, 25.0, 200.0)]);
        assert_eq!(third.map(|p| p.0), Some(2));
    }

    #[test]
    fn limits_equal_in_hit_points_count_once() {
        // 50 hit points and 50 percent of 100 are one limit: one pause, the health limit named.
        let l = lim("50", "50");
        let mut watch = Watch::default();
        watch.check(l, [(1, 80.0, 100.0)]);
        assert_eq!(
            watch.check(l, [(1, 40.0, 100.0)]),
            Some((
                1,
                Crossed {
                    key: HEALTH_KEY,
                    value: 50
                }
            ))
        );
        assert_eq!(watch.check(l, [(1, 30.0, 100.0)]), None);
    }

    #[test]
    fn a_member_first_seen_low_does_not_pause() {
        let mut watch = Watch::default();
        let l = percents("25");
        assert_eq!(watch.check(l, [(1, 10.0, 100.0)]), None);
        assert_eq!(watch.check(l, [(1, 10.0, 100.0), (2, 5.0, 100.0)]), None);
        let three = [(1, 10.0, 100.0), (2, 5.0, 100.0), (3, 90.0, 100.0)];
        assert_eq!(watch.check(l, three), None);
        assert_eq!(watch.check(l, [(3, 20.0, 100.0)]).map(|p| p.0), Some(3));
        // A ladder's first sight sets the state only, below every limit or between two.
        let l = percents("75,50,10");
        let mut watch = Watch::default();
        assert_eq!(watch.check(l, [(1, 40.0, 100.0), (2, 5.0, 100.0)]), None);
        // From 40 only the limit below it is left to cross.
        assert_eq!(step(&mut watch, l, 9.0), Some(10));
    }

    #[test]
    fn a_member_gone_and_back_starts_afresh() {
        let mut watch = Watch::default();
        watch.check(health_50(), [(1, 80.0, 100.0)]);
        watch.check(health_50(), []);
        assert_eq!(watch.check(health_50(), [(1, 10.0, 100.0)]), None);
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
    fn a_ready_encounter_starts_once_its_test_passes() {
        let mut onset = ReadyOnset::new();
        assert_eq!(onset.check(false, 0, || unreachable!()), None);
        assert_eq!(onset.check(true, 1, || false), None);
        assert_eq!(onset.check(true, 2, || false), None);
        assert_eq!(onset.check(true, 3, || true), Some(Told::Ready));
        // Once told, the encounter stays told, and the test no longer runs.
        assert_eq!(onset.check(true, 4, || unreachable!()), None);
        assert_eq!(onset.check(false, 5, || unreachable!()), None);
        // The next encounter waits again.
        assert_eq!(onset.check(true, 6, || false), None);
        assert_eq!(onset.check(true, 7, || true), Some(Told::Ready));
    }

    #[test]
    fn a_ready_encounter_that_ends_while_waiting_is_never_told() {
        let mut onset = ReadyOnset::new();
        assert_eq!(onset.check(true, 0, || false), None);
        assert_eq!(onset.check(false, 1, || unreachable!()), None);
        assert_eq!(onset.check(false, 2, || unreachable!()), None);
        assert_eq!(onset.check(true, 3, || false), None);
        assert_eq!(onset.check(true, 4, || true), Some(Told::Ready));
    }

    #[test]
    fn a_ready_encounter_whose_test_never_passes_starts_after_the_limit() {
        let mut onset = ReadyOnset::new();
        let limit = i32::try_from(MAX_WAIT).unwrap_or(i32::MAX);
        for tick in 0..limit {
            assert_eq!(onset.check(true, tick, || false), None);
        }
        assert_eq!(onset.check(true, limit, || false), Some(Told::Capped));
        assert_eq!(onset.check(true, limit + 1, || false), None);
    }

    #[test]
    fn the_limit_counts_client_ticks_not_checks() {
        let mut onset = ReadyOnset::new();
        // Checks while the client does not tick (a zone loading) use none of the wait.
        for _ in 0..MAX_WAIT * 2 {
            assert_eq!(onset.check(true, 10, || false), None);
        }
        // A tick count that starts again (the zone loaded anew) adds nothing for the fall.
        assert_eq!(onset.check(true, 0, || false), None);
        assert_eq!(onset.check(true, 59, || false), None);
        assert_eq!(onset.check(true, 60, || false), Some(Told::Capped));
    }

    #[test]
    fn a_passing_test_is_told_as_ready_even_at_the_limit() {
        let mut onset = ReadyOnset::new();
        assert_eq!(onset.check(true, 0, || false), None);
        assert_eq!(onset.check(true, 100, || true), Some(Told::Ready));
    }

    #[test]
    fn a_provoked_encounter_starts_untold_and_stays_started() {
        let mut onset = ReadyOnset::new();
        assert_eq!(
            onset.check_provoked(true, 0, || true, || unreachable!()),
            Some(Start::Provoked)
        );
        // Others joining the encounter are no new start, provoked or not.
        assert_eq!(
            onset.check_provoked(true, 1, || unreachable!(), || unreachable!()),
            None
        );
        assert_eq!(
            onset.check_provoked(false, 2, || unreachable!(), || unreachable!()),
            None
        );
        // The next encounter that the party did not provoke is told.
        assert_eq!(onset.check_provoked(true, 3, || false, || false), None);
        assert_eq!(
            onset.check_provoked(true, 4, || unreachable!(), || true),
            Some(Start::Told(Told::Ready))
        );
    }

    #[test]
    fn a_fight_counts_while_it_lasts_and_a_while_after() {
        let mut engaged = Engaged::new();
        engaged.tick([7]);
        assert!(engaged.contains(7));
        assert!(!engaged.contains(8));
        for _ in 0..ENGAGED_TICKS {
            engaged.tick([]);
        }
        assert!(engaged.contains(7));
        engaged.tick([]);
        assert!(!engaged.contains(7));
        // A fight seen again starts its time afresh.
        engaged.tick([8]);
        for _ in 0..ENGAGED_TICKS - 1 {
            engaged.tick([]);
        }
        engaged.tick([8]);
        for _ in 0..ENGAGED_TICKS {
            engaged.tick([]);
        }
        assert!(engaged.contains(8));
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
                limits: health_50(),
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

    #[test]
    fn a_paused_fade_in_keeps_the_games_pace() {
        use core::time::Duration;
        let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
        // One tick at the normal speed (55 ms) and at the double speed (27 ms).
        assert!(close(
            paused_fade(0.0, Duration::from_millis(55), 55),
            FADE_PER_TICK
        ));
        assert!(close(
            paused_fade(0.0, Duration::from_millis(27), 27),
            FADE_PER_TICK
        ));
        // 15 ticks fade a unit in from 0; half of them half of the way.
        assert!(close(
            paused_fade(0.0, Duration::from_millis(15 * 55), 55),
            1.0
        ));
        assert!(close(
            paused_fade(0.25, Duration::from_micros(15 * 55 * 1000 / 2), 55),
            0.75
        ));
        // Frames add up as the ticks would.
        let mut fade = 0.0;
        for _ in 0..5 {
            fade = paused_fade(fade, Duration::from_millis(11), 55);
        }
        assert!(close(fade, FADE_PER_TICK));
        assert_eq!(paused_fade(0.9, Duration::from_secs(5), 55), 1.0);
        assert_eq!(paused_fade(0.3, Duration::from_secs(1), 0), 0.3);
        assert_eq!(paused_fade(0.3, Duration::ZERO, 55), 0.3);
    }
}
