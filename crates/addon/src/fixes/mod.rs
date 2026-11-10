//! eix's own fixes of game.exe defects, beyond what the original addon and SpellAddon change
//! (`docs/engine.md`). None writes a site a SpellAddon feature patches: SpellAddon applies later, and a
//! feature that found our bytes would be left out. A fix a player can see has a setting of its own in eix.ini's
//! `[EngineFixes]` that turns it off; `Enabled` = 0 there leaves them all out, but not the mod settings
//! ([`MOD_SETTINGS`]).

use std::sync::atomic::{AtomicU32, Ordering};

use crate::game::BeforeWinMain;
use crate::groups::{Build, Tally, apply_built};
use crate::log;
use crate::setup::Setup;

mod accepted_skills;
mod briefing_text;
mod cast_target_gone;
mod diplomacy_sides;
mod disc_check;
mod empty_item_strip;
mod empty_music_theme;
mod exit_index;
mod fatal_error_exit;
mod find_handle_leak;
mod full_screen_fallback;
mod heap_padding;
mod keep_window;
mod kill_script_outside;
mod load_failure_leaks;
mod lowercase_table;
mod mapped_file_message;
mod missing_player_mode;
mod no_audio_device;
mod player_list_on_join;
mod point_light_overflow;
mod present_failure;
mod quest_sort;
mod screenshot_row_padding;
mod show_tutorial_default;
mod smooth_camera;
mod stream_fade_lock;
mod stream_wait;
mod timer_resolution;
mod truncated_script;
mod unit_panel_spells;
mod unit_panel_text;
mod unit_shadow;
mod unknown_screen;
mod unregistered_light;
mod vertex_lighting;
mod view_depth_init;
mod window_focus;
mod zone_mode_text;

/// A fix's warnings, of which the first [`Warnings::MAX`] go to addon.log: a defect that recurs (each frame, each
/// message of a host) would fill it.
pub(super) struct Warnings {
    fix: &'static str,
    count: AtomicU32,
}

impl Warnings {
    const MAX: u32 = 16;

    pub(super) const fn new(fix: &'static str) -> Self {
        Self {
            fix,
            count: AtomicU32::new(0),
        }
    }

    /// Logs `what()` unless the fix has logged its share.
    pub(super) fn warn(&self, what: impl FnOnce() -> String) {
        if self.count.fetch_add(1, Ordering::Relaxed) < Self::MAX {
            log::warn("fixes", &what(), self.fix);
        }
    }
}

/// What turns a fix off: the reason it is off, or `None` to apply it.
type Switch = fn(&Setup) -> Option<String>;

/// A visible fix's own registry setting: off when it is 0.
fn setting(value: u32, name: &str) -> Option<String> {
    (value == 0).then(|| format!("{name} is 0"))
}

/// Every fix, by its name in addon.log, in the order they apply, with its own switch if it has one.
const FIXES: &[(&str, Option<Switch>, Build)] = &[
    ("HeapPadding", None, heap_padding::build),
    ("TimerResolution", None, timer_resolution::build),
    ("CastTargetGone", None, cast_target_gone::build),
    ("KillScriptOutside", None, kill_script_outside::build),
    ("LowercaseTable", None, lowercase_table::build),
    ("ViewDepthInit", None, view_depth_init::build),
    ("MappedFileMessage", None, mapped_file_message::build),
    ("PresentFailure", None, present_failure::build),
    ("FullScreenFallback", None, full_screen_fallback::build),
    ("ScreenshotRowPadding", None, screenshot_row_padding::build),
    ("BriefingText", None, briefing_text::build),
    ("UnitPanelText", None, unit_panel_text::build),
    ("StreamWait", None, stream_wait::build),
    ("QuestSort", None, quest_sort::build),
    ("ExitIndex", None, exit_index::build),
    ("FatalErrorExit", None, fatal_error_exit::build),
    ("UnknownScreen", None, unknown_screen::build),
    ("DiplomacySides", None, diplomacy_sides::build),
    ("MissingPlayerMode", None, missing_player_mode::build),
    ("NoAudioDevice", None, no_audio_device::build),
    ("EmptyMusicTheme", None, empty_music_theme::build),
    ("ZoneModeText", None, zone_mode_text::build),
    ("StreamFadeLock", None, stream_fade_lock::build),
    ("PointLightOverflow", None, point_light_overflow::build),
    ("EmptyItemStrip", None, empty_item_strip::build),
    ("UnregisteredLight", None, unregistered_light::build),
    ("FindHandleLeak", None, find_handle_leak::build),
    ("LoadFailureLeaks", None, load_failure_leaks::build),
    ("TruncatedScript", None, truncated_script::build),
    ("VertexLighting", None, vertex_lighting::build),
    (
        "KeepWindowOnFocusLoss",
        Some(|s| {
            setting(
                s.options.eix.fixes.keep_window_on_focus_loss,
                "KeepWindowOnFocusLoss",
            )
        }),
        keep_window::build,
    ),
    (
        "SmoothCamera",
        Some(|s| setting(s.options.eix.fixes.smooth_camera, "SmoothCamera")),
        smooth_camera::build,
    ),
    (
        "PlayerListOnJoin",
        Some(|s| setting(s.options.eix.fixes.player_list_on_join, "PlayerListOnJoin")),
        player_list_on_join::build,
    ),
    (
        "SkipDiscCheck",
        Some(|s| {
            setting(s.options.eix.fixes.skip_disc_check, "SkipDiscCheck")
                .or_else(disc_check::not_needed)
        }),
        disc_check::build,
    ),
    (
        "UnitShadowResolution",
        Some(unit_shadow::off),
        unit_shadow::build,
    ),
    ("WindowFocus", Some(window_focus::off), window_focus::build),
    (
        "UnitPanelSpells",
        Some(|s| setting(s.options.eix.fixes.unit_panel_spells, "UnitPanelSpells")),
        unit_panel_spells::build,
    ),
    (
        "SpellsNeedAcceptedSkills",
        Some(|s| {
            setting(
                s.options.eix.fixes.spells_need_accepted_skills,
                "SpellsNeedAcceptedSkills",
            )
        }),
        accepted_skills::build,
    ),
];

