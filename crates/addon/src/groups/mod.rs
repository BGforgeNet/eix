//! The 87 patch groups, in the original's table order (T1 to T5).
//!
//! Each group's enabling predicate is core's [`Group::decide`]; the group's module here prepares its site writes
//! through the engine's hooks, and [`run`] applies them all or nothing and logs each site and the outcome.

use eix_core::groups::{Decision, Group};
use eix_engine::group::{Outcome, SiteWrite};
use eix_engine::hook::{self, Context, HookError, Mode};
use eix_engine::patch::PatchError;
use eix_sites::Site;

use crate::game::BeforeWinMain;
use crate::log;
use crate::setup::Setup;

mod add_new_console_commands;
mod add_new_script_commands;
mod allow_script_args_in_loops;
mod allow_scripts_in_main_menu;
mod arg_path;
mod attack_anim_speed;
mod auto_exec;
mod bink_patch;
mod camera_patch;
mod chat_log;
mod command_line;
mod config_ai_reg;
mod config_lights_ini;
mod config_movie_ini;
mod config_music_reg;
mod config_stream_sn_reg;
mod config_tutorial_ini;
mod deagr_player_on_loose;
mod disable_exit_on_crash_report;
mod draw_radius;
mod exp_calc;
mod exp_limit;
mod extended_server_name;
mod fix_add_map_object;
mod fix_battle_music;
mod fix_create_item;
mod fix_levers_activate;
mod fix_move_object;
mod fix_process_login;
mod fix_progress_names;
mod fix_remove_map_object;
mod fix_repair_price;
mod fix_spell_effects_overflow;
mod fix_spell_self_casting;
mod force_show_tutorial;
mod game_exe_debug;
mod game_text;
mod graph_gen_patches;
mod improve_console;
mod improve_info_windows;
mod load_spell_animation;
mod lua_calc_attack_params;
mod lua_calc_running_mana_cost;
mod lua_common;
mod lua_get_exp_modifiers;
mod lua_get_modified_damage;
mod lua_get_perk_price;
mod lua_lmp_exp_distribution;
mod lua_recalc_shops_price_multipliers;
mod lua_recalc_unit_stats;
mod lua_regenerate_hp_and_mp;
mod maps;
mod memory;
mod menu_disable;
mod movies_patch;
mod mp_switches;
mod mp_zone;
mod multiple_loading_mobs;
mod negative_magic_effect;
mod new_rune_codes;
mod new_string_events;
mod nival_registry;
mod perk_limit;
mod persistent_global_vars;
mod player_numbers;
mod prevent_zone1_animation;
mod prototype_actions;
mod random_magic_damage;
mod res_override;
mod rule_bytes;
mod saves_path;
mod send_spell_effect_value;
mod set_default_rate;
mod set_ei_plugin_mode;
mod stream_patch;
mod wide_screen;

/// A group's preparation: its labelled site writes, with their stubs placed.
pub(crate) type Build = fn(&Setup) -> Result<Vec<Labelled>, BuildError>;

/// Why a group could not be prepared.
#[derive(Debug)]
pub(crate) struct BuildError(String);

impl From<HookError> for BuildError {
    fn from(e: HookError) -> Self {
        Self(e.to_string())
    }
}

impl From<String> for BuildError {
    fn from(e: String) -> Self {
        Self(e)
    }
}

impl From<PatchError> for BuildError {
    fn from(e: PatchError) -> Self {
        Self(e.to_string())
    }
}

/// A plain byte write at `site`: `original` becomes `new`.
pub(crate) fn bytes(
    label: &str,
    site: Site,
    original: &[u8],
    new: &[u8],
) -> Result<Labelled, BuildError> {
    Ok(Labelled {
        label: label.to_owned(),
        write: SiteWrite::bytes(site, original, new)?,
    })
}

/// `bytes` as a C string that lives as long as the process: hooks hand it to the game, which may keep it.
pub(crate) fn static_c_str(bytes: Vec<u8>) -> Result<&'static std::ffi::CStr, BuildError> {
    let s = std::ffi::CString::new(bytes)
        .map_err(|e| BuildError(format!("a path holds a NUL at byte {}", e.nul_position())))?;
    Ok(Box::leak(s.into_boxed_c_str()))
}

