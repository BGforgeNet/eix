//! Every SpellAddon feature: one per INI flag that has a patch of its own, in the original's order (which decides who
//! wins where two features write the same bytes), with its build when implemented.

use eix_core::spelladdon::Values;

use crate::features::{self, Build};

/// One feature: its flag key and its build when implemented.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Feature {
    pub(crate) section: &'static str,
    pub(crate) key: &'static str,
    pub(crate) build: Option<Build>,
}

impl Feature {
    const fn new(section: &'static str, key: &'static str, build: Option<Build>) -> Self {
        Self {
            section,
            key,
            build,
        }
    }
}

/// Why a feature that is switched on is skipped: its original patch faults or corrupts the game's state, or a later
/// feature replaces every change it makes, so it is logged and left out while every other feature applies.
pub(crate) fn skipped(f: &Feature, value: i8, values: &Values) -> Option<&'static str> {
    let flag = |section: &str, key: &str| values.flag(section, key).unwrap_or(0);
    match (f.section, f.key, value) {
        ("gameplay", "rewritenpcvisionarc", 1..)
            if matches!(flag("gameplay", "removenpcsblock"), 1 | 2) =>
        {
            Some("removenpcsblock removes the value it sets")
        }
        ("gameplay", "autorunmobfile", 1..) if matches!(flag("gameplay", "newspells"), 1 | 2) => {
            Some("newspells loads the same mob")
        }
        ("gameplay", "genocideincvars", 1 | 3)
            if matches!(flag("gameplay", "genocidedipvars"), 1 | 3..=5) =>
        {
            Some("genocidedipvars replaces its hook")
        }
        ("gameplay", "fhrunetrans", 1) => Some("the original crashes"),
        ("gameplay", "allvisible", 1) => Some("the original corrupts the options load"),
        ("rebalanceengine", "meleesum_1", 1) => Some("the original crashes at the first attack"),
        ("rebalanceengine", "rewriteherolevelcounting", 2) => {
            Some("the original returns a stale level")
        }
        ("gameplay", "avgarmors", 1) if flag("gameplay", "newperks") < 1 => {
            Some("without newperks the original crashes entering the camp")
        }
        ("rebalanceengine", "meleesum_low", 1)
            if flag("rebalanceengine", "meleesum_1") != 1
                && flag("rebalanceengine", "meleesum_2") != 1 =>
        {
            Some("without meleesum_1 or meleesum_2 the original hangs at the first attack")
        }
        ("gameplay", "nightpower", 1) => {
            Some("the original turns every attack and defence value into garbage")
        }
        ("gameplay", "newbigtables", 1) => {
            Some("the original jumps through code bytes for spell types 39 and up")
        }
        ("gameplay", "randomlessaimedhits", 10) => Some("the original calls code it never wrote"),
        ("gameplay", "armorspell", 1) => Some("the original corrupts the stats refresh"),
        ("gameplay", "vampirism", 1) => Some("the original runs unfinished code"),
        ("gameplay", "aggrmod", 3) => Some("the original corrupts the AI's state"),
        ("gameplay", "unitsrunfromdangers", 1) => Some("the original passes a point as a unit"),
        ("gameplay", "controlmagic", 4 | 6) => Some("the original corrupts the effect's duration"),
        ("gameplay", "sizerebalance", 1..=3) => Some("the original corrupts the unit"),
        ("gameplay", "inspirationintelligence", 2 | 3) => {
            Some("the original crashes on Healing items")
        }
        ("gameplay", "linkremaster", 2 | 3) => Some("the original crashes"),
        ("gameplay", "genocideincvars", 2) => Some("the original reads garbage as objects"),
        ("gameplay", "genocideincvars", 1 | 3) if flag("gameplay", "genocidedipvars") == 2 => {
            Some("with genocidedipvars=2 the original reads garbage as objects")
        }
        ("gameplay", "maxmanatrans", 1) => Some("the original breaks every spell type but one"),
        ("tech", "scriptchecking", 8..) => Some("the original calls code it never wrote"),
        _ => None,
    }
}