/// A mod's own settings that share the fixes' machinery: the mod chose them, so `[EngineFixes] Enabled` = 0 leaves
/// them on.
const MOD_SETTINGS: &[(&str, Option<Switch>, Build)] = &[(
    "ShowTutorialDefault",
    Some(show_tutorial_default::off),
    show_tutorial_default::build,
)];

/// The fixes, unless `[EngineFixes] Enabled` is 0, then the mod settings, in the order they apply.
fn selected(
    engine_fixes: u32,
) -> impl Iterator<Item = &'static (&'static str, Option<Switch>, Build)> {
    let fixes: &[_] = if engine_fixes == 0 { &[] } else { FIXES };
    fixes.iter().chain(MOD_SETTINGS)
}

/// Applies every fix whose switch is not 0, unless `[EngineFixes] Enabled` is 0, and the mod settings the mod sets.
pub(crate) fn run(token: &BeforeWinMain, setup: &Setup) {
    if setup.options.eix.fixes.engine_fixes == 0 {
        log::info("fixes", "[EngineFixes] Enabled is 0: no fix applied", "");
    }
    let mut tally = Tally::default();
    for &(name, switch, build) in selected(setup.options.eix.fixes.engine_fixes) {
        if let Some(reason) = switch.and_then(|off| off(setup)) {
            log::info("fixes", &format!("off: {reason}"), name);
            continue;
        }
        let built = eix_engine::owners::building(name, || build(setup));
        apply_built(token, "fixes", name, built, &mut tally);
    }
    log::info(
        "fixes",
        &format!("{} applied, {} failed", tally.applied, tally.failed),
        "",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::groups::Labelled;

    fn names(engine_fixes: u32) -> Vec<&'static str> {
        selected(engine_fixes).map(|&(name, _, _)| name).collect()
    }

    #[test]
    fn engine_fixes_0_leaves_only_the_mod_settings() {
        assert_eq!(names(0), ["ShowTutorialDefault"]);
    }

    #[test]
    fn the_mod_settings_apply_after_every_fix() {
        let all = names(1);
        assert_eq!(all.len(), FIXES.len() + MOD_SETTINGS.len());
        assert_eq!(all.last(), Some(&"ShowTutorialDefault"));
        assert_eq!(all.first(), Some(&"HeapPadding"));
    }

    /// Every write of every fix and mod setting, in the order they apply.
    fn every_write() -> Vec<Labelled> {
        let mut setup = Setup::default();
        // The one entry that builds only when its mod key is set.
        setup.options.config.mod_.show_tutorial_default = Some(0);
        // The one that builds only for a side the game does not have.
        setup.options.eix.fixes.unit_shadow_resolution = 128;
        let mut built = 0;
        let writes = selected(1)
            .flat_map(|&(name, _, build)| {
                built += 1;
                build(&setup).unwrap_or_else(|e| panic!("{name}: {e:?}"))
            })
            .collect();
        assert_eq!(built, FIXES.len() + MOD_SETTINGS.len());
        writes
    }

    #[test]
    fn no_fix_writes_a_site_spelladdon_patches() {
        let spelladdon: Vec<(String, std::ops::Range<usize>)> = spelladdon::writes()
            .into_iter()
            .map(|(name, w)| (name, w.site()..w.site() + w.size()))
            .collect();
        assert!(
            spelladdon.len() > 100,
            "{} SpellAddon sites",
            spelladdon.len()
        );
        for w in every_write() {
            let ours = w.write.site()..w.write.site() + w.write.size();
            for (theirs, at) in &spelladdon {
                assert!(
                    ours.end <= at.start || at.end <= ours.start,
                    "{} at {:#X}..{:#X} overlaps SpellAddon's {theirs} at {:#X}..{:#X}",
                    w.label,
                    ours.start,
                    ours.end,
                    at.start,
                    at.end
                );
            }
        }
    }

    /// game.exe as the loader maps it: its base address and image, the headers and every section at its address.
    fn mapped(file: &[u8]) -> (usize, Vec<u8>) {
        let u16_at = |at: usize| usize::from(u16::from_le_bytes([file[at], file[at + 1]]));
        let u32_at = |at: usize| u32::from_le_bytes(file[at..at + 4].try_into().unwrap()) as usize;
        let pe = u32_at(0x3C);
        let optional = pe + 24;
        let (base, size, headers) = (
            u32_at(optional + 28),
            u32_at(optional + 56),
            u32_at(optional + 60),
        );
        let mut image = vec![0; size];
        image[..headers].copy_from_slice(&file[..headers]);
        for i in 0..u16_at(pe + 6) {
            let section = optional + u16_at(pe + 20) + 40 * i;
            let (at, raw_size, raw) = (
                u32_at(section + 12),
                u32_at(section + 16),
                u32_at(section + 20),
            );
            let n = raw_size.min(size - at);
            image[at..at + n].copy_from_slice(&file[raw..raw + n]);
        }
        (base, image)
    }

    /// The import address table's range: its slots hold the loaded functions' addresses, not the file's bytes.
    fn import_slots(base: usize, image: &[u8]) -> std::ops::Range<usize> {
        let u32_at = |at: usize| u32::from_le_bytes(image[at..at + 4].try_into().unwrap()) as usize;
        let directory = u32_at(0x3C) + 24 + 96 + 12 * 8;
        let at = base + u32_at(directory);
        at..at + u32_at(directory + 4)
    }

    /// The patch groups read game memory while they build, so a test process, which holds no game.exe, cannot build
    /// them; the fixes and SpellAddon's features build from their own tables.
    #[test]
    #[ignore = "needs the game's game.exe: EIX_GAME_DIR=<the game folder, as a Windows path>"]
    fn every_expected_byte_is_game_exes() {
        let dir = std::env::var("EIX_GAME_DIR").expect("EIX_GAME_DIR names the game folder");
        let file = std::fs::read(format!("{dir}\\game.exe")).expect("game.exe reads");
        let (base, game) = mapped(&file);
        let slots = import_slots(base, &game);
        let at = |site: usize, len: usize| site - base..site - base + len;
        let (mut checked, mut skipped, mut failures) = (0, 0, Vec::new());
        let mut fixed = game.clone();
        for w in every_write() {
            let (site, expected) = (w.write.site(), w.write.expected());
            if slots.contains(&site) {
                skipped += 1;
                continue;
            }
            checked += 1;
            let found = &fixed[at(site, expected.len())];
            if found == expected {
                fixed[at(site, w.write.size())].copy_from_slice(w.write.new_bytes());
            } else if found != w.write.new_bytes() {
                // Bytes the exe already holds need no write (SkipDiscCheck on GOG's exe); any others are wrong.
                failures.push(format!(
                    "{} at {site:#X}: expected {expected:02X?}, found {found:02X?}",
                    w.label
                ));
            }
        }
        // SpellAddon applies after the fixes: a feature finds the game's bytes or a fix's.
        for (name, w) in spelladdon::writes() {
            let range = at(w.site(), w.size());
            checked += 1;
            if game[range.clone()] != *w.expected() && fixed[range.clone()] != *w.expected() {
                failures.push(format!(
                    "SpellAddon {name} at {:#X}: expected {:02X?}, found {:02X?}",
                    w.site(),
                    w.expected(),
                    &game[range]
                ));
            }
        }
        eprintln!("{checked} writes checked, {skipped} import-slot writes skipped");
        assert!(
            failures.is_empty(),
            "{} of {checked} writes:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