/// A site write and the label it is logged under (`<group> #<n>` or a name).
#[derive(Debug)]
pub(crate) struct Labelled {
    pub(crate) label: String,
    pub(crate) write: SiteWrite,
}

/// A 4-byte pointer at `site` (an operand, a table entry) goes from `expected` to `new`, logged as `label`.
pub(crate) fn pointer(label: &str, site: Site, expected: usize, new: usize) -> Labelled {
    Labelled {
        label: label.to_owned(),
        write: SiteWrite::pointer(site, expected, new),
    }
}

/// Hook handler: the call's first argument (stack word 1, after the return address) becomes `arg`.
extern "C" fn set_first_arg(arg: usize, ctx: &mut Context) {
    if !ctx.set_stack_word(1, arg as u32) {
        log::error("groups", "the first argument is off the stack", "");
    }
}

/// Hook handler: EAX becomes `arg`.
extern "C" fn set_eax(arg: usize, ctx: &mut Context) {
    ctx.eax = arg as u32;
}

/// The `call` at `site` (bytes `original`) gets `path` as its first argument instead of the game's.
pub(crate) fn path_as_first_arg(
    label: &str,
    site: Site,
    original: [u8; 5],
    path: &'static std::ffi::CStr,
) -> Result<Labelled, BuildError> {
    let (write, _) = hook::call_hook(site, original, set_first_arg, path.as_ptr() as usize)?;
    Ok(Labelled {
        label: label.to_owned(),
        write,
    })
}

/// After the instructions at `site` (bytes `original`), EAX holds `path`, which the next instruction pushes.
pub(crate) fn path_in_eax(
    label: &str,
    site: Site,
    original: &[u8],
    path: &'static std::ffi::CStr,
) -> Result<Labelled, BuildError> {
    let write = hook::context_call(
        site,
        original,
        Mode::Before,
        set_eax,
        path.as_ptr() as usize,
    )?;
    Ok(Labelled {
        label: label.to_owned(),
        write,
    })
}

/// The longest `[CONFIG]` path the original takes: it copies the joined path into a 0x104-byte buffer with
/// `strcpy_s`, which ends the process from 260 bytes; ours fails the group there instead.
const MAX_GAME_PATH: usize = 259;

/// The mod folder joined with a `[CONFIG]` value, as a static C string.
pub(crate) fn mod_file(setup: &Setup, value: &[u8]) -> Result<&'static std::ffi::CStr, BuildError> {
    let path = [&setup.mod_dir[..], value].concat();
    if path.len() > MAX_GAME_PATH {
        return Err(BuildError(format!(
            "the path is {} bytes long; the game takes at most {MAX_GAME_PATH}",
            path.len()
        )));
    }
    static_c_str(path)
}

