//! Which of addon.dll's 87 patch groups switch on, from the options: one predicate per group.
//!
//! The groups run in five tables; [`Group::decide`] first applies the table's gate, then the group's own predicate.
//! A disabled group carries the reason for the log.

use crate::options::{CommandLine, Options};
use crate::widescreen::Mode;

/// The outcome for one group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// The group is applied.
    Enabled,
    /// The group is skipped, with the reason for the log.
    Disabled(String),
}

impl Decision {
    /// Whether the group is applied.
    pub fn is_enabled(&self) -> bool {
        matches!(self, Decision::Enabled)
    }
}

/// The five tables the groups run in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Table {
    /// Always runs.
    T1,
    /// Runs when game.exe holds the expected bytes.
    T2,
    /// Runs when the `*.asi` scan passes.
    T3,
    /// Runs when a mod is loaded.
    T4,
    /// Runs when config.reg names an ai.reg.
    T5,
}

impl Table {
    /// The gate of the table, as a reason when it is closed.
    fn gate(self, o: &Options) -> Decision {
        match self {
            Table::T1 => Decision::Enabled,
            Table::T2 => when(
                o.env.exe_enables_t2,
                "table T2 is off: game.exe lacks the expected bytes",
            ),
            Table::T3 => when(
                o.env.asi_scan_passes,
                "table T3 is off: the *.asi scan failed",
            ),
            Table::T4 => when(o.env.mod_loaded, "table T4 is off: no mod is loaded"),
            Table::T5 => when(
                o.env.mod_loaded && o.ai_reg_named(),
                "table T5 is off: config.reg [CONFIG] names no ai.reg",
            ),
        }
    }
}

fn when(cond: bool, reason: &str) -> Decision {
    if cond {
        Decision::Enabled
    } else {
        Decision::Disabled(reason.to_owned())
    }
}

fn nonzero(v: i32, key: &str) -> Decision {
    when(v != 0, &format!("{key} is 0"))
}

fn non_empty(v: &[u8], key: &str) -> Decision {
    when(!v.is_empty(), &format!("{key} is not set"))
}

fn always(_: &Options) -> Decision {
    Decision::Enabled
}

macro_rules! groups {
    ($($variant:ident $table:ident $rule:path;)*) => {
        /// One of the addon's patch groups; the variant names are the names in the addon's log.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Group {
            $(
                #[doc = concat!("The ", stringify!($variant), " group.")]
                $variant,
            )*
        }

        impl Group {
            /// Every group in table order.
            pub const ALL: &'static [Group] = &[$(Group::$variant),*];

            /// The group's name in the addon's log.
            pub fn name(self) -> &'static str {
                match self { $(Group::$variant => stringify!($variant),)* }
            }

            /// The table the group runs in.
            pub fn table(self) -> Table {
                match self { $(Group::$variant => Table::$table,)* }
            }

            /// Whether the group is applied under these options.
            pub fn decide(self, o: &Options) -> Decision {
                let gate = self.table().gate(o);
                if !gate.is_enabled() {
                    return gate;
                }
                match self { $(Group::$variant => $rule(o),)* }
            }
        }
    };
}

fn game_exe_debug(o: &Options) -> Decision {
    non_empty(&o.config.dev.game_debug_file, "[DEV] GameDebugFile")
}

/// BinkPatch: the original reports a missing game binkw32 or `_BinkOpen@8` as a failure, before it
/// looks for a mod; those cases are enabled here so the group's preparation reports them.
fn bink_patch(o: &Options) -> Decision {
    let e = &o.env;
    if !e.game_bink_loaded || !e.bink_open_exported {
        return Decision::Enabled;
    }
    if !e.mod_loaded {
        return Decision::Disabled("no mod is loaded".to_owned());
    }
    when(
        e.bink_dll_in_mod,
        "the mod folder has no binkw32.dll that loads as a module of its own",
    )
}

fn wide_screen(o: &Options) -> Decision {
    if o.settings.wide_screen == 0 {
        return Decision::Disabled("WideScreen is 0".to_owned());
    }
    // An invalid value goes on to the group, which fails on it as the original's does.
    match Mode::from_resolution(o.settings.video_resolution.as_deref()) {
        Mode::NotWide(reason) => Decision::Disabled(reason),
        Mode::Invalid(_) | Mode::Wide(_) => Decision::Enabled,
    }
}

fn objects_draw_radius(o: &Options) -> Decision {
    when(
        o.settings.objects_draw_radius != 100,
        "ObjectsDrawRadius is 100",
    )
}

