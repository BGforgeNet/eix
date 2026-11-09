//! eix.ini, eix's own settings in the mod folder (`docs/eix-ini.md`): `[EngineFixes]` and `[AutoPause]`.

use crate::auto_pause::{self, Limits, Triggers};

/// The section of [`FixSettings`].
pub const FIXES_SECTION: &str = "EngineFixes";

/// The `[EngineFixes]` switches with their defaults; 0 is off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixSettings {
    /// `Enabled`: 0 leaves out every fix beyond the originals. Default 1.
    pub engine_fixes: u32,
    /// `KeepWindowOnFocusLoss`: 0 lets a windowed game minimise when it loses the focus, as the game does. Default 1.
    pub keep_window_on_focus_loss: u32,
    /// `SmoothCamera`: 0 moves the camera in whole game ticks, as the game does. Default 1.
    pub smooth_camera: u32,
    /// `PlayerListOnJoin`: 0 leaves a joining client's player list empty until the next change of players, as the
    /// game does. Default 1.
    pub player_list_on_join: u32,
    /// `SkipDiscCheck`: 0 lets the starter's game.exe ask for the game disc, as it does. Default 1.
    pub skip_disc_check: u32,
    /// `UnitShadowResolution`: the side in pixels of a unit's shadow texture; 64 is the game's, 128 and 256 are
    /// sharper ([`crate::engine_fixes::unit_shadow_size`]). Default 64.
    pub unit_shadow_resolution: u32,
    /// `KeepRunningOnFocusLoss`: nonzero keeps a windowed game running when it loses the focus. Default 0: the game
    /// pauses, as it does without eix (a single-player game would otherwise go on while the player is away).
    pub keep_running_on_focus_loss: u32,
    /// `ClipCursorToWindow`: nonzero keeps the cursor inside a focused windowed game. Default 0.
    pub clip_cursor_to_window: u32,
    /// `UnitPanelSpells`: 0 leaves a unit outside the player's party without its spells on the unit panel's values
    /// page, as the game shows it. Default 1.
    pub unit_panel_spells: u32,
}

impl Default for FixSettings {
    fn default() -> Self {
        Self::read(|_, default| default)
    }
}

impl FixSettings {
    /// The switches from `int(key, default)`, a reader of `[EngineFixes]`.
    fn read(int: impl Fn(&str, i32) -> i32) -> Self {
        // A switch keeps the reader's bits, as the registry DWORDs did: any nonzero value is on.
        let value = |key: &str, default: u32| int(key, default.cast_signed()).cast_unsigned();
        Self {
            engine_fixes: value("Enabled", 1),
            keep_window_on_focus_loss: value("KeepWindowOnFocusLoss", 1),
            smooth_camera: value("SmoothCamera", 1),
            player_list_on_join: value("PlayerListOnJoin", 1),
            skip_disc_check: value("SkipDiscCheck", 1),
            unit_shadow_resolution: value("UnitShadowResolution", 64),
            keep_running_on_focus_loss: value("KeepRunningOnFocusLoss", 0),
            clip_cursor_to_window: value("ClipCursorToWindow", 0),
            unit_panel_spells: value("UnitPanelSpells", 1),
        }
    }
}

/// Everything eix.ini sets; a missing file or key takes the defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EixIni {
    /// `[EngineFixes]`.
    pub fixes: FixSettings,
    /// `[AutoPause]`.
    pub auto_pause: Triggers,
}