/// Every group in table order, with its preparation.
const REGISTRY: [(Group, Build); 87] = [
    // T1: always runs.
    (Group::GameExeDebug, game_exe_debug::build),
    (Group::BinkPatch, bink_patch::build),
    (Group::WideScreen, wide_screen::build),
    (Group::ObjectsDrawRadius, draw_radius::build_objects),
    (Group::LandscapeDrawRadius, draw_radius::build_landscape),
    (
        Group::AllowScriptArgsInLoops,
        allow_script_args_in_loops::build,
    ),
    (
        Group::AllowScriptsInMainMenu,
        allow_scripts_in_main_menu::build,
    ),
    (Group::FixLeversActivate, fix_levers_activate::build),
    (
        Group::FixSpellEffectsOverflow,
        fix_spell_effects_overflow::build,
    ),
    (Group::FixProgressNames, fix_progress_names::build),
    (
        Group::AddNewConsoleCommands,
        add_new_console_commands::build,
    ),
    (Group::ImproveConsole, improve_console::build),
    (Group::CommandLine, command_line::build),
    (Group::NewStringEvents, new_string_events::build),
    (Group::FixSpellSelfCasting, fix_spell_self_casting::build),
    (Group::FixProcessLogin, fix_process_login::build),
    (Group::FixMoveObject, fix_move_object::build),
    (Group::GraphGenPatches, graph_gen_patches::build),
    (Group::FixAddMapObject, fix_add_map_object::build),
    (
        Group::FixPlaingBattleMusicFromAntimagic,
        fix_battle_music::build,
    ),
    (Group::FixRepairPrice, fix_repair_price::build),
    (Group::FixRemoveMapObject, fix_remove_map_object::build),
    (Group::FixCreateItem, fix_create_item::build),
    (
        Group::DisableExitOnCrashReport,
        disable_exit_on_crash_report::build,
    ),
    (Group::ExtendedServerName, extended_server_name::build),
    (Group::SetDefaultRate, set_default_rate::build),
    // T2: game.exe holds the expected registry root.
    (Group::NivalRegistry, nival_registry::build),
    (Group::AutoExec, auto_exec::build),
    (Group::MQClient, mp_switches::mq_client),
    (Group::MPSpeedControl, mp_switches::mp_speed_control),
    (Group::MPConsole, mp_switches::mp_console),
    // T3: the *.asi scan passes.
    (Group::SetEiPluginMode, set_ei_plugin_mode::build),
    (Group::PlayerNumbers, player_numbers::build),
    (Group::AddNewScriptCommands, add_new_script_commands::build),
    (Group::PersistentGlobalVars, persistent_global_vars::build),
    // T4: a mod is loaded.
    (Group::MenuMultiDisable, menu_disable::multi),
    (Group::MenuSingleDisable, menu_disable::single),
    (Group::SavesPath, saves_path::build),
    (Group::MpZone, mp_zone::build),
    (Group::PreventZone1Animation, prevent_zone1_animation::build),
    (Group::ChatLog, chat_log::build),
    (Group::Maps, maps::build),
    (Group::ResOverride, res_override::build),
    (Group::ConfigMusicReg, config_music_reg::build),
    (Group::MoviesPatch, movies_patch::build),
    (Group::CameraPatch, camera_patch::build),
    (Group::ConfigStreamSNReg, config_stream_sn_reg::build),
    (Group::StreamPatch, stream_patch::build),
    (Group::ForceShowTutorial, force_show_tutorial::build),
    (Group::ConfigMovieIni, config_movie_ini::build),
    (Group::ConfigLightsIni, config_lights_ini::build),
    (Group::ConfigTutorialIni, config_tutorial_ini::build),
    // T5: config.reg names an ai.reg.
    (Group::ConfigAiReg, config_ai_reg::build),
    (Group::PerkLimit, perk_limit::build),
    (Group::ExpCalc, exp_calc::build),
    (Group::MPShowAllQuests, rule_bytes::mp_show_all_quests),
    (Group::DisableRunaway, rule_bytes::disable_runaway),
    (
        Group::EnableArchersDefence,
        rule_bytes::enable_archers_defence,
    ),
    (
        Group::DisableDistanceModifier,
        rule_bytes::disable_distance_modifier,
    ),
    (Group::DisableWeightBonus, rule_bytes::disable_weight_bonus),
    (Group::ExpLimit, exp_limit::build),
    (Group::RandomMagicDamage, random_magic_damage::build),
    (Group::NegativeMagicEffect, negative_magic_effect::build),
    (Group::DeagrPlayerOnLoose, deagr_player_on_loose::build),
    (Group::AttackAnimSpeed, attack_anim_speed::build),
    (Group::NewRuneCodes, new_rune_codes::build),
    (
        Group::UsePrototypeActionsForMobAttack,
        prototype_actions::build_attack,
    ),
    (
        Group::UsePrototypeActionsForMobCast,
        prototype_actions::build_cast,
    ),
    (Group::FastTeleport, rule_bytes::fast_teleport),
    (Group::DisableCrippledHL, rule_bytes::disable_crippled_hl),
    (
        Group::UnlockExperimentalSpells,
        rule_bytes::unlock_experimental_spells,
    ),
    (
        Group::LoadSpellAnimationFromSlotsField,
        load_spell_animation::build,
    ),
    (Group::ImproveInfoWindows, improve_info_windows::build),
    (
        Group::RemoveSpellConstructReqs,
        rule_bytes::remove_spell_construct_reqs,
    ),
    (
        Group::SendSpellEffectValueToPlayers,
        send_spell_effect_value::build,
    ),
    (
        Group::AddWeaponsFromDbToAllUnits,
        rule_bytes::add_weapons_from_db_to_all_units,
    ),
    (Group::MultipleLoadingMobs, multiple_loading_mobs::build),
    (Group::UnlockCorpses, rule_bytes::unlock_corpses),
    (Group::LuaGetPerkPrice, lua_get_perk_price::build),
    (Group::LuaGetModifiedDamage, lua_get_modified_damage::build),
    (Group::LuaRecalcUnitStats, lua_recalc_unit_stats::build),
    (Group::LuaRegenerateHpAndMp, lua_regenerate_hp_and_mp::build),
    (
        Group::LuaCalcRunningManaCost,
        lua_calc_running_mana_cost::build,
    ),
    (Group::LuaCalcAttackParams, lua_calc_attack_params::build),
    (Group::LuaGetExpModifiers, lua_get_exp_modifiers::build),
    (
        Group::LuaRecalcShopsPriceMultipliers,
        lua_recalc_shops_price_multipliers::build,
    ),
    (
        Group::LuaLmpExpDistribution,
        lua_lmp_exp_distribution::build,
    ),
];