fn landscape_draw_radius(o: &Options) -> Decision {
    when(
        o.settings.landscape_draw_radius != 100,
        "LandscapeDrawRadius is 100",
    )
}

fn command_line(o: &Options) -> Decision {
    when(
        o.env.command_line != CommandLine::None,
        "game.exe was not started with the addon argument",
    )
}

fn graph_gen_patches(o: &Options) -> Decision {
    when(
        o.env.command_line == CommandLine::AiGraph,
        "game.exe was not started with addon aigraph",
    )
}

/// AntiCrash: the ai.reg value when table T5 reads it, else the registry value.
fn anti_crash(o: &Options) -> Decision {
    let ai = if o.ai_reg_named() && o.env.mod_loaded {
        o.ai.addon.anti_crash
    } else {
        None
    };
    when(
        ai.unwrap_or(i32::from(o.settings.anti_crash != 0)) != 0,
        "AntiCrash is 0",
    )
}

fn set_default_rate(o: &Options) -> Decision {
    when(o.settings.rate == 2500, "Rate is not 2500")
}

fn auto_exec(o: &Options) -> Decision {
    let from_registry = o.settings.console_autoexec.iter().any(|c| !c.is_empty());
    when(
        from_registry || o.env.autoexec_file_commands > 0,
        "no Console AutoExec commands in the registry or 2utoexec.cfg",
    )
}

fn mq_client(o: &Options) -> Decision {
    when(o.settings.mq_client != 0, "MQ client is 0")
}

fn mp_speed_control(o: &Options) -> Decision {
    when(o.settings.mp_speed_control != 0, "MP Speed-control is 0")
}

fn mp_console(o: &Options) -> Decision {
    when(o.settings.mp_console != 0, "MP console is 0")
}

fn menu_multi_disable(o: &Options) -> Decision {
    when(o.config.mod_.multi == 0, "[MOD] Multi is not 0")
}

fn menu_single_disable(o: &Options) -> Decision {
    when(o.config.mod_.single == 0, "[MOD] Single is not 0")
}

fn saves_path(o: &Options) -> Decision {
    nonzero(o.config.mod_.game_saves, "[MOD] GameSaves")
}

fn mp_zone(o: &Options) -> Decision {
    when(
        o.config.mp_zone.complete(),
        "[MPZone] lacks Gipath, Ingos or Suslanger",
    )
}

fn prevent_zone1_animation(o: &Options) -> Decision {
    nonzero(
        o.config.mod_.prevent_zone1_animation,
        "[MOD] PreventZone1Animation",
    )
}

fn chat_log(o: &Options) -> Decision {
    non_empty(&o.config.dev.chat_html_file, "[DEV] ChatHtmlFile")
}

fn config_music_reg(o: &Options) -> Decision {
    non_empty(&o.config.config.music_reg, "[CONFIG] music.reg")
}

fn movies_patch(o: &Options) -> Decision {
    when(
        o.env.bink_open_exported,
        "the game's binkw32 does not export _BinkOpen@8",
    )
}

fn config_streamsn_reg(o: &Options) -> Decision {
    non_empty(&o.config.config.streamsn_reg, "[CONFIG] streamsn.reg")
}

fn force_show_tutorial(o: &Options) -> Decision {
    nonzero(o.config.mod_.force_show_tutorial, "[MOD] ForceShowTutorial")
}

fn config_movie_ini(o: &Options) -> Decision {
    non_empty(&o.config.config.movie_ini, "[CONFIG] movie.ini")
}

fn config_lights_ini(o: &Options) -> Decision {
    when(
        !o.config.config.lights_ini.is_empty(),
        "[CONFIG] has no lights<X>.ini key",
    )
}

fn config_tutorial_ini(o: &Options) -> Decision {
    non_empty(&o.config.config.tutorial_ini, "[CONFIG] tutorial.ini")
}

fn perk_limit(o: &Options) -> Decision {
    let p = &o.ai.perk;
    when(
        [p.melee, p.archery, p.elemental, p.sense, p.astral]
            .iter()
            .any(|&v| v != 100),
        "[AddonPerk] limits are all 100",
    )
}

fn exp_calc(o: &Options) -> Decision {
    let x = &o.ai.exp;
    when(
        x.charge_method != 0 || x.mul_factor != 1.0,
        "[AddonExp] ChargeMethod is 0 and MulFactor is 1.0",
    )
}

fn mp_show_all_quests(o: &Options) -> Decision {
    nonzero(o.ai.addon.show_all_quests, "[Addon] ShowAllQuests")
}