/// The features, in the original's order. The array order is the apply order: where two features write the same
/// bytes the later one wins, so it must not be rearranged (`features_sharing_an_address_both_apply` checks the pairs
/// that share an address).
#[rustfmt::skip]
pub(crate) static FEATURES: [Feature; 191] = [
    Feature::new("gameplay", "archersdefence", Some(features::combat::archers_defence)),
    Feature::new("gameplay", "rewritenpcvisionarc", Some(features::ai::npc_vision_arc)),
    Feature::new("gameplay", "shopschanges", Some(features::items::shops_changes)),
    Feature::new("fonts", "changefont", Some(features::world::change_font)),
    Feature::new("gameplay", "batterycharge", Some(features::magic::battery_charge_build)),
    Feature::new("gameplay", "infiniteaddmobspawn", Some(features::script::infinite_add_mob_spawn)),
    Feature::new("gameplay", "splashdamageonair", Some(features::magic::splash_damage_on_air)),
    Feature::new("speed", "allowmpspeed", Some(features::multiplayer::allow_mp_speed)),
    Feature::new("gameplay", "autorunmobfile", Some(features::script::auto_run_mob_file)),
    Feature::new("gameplay", "nonstopmanaregen", Some(features::magic::nonstop_mana_regen)),
    Feature::new("gameplay", "unbreakablebodyparts", Some(features::combat::unbreakable_body_parts)),
    Feature::new("gameplay", "importantalerts", Some(features::startup::important_alerts)),
    Feature::new("gameplay", "newspells", Some(features::magic::new_spells)),
    Feature::new("gameplay", "saveall", Some(features::items::save_all)),
    Feature::new("gameplay", "extendedmerc", Some(features::items::extended_merc)),
    Feature::new("rebalanceengine", "rewriteskillscounting", Some(features::rebalance::skills_counting)),
    Feature::new("rebalanceengine", "rewriterunabilitytime", Some(features::rebalance::run_ability_time)),
    Feature::new("gameplay", "fullexpforall", Some(features::multiplayer::full_exp_for_all)),
    Feature::new("gameplay", "possessioncharmlink", Some(features::magic::possession_charm_link)),
    Feature::new("gameplay", "damageboost", Some(features::combat::damage_boost)),
    Feature::new("skills", "extendedskills", Some(features::items::extended_skills)),
    Feature::new("stealing", "extendedstealing", Some(features::items::extended_stealing)),
    Feature::new("gameplay", "unlockmoney", Some(features::items::unlock_money)),
    Feature::new("gameplay", "newattackdefence", Some(features::combat::new_attack_defence)),
    Feature::new("gameplay", "manygameexe", Some(features::multiplayer::many_game_exe)),
    Feature::new("gameplay", "enablecheatsinmulti", Some(features::multiplayer::cheats_in_multi)),
    Feature::new("gameplay", "visionfix", Some(features::ai::vision_fix)),
    Feature::new("gameplay", "aggrmod", Some(features::ai::aggrmod)),
    Feature::new("gameplay", "enablekillsinmulti", Some(features::combat::kills_in_multi)),
    Feature::new("gameplay", "allvisible", Some(features::ai::all_visible)),
    Feature::new("animationsspeed", "rewriteactionsspeed", Some(features::rebalance::actions_speed)),
    Feature::new("gameplay", "rewriteperssize", Some(features::world::pers_size)),
    Feature::new("rebalanceengine", "newrunsystem", Some(features::rebalance::new_run_system)),
    Feature::new("gameplay", "extendedparams", Some(features::multiplayer::extended_params)),
    Feature::new("gameplay", "longnick", Some(features::multiplayer::long_nick)),
    Feature::new("speed", "rewritespeed", Some(features::multiplayer::rewrite_speed)),
    Feature::new("speed", "rewritespeedkey", Some(features::multiplayer::rewrite_speed_key)),
    Feature::new("speed", "disablepausekey ", Some(features::multiplayer::disable_pause_key)),
    Feature::new("speed", "disablepause", Some(features::multiplayer::disable_pause)),
    Feature::new("speed", "showallquestsinmulti", Some(features::multiplayer::all_quests_in_multi)),
    Feature::new("speed", "spellsaimod", Some(features::magic::spells_ai_mod)),
    Feature::new("gameplay", "infowindowschanges", Some(features::world::info_window_changes)),
    Feature::new("speed", "spellsarealwaysconstructable", Some(features::magic::always_constructable)),
    Feature::new("gameplay", "diffchange", Some(features::rebalance::difficulty_change)),
    Feature::new("speed", "rewriteotherspeed", Some(features::multiplayer::rewrite_other_speed)),
    Feature::new("speed", "gsgetvar", Some(features::script::gs_get_var)),
    Feature::new("speed", "gssetvar", Some(features::script::gs_set_var)),
    Feature::new("gameplay", "versionalerts", Some(features::startup::version_alerts)),
    Feature::new("gameplay", "nogameoveronleadersdeath", Some(features::world::no_game_over_on_leaders_death)),
    Feature::new("gameplay", "lastspellsdamagetype", Some(features::magic::last_spells_damage_type)),
    Feature::new("saves", "savecontrol", Some(features::script::save_control)),
    Feature::new("gameplay", "rangedweaponbackstabs", Some(features::combat::ranged_weapon_backstabs)),
    Feature::new("gameplay", "randomlessaimedhits", Some(features::combat::randomless_aimed_hits)),
    Feature::new("gameplay", "allodsinfo", Some(features::world::allods_info)),
    Feature::new("rebalanceengine", "rewriteherolevelcounting", Some(features::rebalance::level_counting)),
    Feature::new("rebalanceengine", "useonlymindamage", Some(features::combat::only_min_damage)),
    Feature::new("gameplay", "npcsmana", Some(features::magic::npcs_mana)),
    Feature::new("gameplay", "showtype", Some(features::world::show_type)),
    Feature::new("gameplay", "speedboost", Some(features::rebalance::speed_boost)),
    Feature::new("gameplay", "changeweightrangebonuses", Some(features::combat::weight_range_bonuses)),
    Feature::new("gameplay", "spellsprioritys", Some(features::ai::spells_priorities)),
    Feature::new("gameplay", "removenpcsblock", Some(features::ai::remove_npcs_block)),
    Feature::new("gameplay", "changeexpforint", Some(features::rebalance::experience_for_intelligence)),
    Feature::new("gameplay", "copystatscut", Some(features::items::copy_stats_cut)),
    Feature::new("gameplay", "strengthstats", Some(features::combat::strength_stats)),
    Feature::new("gameplay", "expirementalfxes", Some(features::world::experimental_fixes)),
    Feature::new("gameplay", "shopsnewnormalchanges", Some(features::items::shops_new_normal_changes)),
    Feature::new("gameplay", "liftchanges", Some(features::items::lift_changes)),
    Feature::new("gameplay", "liftdebuffs", Some(features::items::lift_debuffs)),
    Feature::new("gameplay", "everyoneishero", Some(features::ai::everyone_is_hero)),
    Feature::new("gameplay", "heroalwayscanrun", Some(features::items::hero_always_can_run)),
    Feature::new("gameplay", "improvedstealing", Some(features::items::improved_stealing)),
    Feature::new("rebalanceengine", "backstabvisualbase", Some(features::rebalance::backstab_visual_base)),
    Feature::new("gameplay", "teleportboost", Some(features::magic::teleport_boost)),
    Feature::new("rebalanceengine", "difficultyreset", Some(features::rebalance::difficulty_reset)),
    Feature::new("rebalanceengine", "meleesum_low", Some(features::combat::melee_sum_low)),
    Feature::new("gameplay", "noruinszoneanimation", Some(features::world::no_ruins_zone_animation)),
    Feature::new("gameplay", "showmanaforeveryone", Some(features::world::show_mana_for_everyone)),
    Feature::new("gameplay", "meterscale", Some(features::world::meter_scale)),
    Feature::new("gameplay", "standtrans", Some(features::script::stand_trans)),
    Feature::new("gameplay", "possessionpatch", Some(features::magic::possession_patch)),
    Feature::new("rebalanceengine", "meleesum_1", Some(features::combat::melee_sum_1)),
    Feature::new("rebalanceengine", "meleesum_2", Some(features::combat::melee_sum_2)),
    Feature::new("gameplay", "waslootedtrans", Some(features::script::was_looted_trans)),
    Feature::new("gameplay", "castscripttrans", Some(features::script::cast_script_trans)),
    Feature::new("gameplay", "armorspell", None),
    Feature::new("gameplay", "newrangedweapontypes", Some(features::combat::new_ranged_weapon_types)),
    Feature::new("gameplay", "inspirationintelligence", Some(features::magic::inspiration_intelligence)),
    Feature::new("gameplay", "lightsboost", Some(features::world::lights_boost)),
    Feature::new("gameplay", "shrinkenlargerebalance", Some(features::magic::shrink_enlarge)),
    Feature::new("gameplay", "hidefireworks", Some(features::magic::hide_fireworks)),
    Feature::new("gameplay", "wallsfrequency", Some(features::magic::walls_frequency)),
    Feature::new("gameplay", "wallspercentage", Some(features::magic::walls_percentage)),
    Feature::new("gameplay", "controlmagic", Some(features::magic::control_magic)),
    Feature::new("gameplay", "newperks", Some(features::rebalance::new_perks)),
    Feature::new("gameplay", "allowheroesarmors", Some(features::combat::heroes_armors)),
    Feature::new("gameplay", "mainmenuweather", Some(features::world::main_menu_weather)),
    Feature::new("gameplay", "visioninweather", Some(features::ai::vision_in_weather)),
    Feature::new("gameplay", "dmgtypes", Some(features::items::dmg_types)),
    Feature::new("gameplay", "nhits", Some(features::rebalance::n_hits)),
    Feature::new("gameplay", "npcsspellcastingtactics", Some(features::ai::spell_casting_tactics)),
    Feature::new("gameplay", "effectredraws", Some(features::magic::effect_redraws)),
    Feature::new("gameplay", "stenchremaster", Some(features::ai::stench_remaster)),
    Feature::new("gameplay", "regenerationremaster", Some(features::magic::regeneration)),
    Feature::new("gameplay", "newplayersafetyconditions", Some(features::ai::player_safety)),
    Feature::new("gameplay", "speedslowremaster", Some(features::magic::speed_slow_remaster)),
    Feature::new("gameplay", "actionspatch", Some(features::rebalance::actions_patch)),
    Feature::new("gameplay", "encumbrancemovespeed", Some(features::items::encumbrance_move_speed)),
    Feature::new("gameplay", "encumbranceattackspeed", Some(features::items::encumbrance_attack_speed)),
    Feature::new("gameplay", "camp", Some(features::items::camp)),
    Feature::new("gameplay", "newoptions", Some(features::items::new_options)),
    Feature::new("gameplay", "notrash", Some(features::world::no_trash)),
    Feature::new("gameplay", "hptrans", Some(features::script::hp_trans)),
    Feature::new("gameplay", "mptrans", Some(features::script::mp_trans)),
    Feature::new("gameplay", "genocideincvars", Some(features::script::genocide_inc_vars)),
    Feature::new("gameplay", "genocidedipvars", Some(features::script::genocide_dip_vars)),
    Feature::new("gameplay", "dontmergegipathzones", Some(features::world::dont_merge_gipat_zones)),
    Feature::new("gameplay", "backstabsforall", Some(features::combat::backstabs_for_all)),
    Feature::new("gameplay", "sizerebalance", Some(features::ai::size_rebalance)),
    Feature::new("gameplay", "nightpowervisionpatch", Some(features::ai::night_power_vision)),
    Feature::new("gameplay", "giveunitspelltrans", Some(features::script::give_unit_spell_trans)),
    Feature::new("gameplay", "musictimings", Some(features::world::music_timings)),
    Feature::new("gameplay", "fearpatch", Some(features::ai::fear_patch)),
    Feature::new("gameplay", "playmusictrans", Some(features::script::play_music_trans)),
    Feature::new("gameplay", "notifydeaths", Some(features::script::notify_deaths)),
    Feature::new("gameplay", "exploredcolors", Some(features::world::explored_colors)),
    Feature::new("gameplay", "disabledragon", Some(features::world::disable_dragon)),
    Feature::new("gameplay", "newcounters", Some(features::world::new_counters)),
    Feature::new("gameplay", "stupidenemies", Some(features::ai::stupid_enemies)),
    Feature::new("gameplay", "increasinghitchance", Some(features::combat::increasing_hit_chance)),
    Feature::new("gameplay", "runawaydistcontrol", Some(features::combat::run_away_distance)),
    Feature::new("gameplay", "otherspellcastingai", Some(features::ai::other_spell_casting_ai)),
    Feature::new("gameplay", "nightpower", Some(features::combat::night_power)),
    Feature::new("gameplay", "vampirism", Some(features::magic::vampirism)),
    Feature::new("gameplay", "infowindowremake", Some(features::world::info_window_remake)),
    Feature::new("gameplay", "avgarmors", Some(features::items::avg_armors)),
    Feature::new("gameplay", "itemslightchanges", Some(features::items::items_light_changes)),
    Feature::new("gameplay", "offensivespells", Some(features::magic::offensive_spells)),
    Feature::new("gameplay", "acidattack", Some(features::combat::acid_attack)),
    Feature::new("gameplay", "castspellpointtrans", Some(features::script::cast_spell_point_trans)),
    Feature::new("gameplay", "castspellunittrans", Some(features::script::cast_spell_unit_trans)),
    Feature::new("gameplay", "legendaryspellsystem", Some(features::script::legendary_spell_system)),
    Feature::new("gameplay", "maxmanatrans", None),
    Feature::new("aicastertable", "rebuildedaicastertable", Some(features::ai::caster_table)),
    Feature::new("gameplay", "newbigtables", Some(features::ai::new_big_tables)),
    Feature::new("aioffendtable", "rebuildedaioffendtable", Some(features::ai::offend_table)),
    Feature::new("gameplay", "linkremaster", Some(features::magic::link_remaster)),
    Feature::new("gameplay", "allowcastsonbriefings", Some(features::magic::casts_on_briefings)),
    Feature::new("gameplay", "drawvisiblepatch", Some(features::world::draw_visible_patch)),
    Feature::new("gameplay", "fhrunetrans", None),
    Feature::new("gameplay", "heroscanhideweapons", Some(features::items::heroes_can_hide_weapons)),
    Feature::new("gameplay", "newdifficultylevels", Some(features::rebalance::new_difficulty_levels)),
    Feature::new("gameplay", "fakesrcrune", Some(features::magic::fake_source_rune)),
    Feature::new("gameplay", "filtersarealwaysusing", Some(features::magic::filters_always)),
    Feature::new("gameplay", "disableexppenalty", Some(features::multiplayer::no_exp_penalty)),
    Feature::new("gameplay", "unitsrunfromdangers", Some(features::ai::units_run_from_dangers)),
    Feature::new("gameplay", "tornadochanges", Some(features::magic::tornado)),
    Feature::new("gameplay", "nightvisionincaves", Some(features::ai::night_vision_in_caves)),
    Feature::new("gameplay", "actionsdexteritymovespeed", Some(features::rebalance::dexterity_move_speed)),
    Feature::new("gameplay", "removefrozenvar", Some(features::script::remove_frozen_var)),
    Feature::new("gameplay", "freemultiplayervars", Some(features::script::free_multiplayer_vars)),
    Feature::new("gameplay", "playingnewloadingscreens", Some(features::world::new_loading_screens)),
    Feature::new("gameplay", "allysseeseachothers", Some(features::ai::allies_see_each_other)),
    Feature::new("gameplay", "changesounds", Some(features::magic::change_sounds)),
    Feature::new("gameplay", "cleveranimationsspeedup", Some(features::rebalance::clever_animations)),
    Feature::new("gameplay", "movetoobjecttrans", Some(features::script::move_to_object_trans)),
    Feature::new("gameplay", "hyperreactions", Some(features::ai::hyper_reactions)),
    Feature::new("gameplay", "groupseetrans", Some(features::script::group_see_trans)),
    Feature::new("gameplay", "unitseetrans", Some(features::script::unit_see_trans)),
    Feature::new("gameplay", "newzonescpecificmusic", Some(features::world::zone_specific_music)),
    Feature::new("gameplay", "notifylootings", Some(features::script::notify_lootings)),
    Feature::new("gameplay", "automaticruncontrol", Some(features::rebalance::automatic_run_control)),
    Feature::new("gameplay", "newallodsacks", Some(features::world::new_allods_acks)),
    Feature::new("gameplay", "segmentsonexitenter", Some(features::world::segments_on_exit_enter)),
    Feature::new("gameplay", "hearingreactionspatch", Some(features::ai::hearing_reactions)),
    Feature::new("gameplay", "strengthweakrebalance", Some(features::magic::strength_weak)),
    Feature::new("gameplay", "getplayertrans", Some(features::script::get_player_trans)),
    Feature::new("gameplay", "runtrans", Some(features::script::run_trans)),
    Feature::new("gameplay", "crawltrans", Some(features::script::crawl_trans)),
    Feature::new("gameplay", "setplayertrans", Some(features::script::set_player_trans)),
    Feature::new("gameplay", "getaiclasstrans", Some(features::script::get_ai_class_trans)),
    Feature::new("speedrun", "timervars", Some(features::multiplayer::timer_vars)),
    Feature::new("gameplay", "bigmapspatch", Some(features::world::big_maps_patch)),
    Feature::new("gameplay", "ultrafixes", Some(features::world::ultra_fixes)),
    Feature::new("tech", "scriptchecking", Some(features::script::script_checking)),
    Feature::new("tech", "drawunitsids", Some(features::script::draw_units_ids)),
    Feature::new("gameplay", "killunittrans", Some(features::script::kill_unit_trans)),
    Feature::new("gameplay", "fixworldtimetrans", Some(features::script::fix_world_time_trans)),
    Feature::new("gameplay", "hiddenzonesexits", Some(features::world::hidden_zones_exits)),
    Feature::new("gameplay", "setspellaggressiontrans", Some(features::script::set_spell_aggression_trans)),
    Feature::new("gameplay", "duels", Some(features::combat::duels)),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::Ctx;
    use crate::features::testing::{no_log, values};
    use std::collections::{BTreeMap, BTreeSet};

    use eix_core::spelladdon::{KEYS, Kind};
    use eix_engine::group::SiteWrite;

    /// The features that write one address differently, as `(earlier, later)`.
    const SHARED: [(&str, &str); 8] = [
        ("rewritenpcvisionarc", "removenpcsblock"),
        ("autorunmobfile", "newspells"),
        ("damageboost", "nightpower"),
        ("genocideincvars", "genocidedipvars"),
        ("newattackdefence", "increasinghitchance"),
        ("rewriteactionsspeed", "encumbranceattackspeed"),
        ("rangedweaponbackstabs", "newrangedweapontypes"),
        ("actionspatch", "encumbranceattackspeed"),
    ];

    /// Each address a build writes, with the byte it expects there and the byte it writes.
    fn bytes_of(writes: &[SiteWrite]) -> BTreeMap<usize, (u8, u8)> {
        writes
            .iter()
            .flat_map(|w| {
                let at = w.site().ru();
                w.expected()
                    .iter()
                    .zip(w.new_bytes())
                    .enumerate()
                    .map(move |(i, (&was, &now))| (at + i, (was, now)))
            })
            .collect()
    }

    /// Where two features write one address differently, both on keep both: the registry skips the earlier, whose
    /// every change the later replaces, or each byte the later writes there expects the earlier's, so the later
    /// builds on it. Every value from 1 to 20 is built (no feature tells apart values past 16).
    #[test]
    fn features_sharing_an_address_both_apply() {
        let defaults = values(&[]);
        let ctx = Ctx::new(&defaults, no_log);
        let mut built = Vec::new();
        for (i, f) in FEATURES.iter().enumerate() {
            let Some(build) = f.build else { continue };
            for value in 1..=20 {
                if skipped(f, value, &defaults).is_some() {
                    continue;
                }
                if let Ok(writes) = build(&ctx, value)
                    && !writes.is_empty()
                {
                    built.push((i, f, value, bytes_of(&writes)));
                }
            }
        }
        let mut found = BTreeSet::new();
        for (i, a, av, ab) in &built {
            for (j, b, bv, bb) in &built {
                let shared = bb
                    .iter()
                    .any(|(at, &(_, now))| ab.get(at).is_some_and(|&(_, was)| was != now));
                if j <= i || !shared {
                    continue;
                }
                found.insert((a.key, b.key));
                let (a_text, b_text) = (av.to_string(), bv.to_string());
                let both = values(&[
                    (a.section, a.key, a_text.as_str()),
                    (b.section, b.key, b_text.as_str()),
                ]);
                if skipped(a, *av, &both).is_some() {
                    continue;
                }
                let ctx = Ctx::new(&both, no_log);
                let first = bytes_of(&a.build.unwrap()(&ctx, *av).unwrap());
                let second = bytes_of(&b.build.unwrap()(&ctx, *bv).unwrap());
                for (at, &(was, now)) in &second {
                    if let Some(&(_, left)) = first.get(at)
                        && left != now
                    {
                        assert_eq!(
                            was, left,
                            "{}={av} then {}={bv}: {at:#x} expects {was:02X}, not the earlier's {left:02X}",
                            a.key, b.key
                        );
                    }
                }
            }
        }
        assert_eq!(found, SHARED.into_iter().collect());
    }

    #[test]
    fn features_are_flags() {
        for f in &FEATURES {
            let key = KEYS
                .iter()
                .find(|k| k.section == f.section && k.name == f.key);
            assert!(
                matches!(key.map(|k| k.kind), Some(Kind::Flag { .. })),
                "[{}] {:?} is not a flag",
                f.section,
                f.key
            );
        }
    }

    #[test]
    fn skips_that_depend_on_other_keys() {
        use crate::features::testing::values;
        let find = |key: &str| FEATURES.iter().find(|f| f.key == key).unwrap();
        let alone = values(&[]);
        assert!(skipped(find("avgarmors"), 1, &alone).is_some());
        let perks = values(&[("gameplay", "newperks", "1")]);
        assert!(skipped(find("avgarmors"), 1, &perks).is_none());
        assert!(skipped(find("genocideincvars"), 2, &alone).is_some());
        assert!(skipped(find("genocideincvars"), 1, &alone).is_none());
        let dip = values(&[("gameplay", "genocidedipvars", "2")]);
        assert!(skipped(find("genocideincvars"), 3, &dip).is_some());
        assert!(skipped(find("genocidedipvars"), 2, &dip).is_none());
        assert!(skipped(find("maxmanatrans"), 1, &alone).is_some());
        assert!(skipped(find("randomlessaimedhits"), 10, &alone).is_some());
        assert!(skipped(find("nightpower"), 1, &alone).is_some());
        assert!(skipped(find("nightpower"), 2, &alone).is_none());
        assert!(skipped(find("randomlessaimedhits"), 9, &alone).is_none());
        assert!(skipped(find("scriptchecking"), 7, &alone).is_none());
        assert!(skipped(find("scriptchecking"), 8, &alone).is_some());
        assert!(skipped(find("newbigtables"), 1, &alone).is_some());
    }
}