/// How applying the groups went, for the summary line.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tally {
    pub(crate) applied: usize,
    pub(crate) disabled: usize,
    pub(crate) failed: usize,
}

/// Logs a site write as `site`, its label, `0x<address>+<length>`.
pub(crate) fn log_site(label: &str, write: &SiteWrite) {
    log::info(
        "site",
        label,
        &format!("0x{:08X}+{}", write.site(), write.size()),
    );
}

thread_local! {
    /// The writes of the groups applied so far, by site, in table order.
    static APPLIED: std::cell::RefCell<Vec<(Site, Vec<u8>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The bytes the last applied group wrote at exactly `site`, if any: a later group that takes over the site (as the
/// original's table order lets it) expects them there.
pub(crate) fn applied_write(site: Site) -> Option<Vec<u8>> {
    APPLIED.with_borrow(|a| {
        a.iter()
            .rev()
            .find(|(s, _)| *s == site)
            .map(|(_, b)| b.clone())
    })
}

/// Applies every group in table order.
pub(crate) fn run(token: &BeforeWinMain, setup: &Setup) -> Tally {
    let mut tally = Tally::default();
    for (group, build) in REGISTRY {
        let name = group.name();
        if let Decision::Disabled(reason) = group.decide(&setup.options) {
            log::info("groups", &format!("disabled: {reason}"), name);
            tally.disabled += 1;
            continue;
        }
        let built = eix_engine::owners::building(name, || build(setup));
        apply_built(token, "groups", name, built, &mut tally);
    }
    tally
}

/// Applies one prepared group or fix (`area` names the kind in addon.log): logs its sites and outcome, counts it in
/// `tally`, and records an applied one's writes for [`applied_write`].
pub(crate) fn apply_built(
    token: &BeforeWinMain,
    area: &str,
    name: &'static str,
    built: Result<Vec<Labelled>, BuildError>,
    tally: &mut Tally,
) {
    let labelled = match built {
        Ok(l) => l,
        Err(BuildError(e)) => {
            log::error(area, &format!("failed: {e}"), name);
            tally.failed += 1;
            return;
        }
    };
    for l in &labelled {
        log_site(&l.label, &l.write);
    }
    let written: Vec<(Site, Vec<u8>)> = labelled
        .iter()
        .map(|l| (l.write.site(), l.write.new_bytes().to_vec()))
        .collect();
    let report = token.apply(name, || Ok(labelled.into_iter().map(|l| l.write).collect()));
    match &report.outcome {
        Outcome::Applied => {
            log::info(area, "applied", name);
            tally.applied += 1;
            APPLIED.with_borrow_mut(|a| a.extend(written));
        }
        Outcome::Disabled => {
            log::info(area, "disabled", name);
            tally.disabled += 1;
        }
        Outcome::Failed { .. } => {
            log::error(area, &report.to_string(), name);
            tally.failed += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_is_in_table_order() {
        let names: Vec<Group> = REGISTRY.iter().map(|(g, _)| *g).collect();
        assert_eq!(names, Group::ALL);
    }
}