fn disable_runaway(o: &Options) -> Decision {
    nonzero(o.ai.addon.disable_runaway, "[Addon] DisableRunaway")
}

fn enable_archers_defence(o: &Options) -> Decision {
    nonzero(o.ai.addon.archers_defence, "[Addon] ArchersDefence")
}

fn disable_distance_modifier(o: &Options) -> Decision {
    nonzero(o.ai.addon.distance_modifier, "[Addon] DistanceModifier")
}

fn disable_weight_bonus(o: &Options) -> Decision {
    nonzero(o.ai.addon.weight_bonus, "[Addon] WeightBonus")
}

fn exp_limit(o: &Options) -> Decision {
    when(
        o.ai.exp.limit >= 0.0,
        "[AddonExp] Limit is below 0 or not set",
    )
}

fn random_magic_damage(o: &Options) -> Decision {
    when(
        o.ai.addon.magic_random >= 0.0,
        "[Addon] MagicRandom is below 0 or not set",
    )
}

fn negative_magic_effect(o: &Options) -> Decision {
    nonzero(
        o.ai.addon.allow_magic_negative,
        "[Addon] AllowMagicNegative",
    )
}

fn deagr_player_on_loose(o: &Options) -> Decision {
    nonzero(
        o.ai.addon.deagr_player_on_loose,
        "[Addon] DeagrPlayerOnLoose",
    )
}

fn attack_anim_speed(o: &Options) -> Decision {
    nonzero(o.ai.addon.attack_anim_speed, "[Addon] AttackAnimSpeed")
}

fn new_rune_codes(o: &Options) -> Decision {
    nonzero(o.ai.addon.new_runes, "[Addon] NewRunes")
}

fn use_prototype_actions_for_mob_attack(o: &Options) -> Decision {
    nonzero(
        o.ai.addon.use_prototype_actions_for_mob_attack,
        "[Addon] UsePrototypeActionsForMobAttack",
    )
}

fn use_prototype_actions_for_mob_cast(o: &Options) -> Decision {
    nonzero(
        o.ai.addon.use_prototype_actions_for_mob_cast,
        "[Addon] UsePrototypeActionsForMobCast",
    )
}

fn fast_teleport(o: &Options) -> Decision {
    nonzero(o.ai.addon.fast_teleport, "[Addon] FastTeleport")
}

fn disable_crippled_hl(o: &Options) -> Decision {
    nonzero(o.ai.addon.disable_crippled_hl, "[Addon] DisableCrippledHL")
}

fn unlock_experimental_spells(o: &Options) -> Decision {
    nonzero(
        o.ai.addon.unlock_experimental_spells,
        "[Addon] UnlockExperimentalSpells",
    )
}

fn load_spell_animation_from_slots_field(o: &Options) -> Decision {
    nonzero(
        o.ai.addon.load_spell_animation_from_slots_field,
        "[Addon] LoadSpellAnimationFromSlotsField",
    )
}

fn improve_info_windows(o: &Options) -> Decision {
    nonzero(
        o.ai.addon.improve_info_windows,
        "[Addon] ImproveInfoWindows",
    )
}

fn remove_spell_construct_reqs(o: &Options) -> Decision {
    nonzero(
        o.ai.addon.remove_spell_construct_reqs,
        "[Addon] RemoveSpellConstructReqs",
    )
}

fn send_spell_effect_value_to_players(o: &Options) -> Decision {
    nonzero(
        o.ai.addon.send_spell_effect_value_to_players,
        "[Addon] SendSpellEffectValueToPlayers",
    )
}

fn add_weapons_from_db_to_all_units(o: &Options) -> Decision {
    nonzero(
        o.ai.addon.add_weapons_from_db_to_all_units,
        "[Addon] AddWeaponsFromDbToAllUnits",
    )
}

fn unlock_corpses(o: &Options) -> Decision {
    nonzero(o.ai.addon.unlock_corpses, "[Addon] UnlockCorpses")
}

/// The nine Lua groups share one switch. The script path and the callbacks it must define are checked when the
/// scripts load, not here.
fn lua_scripts(o: &Options) -> Decision {
    nonzero(o.ai.addon.enable_lua_scripts, "[Addon] EnableLuaScripts")
}