impl EixIni {
    /// The settings from `int(section, key, default)`, the system's profile reader on the file, and a warning for
    /// each value left out.
    pub fn read(int: impl Fn(&str, &str, i32) -> i32) -> (Self, Vec<String>) {
        let fixes = FixSettings::read(|key, default| int(FIXES_SECTION, key, default));
        let pause = |key| int(auto_pause::SECTION, key, 0);
        let (limits, warnings) = Limits::from_values(
            pause(auto_pause::HEALTH_KEY),
            pause(auto_pause::PERCENT_KEY),
        );
        let auto_pause = Triggers {
            limits,
            enemy_sighted: pause(auto_pause::SIGHTED_KEY) != 0,
            enemy_aggro: pause(auto_pause::AGGRO_KEY) != 0,
            enemy_dead: pause(auto_pause::DEAD_KEY) != 0,
        };
        (Self { fixes, auto_pause }, warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// eix.ini with the one value `section` / `key` = `value`.
    fn with(section: &str, key: &str, value: i32) -> EixIni {
        EixIni::read(|s, k, default| {
            if s.eq_ignore_ascii_case(section) && k.eq_ignore_ascii_case(key) {
                value
            } else {
                default
            }
        })
        .0
    }

    #[test]
    fn no_file_gives_the_defaults() {
        let none = EixIni::read(|_, _, default| default).0;
        assert_eq!(none, EixIni::default());
        let f = none.fixes;
        assert_eq!(
            (
                f.engine_fixes,
                f.keep_window_on_focus_loss,
                f.smooth_camera,
                f.player_list_on_join,
                f.skip_disc_check,
                f.unit_panel_spells
            ),
            (1, 1, 1, 1, 1, 1)
        );
        assert_eq!(
            (
                f.unit_shadow_resolution,
                f.keep_running_on_focus_loss,
                f.clip_cursor_to_window
            ),
            (64, 0, 0)
        );
        assert!(!none.auto_pause.any());
    }

    #[test]
    fn each_switch_reads_its_own_key() {
        let fixes = |key, value| with(FIXES_SECTION, key, value).fixes;
        assert_eq!(fixes("Enabled", 0).engine_fixes, 0);
        assert_eq!(
            fixes("KeepWindowOnFocusLoss", 0).keep_window_on_focus_loss,
            0
        );
        assert_eq!(fixes("SmoothCamera", 0).smooth_camera, 0);
        assert_eq!(fixes("PlayerListOnJoin", 0).player_list_on_join, 0);
        assert_eq!(fixes("SkipDiscCheck", 0).skip_disc_check, 0);
        assert_eq!(
            fixes("UnitShadowResolution", 256).unit_shadow_resolution,
            256
        );
        assert_eq!(
            fixes("KeepRunningOnFocusLoss", 1).keep_running_on_focus_loss,
            1
        );
        assert_eq!(fixes("ClipCursorToWindow", 1).clip_cursor_to_window, 1);
        assert_eq!(fixes("UnitPanelSpells", 0).unit_panel_spells, 0);
        assert_eq!(fixes("SmoothCamera", 0).unit_panel_spells, 1);
    }

    #[test]
    fn a_negative_switch_is_on_and_keeps_its_bits() {
        assert_eq!(
            with(FIXES_SECTION, "SmoothCamera", -1).fixes.smooth_camera,
            u32::MAX
        );
    }

    #[test]
    fn auto_pause_reads_its_section() {
        let ini = with("AutoPause", "HealthPercent", 30);
        assert_eq!(ini.auto_pause.limits.percent, 30);
        assert_eq!(
            with(FIXES_SECTION, "HealthPercent", 30)
                .auto_pause
                .limits
                .percent,
            0
        );
        let pause = |key| with("AutoPause", key, 1).auto_pause;
        let t = pause("EnemySighted");
        assert_eq!(
            (t.enemy_sighted, t.enemy_aggro, t.enemy_dead),
            (true, false, false)
        );
        let t = pause("EnemyAggro");
        assert_eq!(
            (t.enemy_sighted, t.enemy_aggro, t.enemy_dead),
            (false, true, false)
        );
        let t = pause("EnemyDead");
        assert_eq!(
            (t.enemy_sighted, t.enemy_aggro, t.enemy_dead),
            (false, false, true)
        );
        assert!(!with(FIXES_SECTION, "EnemyDead", 1).auto_pause.enemy_dead);
        let (_, warnings) = EixIni::read(|s, k, d| {
            if s == "AutoPause" && k == "Health" {
                -5
            } else {
                d
            }
        });
        assert_eq!(warnings, ["Health -5 is negative: off"]);
    }
}