groups! {
    GameExeDebug T1 game_exe_debug;
    BinkPatch T1 bink_patch;
    WideScreen T1 wide_screen;
    ObjectsDrawRadius T1 objects_draw_radius;
    LandscapeDrawRadius T1 landscape_draw_radius;
    AllowScriptArgsInLoops T1 always;
    AllowScriptsInMainMenu T1 always;
    FixLeversActivate T1 always;
    FixSpellEffectsOverflow T1 always;
    FixProgressNames T1 always;
    AddNewConsoleCommands T1 always;
    ImproveConsole T1 always;
    CommandLine T1 command_line;
    NewStringEvents T1 always;
    FixSpellSelfCasting T1 always;
    FixProcessLogin T1 always;
    FixMoveObject T1 always;
    GraphGenPatches T1 graph_gen_patches;
    FixAddMapObject T1 always;
    FixPlaingBattleMusicFromAntimagic T1 always;
    FixRepairPrice T1 always;
    FixRemoveMapObject T1 always;
    FixCreateItem T1 anti_crash;
    DisableExitOnCrashReport T1 anti_crash;
    ExtendedServerName T1 always;
    SetDefaultRate T1 set_default_rate;
    NivalRegistry T2 always;
    AutoExec T2 auto_exec;
    MQClient T2 mq_client;
    MPSpeedControl T2 mp_speed_control;
    MPConsole T2 mp_console;
    SetEiPluginMode T3 always;
    PlayerNumbers T3 always;
    AddNewScriptCommands T3 always;
    PersistentGlobalVars T3 always;
    MenuMultiDisable T4 menu_multi_disable;
    MenuSingleDisable T4 menu_single_disable;
    SavesPath T4 saves_path;
    MpZone T4 mp_zone;
    PreventZone1Animation T4 prevent_zone1_animation;
    ChatLog T4 chat_log;
    Maps T4 always;
    ResOverride T4 always;
    ConfigMusicReg T4 config_music_reg;
    MoviesPatch T4 movies_patch;
    CameraPatch T4 always;
    ConfigStreamSNReg T4 config_streamsn_reg;
    StreamPatch T4 always;
    ForceShowTutorial T4 force_show_tutorial;
    ConfigMovieIni T4 config_movie_ini;
    ConfigLightsIni T4 config_lights_ini;
    ConfigTutorialIni T4 config_tutorial_ini;
    ConfigAiReg T5 always;
    PerkLimit T5 perk_limit;
    ExpCalc T5 exp_calc;
    MPShowAllQuests T5 mp_show_all_quests;
    DisableRunaway T5 disable_runaway;
    EnableArchersDefence T5 enable_archers_defence;
    DisableDistanceModifier T5 disable_distance_modifier;
    DisableWeightBonus T5 disable_weight_bonus;
    ExpLimit T5 exp_limit;
    RandomMagicDamage T5 random_magic_damage;
    NegativeMagicEffect T5 negative_magic_effect;
    DeagrPlayerOnLoose T5 deagr_player_on_loose;
    AttackAnimSpeed T5 attack_anim_speed;
    NewRuneCodes T5 new_rune_codes;
    UsePrototypeActionsForMobAttack T5 use_prototype_actions_for_mob_attack;
    UsePrototypeActionsForMobCast T5 use_prototype_actions_for_mob_cast;
    FastTeleport T5 fast_teleport;
    DisableCrippledHL T5 disable_crippled_hl;
    UnlockExperimentalSpells T5 unlock_experimental_spells;
    LoadSpellAnimationFromSlotsField T5 load_spell_animation_from_slots_field;
    ImproveInfoWindows T5 improve_info_windows;
    RemoveSpellConstructReqs T5 remove_spell_construct_reqs;
    SendSpellEffectValueToPlayers T5 send_spell_effect_value_to_players;
    AddWeaponsFromDbToAllUnits T5 add_weapons_from_db_to_all_units;
    MultipleLoadingMobs T5 always;
    UnlockCorpses T5 unlock_corpses;
    LuaGetPerkPrice T5 lua_scripts;
    LuaGetModifiedDamage T5 lua_scripts;
    LuaRecalcUnitStats T5 lua_scripts;
    LuaRegenerateHpAndMp T5 lua_scripts;
    LuaCalcRunningManaCost T5 lua_scripts;
    LuaCalcAttackParams T5 lua_scripts;
    LuaGetExpModifiers T5 lua_scripts;
    LuaRecalcShopsPriceMultipliers T5 lua_scripts;
    LuaLmpExpDistribution T5 lua_scripts;
}

impl Options {
    /// Every group with its decision, in table order.
    pub fn decide_all(&self) -> Vec<(Group, Decision)> {
        Group::ALL.iter().map(|&g| (g, g.decide(self))).collect()
    }
}
