//! The options the addon reads: the mod's `config.reg`, its `ai.reg`, the starter's registry values, and the facts
//! about the game folder that the patch groups also depend on.
//!
//! Defaults are the ones addon.dll 0.12.4.2 uses when a key is missing. A key of the
//! wrong type counts as missing, as in the addon.

use std::collections::BTreeMap;

use crate::reg::Registry;

/// Raw bytes of a string value; the format does not name a code page.
pub type Bytes = Vec<u8>;

/// `[MOD]` of config.reg. `Title` and `Author` are left out: nothing in the addon uses them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModSection {
    /// `Multi`; 0 disables the multiplayer menu entry. Default 1.
    pub multi: i32,
    /// `Single`; 0 disables the single-player menu entry. Default 1.
    pub single: i32,
    /// `GameSaves`; non-zero moves the saves into the mod. Default 1: SavesPath applies without the
    /// key.
    pub game_saves: i32,
    /// `PreventZone1Animation`. Default 0.
    pub prevent_zone1_animation: i32,
    /// `ForceShowTutorial`. Default 0.
    pub force_show_tutorial: i32,
    /// `ShowTutorialDefault`, eix's own: the "show tutorial" option's value while the game has none stored; `None`
    /// keeps the game's own default (on).
    pub show_tutorial_default: Option<i32>,
    /// `AddonVersion`: the addon version the mod needs, as text ([`crate::ei_plugin::parse_version`]); empty
    /// when not set.
    pub addon_version: Bytes,
}

impl Default for ModSection {
    fn default() -> Self {
        Self {
            multi: 1,
            single: 1,
            game_saves: 1,
            prevent_zone1_animation: 0,
            force_show_tutorial: 0,
            show_tutorial_default: None,
            addon_version: Bytes::new(),
        }
    }
}

/// `[CONFIG]` of config.reg: paths relative to the mod folder; empty means not set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigSection {
    /// `movie.ini`.
    pub movie_ini: Bytes,
    /// `tutorial.ini`.
    pub tutorial_ini: Bytes,
    /// `ai.reg`: names the mod's ai.reg and switches the T5 table on.
    pub ai_reg: Bytes,
    /// `music.reg`.
    pub music_reg: Bytes,
    /// `streamsn.reg`.
    pub streamsn_reg: Bytes,
    /// Every `lights<X>.ini` entry (key name with prefix `lights` and suffix `.ini`, 11 characters or more), as key
    /// name as written and string value (empty for a value of another type), in file order.
    pub lights_ini: Vec<(Bytes, Bytes)>,
}

/// `[DEV]` of config.reg.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DevSection {
    /// `GameDebugFile`.
    pub game_debug_file: Bytes,
    /// `ChatHtmlFile`.
    pub chat_html_file: Bytes,
    /// `Plugin`: a plugin DLL, a pattern of them, or a folder of `*.plg`, relative to the mod folder.
    pub plugin: Bytes,
}

/// `[MPZone]` of config.reg; each key holds a string or a string array, `None` when the key is missing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MpZone {
    /// `Gipath`.
    pub gipath: Option<Vec<Bytes>>,
    /// `Ingos`.
    pub ingos: Option<Vec<Bytes>>,
    /// `Suslanger`.
    pub suslanger: Option<Vec<Bytes>>,
}

impl MpZone {
    /// Whether all three keys are present.
    pub fn complete(&self) -> bool {
        self.gipath.is_some() && self.ingos.is_some() && self.suslanger.is_some()
    }

    /// A zone name that appears twice, when all three keys are present: the original stops the game with "Duplicate
    /// zone name found in file". It compares ignoring ASCII case (`_stricmp`), within each key, then
    /// across each pair of keys.
    pub fn duplicate_name(&self) -> Option<&[u8]> {
        let (Some(g), Some(i), Some(s)) = (&self.gipath, &self.ingos, &self.suslanger) else {
            return None;
        };
        let same = |a: &Bytes, b: &Bytes| a.eq_ignore_ascii_case(b);
        for l in [g, i, s] {
            if let Some(n) = (0..l.len()).find(|&n| l[n + 1..].iter().any(|b| same(&l[n], b))) {
                return Some(&l[n]);
            }
        }
        for (a, b) in [(i, g), (s, g), (s, i)] {
            if let Some(x) = a.iter().find(|x| b.iter().any(|y| same(x, y))) {
                return Some(x);
            }
        }
        None
    }
}

/// What the addon reads from the mod's config.reg.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigReg {
    /// `[MOD]`.
    pub mod_: ModSection,
    /// `[RES]`: the archive entries in file order, each a name and its paths (a string value is one path, a string
    /// array several; a path starting with `-` makes the game's own archive ignored).
    pub res: Vec<(Bytes, Vec<Bytes>)>,
    /// `[CONFIG]`.
    pub config: ConfigSection,
    /// `[DEV]`.
    pub dev: DevSection,
    /// `[MPZone]`.
    pub mp_zone: MpZone,
}

/// `[Addon]` of ai.reg.
#[derive(Clone, Debug, PartialEq)]
pub struct AddonSection {
    /// `ShowAllQuests`.
    pub show_all_quests: i32,
    /// `DisableRunaway`.
    pub disable_runaway: i32,
    /// `ArchersDefence`.
    pub archers_defence: i32,
    /// `DistanceModifier`.
    pub distance_modifier: i32,
    /// `WeightBonus`.
    pub weight_bonus: i32,
    /// `MagicRandom`; a value of 0 or more enables RandomMagicDamage. Default -1.
    pub magic_random: f32,
    /// `AllowMagicNegative`.
    pub allow_magic_negative: i32,
    /// `DeagrPlayerOnLoose`.
    pub deagr_player_on_loose: i32,
    /// `AttackAnimSpeed`.
    pub attack_anim_speed: i32,
    /// `NewRunes`.
    pub new_runes: i32,
    /// `UsePrototypeActionsForMobAttack`.
    pub use_prototype_actions_for_mob_attack: i32,
    /// `UsePrototypeActionsForMobCast`.
    pub use_prototype_actions_for_mob_cast: i32,
    /// `FastTeleport`.
    pub fast_teleport: i32,
    /// `DisableCrippledHL`.
    pub disable_crippled_hl: i32,
    /// `UnlockExperimentalSpells`.
    pub unlock_experimental_spells: i32,
    /// `LoadSpellAnimationFromSlotsField`.
    pub load_spell_animation_from_slots_field: i32,
    /// `SendSpellEffectValueToPlayers`.
    pub send_spell_effect_value_to_players: i32,
    /// `AntiCrash`; `None` takes the registry value.
    pub anti_crash: Option<i32>,
    /// `LogErrorsInFile`; read at run time.
    pub log_errors_in_file: i32,
    /// `LogErrorsInConsole`; read at run time.
    pub log_errors_in_console: i32,
    /// `LogErrorsInChat`; read at run time.
    pub log_errors_in_chat: i32,
    /// `EnableShowingScriptErrors`; read at run time.
    pub enable_showing_script_errors: i32,
    /// `ImproveInfoWindows`.
    pub improve_info_windows: i32,
    /// `RemoveSpellConstructReqs`.
    pub remove_spell_construct_reqs: i32,
    /// `AddWeaponsFromDbToAllUnits`.
    pub add_weapons_from_db_to_all_units: i32,
    /// `UnlockCorpses`.
    pub unlock_corpses: i32,
    /// `LuaMainScriptPath`, relative to the mod folder. Default `main.lua`.
    pub lua_main_script_path: Bytes,
    /// `EnableLuaScripts`.
    pub enable_lua_scripts: i32,
}

impl Default for AddonSection {
    fn default() -> Self {
        Self {
            show_all_quests: 0,
            disable_runaway: 0,
            archers_defence: 0,
            distance_modifier: 0,
            weight_bonus: 0,
            magic_random: -1.0,
            allow_magic_negative: 0,
            deagr_player_on_loose: 0,
            attack_anim_speed: 0,
            new_runes: 0,
            use_prototype_actions_for_mob_attack: 0,
            use_prototype_actions_for_mob_cast: 0,
            fast_teleport: 0,
            disable_crippled_hl: 0,
            unlock_experimental_spells: 0,
            load_spell_animation_from_slots_field: 0,
            send_spell_effect_value_to_players: 0,
            anti_crash: None,
            log_errors_in_file: 0,
            log_errors_in_console: 0,
            log_errors_in_chat: 0,
            enable_showing_script_errors: 0,
            improve_info_windows: 0,
            remove_spell_construct_reqs: 0,
            add_weapons_from_db_to_all_units: 0,
            unlock_corpses: 0,
            lua_main_script_path: b"main.lua".to_vec(),
            enable_lua_scripts: 0,
        }
    }
}

/// `[AddonExp]` of ai.reg.
#[derive(Clone, Debug, PartialEq)]
pub struct AddonExp {
    /// `ChargeMethod`; non-zero enables ExpCalc. Default 0.
    pub charge_method: i32,
    /// `MulFactor`; other than 1.0 enables ExpCalc. Default 1.0.
    pub mul_factor: f32,
    /// `Limit`; 0 or more enables ExpLimit. Default -1.
    pub limit: f32,
}

impl Default for AddonExp {
    fn default() -> Self {
        Self {
            charge_method: 0,
            mul_factor: 1.0,
            limit: -1.0,
        }
    }
}

/// `[AddonPerk]` of ai.reg: the perk limits; 100 is the unchanged value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddonPerk {
    /// `Melee`.
    pub melee: i32,
    /// `Archery`.
    pub archery: i32,
    /// `Elemental`.
    pub elemental: i32,
    /// `Sense`.
    pub sense: i32,
    /// `Astral`.
    pub astral: i32,
}

impl Default for AddonPerk {
    fn default() -> Self {
        Self {
            melee: 100,
            archery: 100,
            elemental: 100,
            sense: 100,
            astral: 100,
        }
    }
}

/// What the addon reads from the mod's ai.reg. `[Addon] UsePerkLimit` and `[DifficultyLevels]` are left out:
/// nothing in the addon uses them, and the game reads its difficulty arrays itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AiReg {
    /// `[Addon]`.
    pub addon: AddonSection,
    /// `[AddonExp]`.
    pub exp: AddonExp,
    /// `[AddonPerk]`.
    pub perk: AddonPerk,
}

/// A value under `HKCU\Software\Gipat.Ru\EI_Starter\Settings`, as the caller read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingValue {
    /// `REG_DWORD`.
    Dword(u32),
    /// `REG_SZ`.
    Text(String),
    /// `REG_MULTI_SZ`.
    MultiText(Vec<String>),
}

/// The registry values the addon reads, by value name (looked up ignoring case, as the registry does). The caller
/// also adds `VideoResolution` from `EI_Starter\EvilIslands\General Settings` under that name.
pub type SettingValues = BTreeMap<String, SettingValue>;

/// The DWORD values [`Settings::from_values`] reads, which the caller fetches from the registry.
pub const SETTING_DWORDS: [&str; 17] = [
    "WideScreen",
    "ObjectsDrawRadius",
    "LandscapeDrawRadius",
    "AntiCrash",
    "AutoRecover",
    "Rate",
    "MQ client",
    "MP Speed-control",
    "MP console",
    "EngineFixes",
    "KeepWindowOnFocusLoss",
    "SmoothCamera",
    "PlayerListOnJoin",
    "SkipDiscCheck",
    "UnitShadowResolution",
    "KeepRunningOnFocusLoss",
    "ClipCursorToWindow",
];

/// The registry options with their defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// `WideScreen`. Default 1.
    pub wide_screen: u32,
    /// `ObjectsDrawRadius`; 100 is unchanged. Default 100.
    pub objects_draw_radius: u32,
    /// `LandscapeDrawRadius`; 100 is unchanged. Default 100.
    pub landscape_draw_radius: u32,
    /// `AntiCrash`. Default 1.
    pub anti_crash: u32,
    /// `AutoRecover`: the crash handler backs up the hosting player's party first. Default 1.
    pub auto_recover: u32,
    /// `Rate`; 2500 enables SetDefaultRate. Default 5000.
    pub rate: u32,
    /// `Console AutoExec`, a `REG_MULTI_SZ`; a value of another type is ignored.
    pub console_autoexec: Vec<String>,
    /// `MQ client`. Default 0.
    pub mq_client: u32,
    /// `MP Speed-control`. Default 0.
    pub mp_speed_control: u32,
    /// `MP console`. Default 0.
    pub mp_console: u32,
    /// `EngineFixes`, eix's own: 0 leaves out the fixes beyond the originals.
    /// Default 1.
    pub engine_fixes: u32,
    /// `KeepWindowOnFocusLoss`, eix's own: 0 lets a windowed game minimise when it loses the focus, as the game does.
    /// Default 1.
    pub keep_window_on_focus_loss: u32,
    /// `SmoothCamera`, eix's own: 0 moves the camera in whole game ticks, as the game does. Default 1.
    pub smooth_camera: u32,
    /// `PlayerListOnJoin`, eix's own: 0 leaves a joining client's player list empty until the next change of players,
    /// as the game does. Default 1.
    pub player_list_on_join: u32,
    /// `SkipDiscCheck`, eix's own: 0 lets the starter's game.exe ask for the game disc, as it does. Default 1.
    pub skip_disc_check: u32,
    /// `UnitShadowResolution`, eix's own: the side in pixels of a unit's shadow texture; 64 is the game's, 128 and 256
    /// are sharper ([`crate::engine_fixes::unit_shadow_size`]). Default 64.
    pub unit_shadow_resolution: u32,
    /// `KeepRunningOnFocusLoss`, eix's own: nonzero keeps a windowed game running when it loses the focus. Default 0:
    /// the game pauses, as it does without eix (a single-player game would otherwise go on while the player is away).
    pub keep_running_on_focus_loss: u32,
    /// `ClipCursorToWindow`, eix's own: nonzero keeps the cursor inside a focused windowed game. Default 0.
    pub clip_cursor_to_window: u32,
    /// `VideoResolution`, `WxH`.
    pub video_resolution: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self::from_values(&SettingValues::new())
    }
}

impl Settings {
    /// Reads the options out of the registry values; a missing value or one of the wrong type takes its default.
    pub fn from_values(values: &SettingValues) -> Self {
        let find = |name: &str| {
            values
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v)
        };
        let dword = |name: &str, default: u32| match find(name) {
            Some(SettingValue::Dword(v)) => *v,
            _ => default,
        };
        Self {
            // The reader passes 1 as the default.
            wide_screen: dword("WideScreen", 1),
            objects_draw_radius: dword("ObjectsDrawRadius", 100),
            landscape_draw_radius: dword("LandscapeDrawRadius", 100),
            anti_crash: dword("AntiCrash", 1),
            auto_recover: dword("AutoRecover", 1),
            rate: dword("Rate", 5000),
            console_autoexec: match find("Console AutoExec") {
                Some(SettingValue::MultiText(v)) => v.clone(),
                _ => Vec::new(),
            },
            mq_client: dword("MQ client", 0),
            mp_speed_control: dword("MP Speed-control", 0),
            mp_console: dword("MP console", 0),
            engine_fixes: dword("EngineFixes", 1),
            keep_window_on_focus_loss: dword("KeepWindowOnFocusLoss", 1),
            smooth_camera: dword("SmoothCamera", 1),
            player_list_on_join: dword("PlayerListOnJoin", 1),
            skip_disc_check: dword("SkipDiscCheck", 1),
            unit_shadow_resolution: dword("UnitShadowResolution", 64),
            keep_running_on_focus_loss: dword("KeepRunningOnFocusLoss", 0),
            clip_cursor_to_window: dword("ClipCursorToWindow", 0),
            video_resolution: match find("VideoResolution") {
                Some(SettingValue::Text(v)) => Some(v.clone()),
                _ => None,
            },
        }
    }
}

/// How game.exe was started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CommandLine {
    /// No `addon` argument.
    #[default]
    None,
    /// `game.exe addon <command>`.
    Addon,
    /// `game.exe addon aigraph ...`, which generates the AI graph instead of playing.
    AiGraph,
}

/// Facts about the game folder and the process that the patch groups also depend on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Environment {
    /// game.exe holds the expected bytes that switch table T2 on (true for the RU 1.06 exe).
    pub exe_enables_t2: bool,
    /// The `*.asi` scan that gates table T3 passes.
    pub asi_scan_passes: bool,
    /// A mod is loaded: the mod path is non-empty. Switches table T4 on.
    pub mod_loaded: bool,
    /// The mod folder holds a `binkw32.dll` that is not the game's own module: one that loads as a separate module,
    /// or one that fails to load (BinkPatch then reports the failure, as the original does).
    pub bink_dll_in_mod: bool,
    /// The game's own binkw32 is loaded.
    pub game_bink_loaded: bool,
    /// The game's binkw32 exports `_BinkOpen@8`; the BinkPatch probe records it even when BinkPatch stays off.
    pub bink_open_exported: bool,
    /// Console commands in `2utoexec.cfg` in the game's working folder.
    pub autoexec_file_commands: usize,
    /// The command line.
    pub command_line: CommandLine,
}

impl Default for Environment {
    /// The RU 1.06 exe with no mod folder, no extra files and no arguments.
    fn default() -> Self {
        Self {
            exe_enables_t2: true,
            asi_scan_passes: true,
            mod_loaded: false,
            bink_dll_in_mod: false,
            game_bink_loaded: false,
            bink_open_exported: false,
            autoexec_file_commands: 0,
            command_line: CommandLine::None,
        }
    }
}

/// Everything the patch-group predicates read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    /// The mod's config.reg.
    pub config: ConfigReg,
    /// The mod's ai.reg; the defaults when config.reg names none.
    pub ai: AiReg,
    /// The registry options.
    pub settings: Settings,
    /// The game folder and process facts.
    pub env: Environment,
}

impl Options {
    /// Whether config.reg names an ai.reg, which switches table T5 on.
    pub fn ai_reg_named(&self) -> bool {
        !self.config.config.ai_reg.is_empty()
    }
}

fn int(reg: &Registry, section: &str, key: &str, default: i32) -> i32 {
    reg.get_int(section, key).unwrap_or(default)
}

fn float(reg: &Registry, section: &str, key: &str, default: f32) -> f32 {
    reg.get_float(section, key).unwrap_or(default)
}

fn string(reg: &Registry, section: &str, key: &str) -> Bytes {
    reg.get_str(section, key)
        .map(<[u8]>::to_vec)
        .unwrap_or_default()
}

fn strings(reg: &Registry, section: &str, key: &str) -> Option<Vec<Bytes>> {
    reg.get_str_array(section, key)
        .map(|v| v.into_iter().map(<[u8]>::to_vec).collect())
}

/// An `[MPZone]` key: a string or string array, with one empty last entry dropped.
fn zone_list(reg: &Registry, key: &str) -> Option<Vec<Bytes>> {
    let mut list = strings(reg, "MPZone", key)?;
    if list.last().is_some_and(Vec::is_empty) {
        list.pop();
    }
    Some(list)
}

fn string_list_entries(reg: &Registry, section: &str) -> Vec<(Bytes, Vec<Bytes>)> {
    let Some(section) = reg.section(section) else {
        return Vec::new();
    };
    section
        .keys
        .iter()
        .filter_map(|k| {
            let values = k.value.as_str_array()?;
            Some((
                k.name.clone(),
                values.into_iter().map(<[u8]>::to_vec).collect(),
            ))
        })
        .collect()
}

fn is_lights_ini(name: &[u8]) -> bool {
    name.len() >= 11
        && name[..6].eq_ignore_ascii_case(b"lights")
        && name[name.len() - 4..].eq_ignore_ascii_case(b".ini")
}

impl ConfigReg {
    /// Reads the keys the addon reads out of a parsed config.reg.
    pub fn from_registry(reg: &Registry) -> Self {
        let d = ModSection::default();
        let lights_ini = reg
            .section("CONFIG")
            .map(|s| {
                s.keys
                    .iter()
                    .filter(|k| is_lights_ini(&k.name))
                    // A value that is not a string maps to the mod folder alone, as the original's reader gives it.
                    .map(|k| {
                        (
                            k.name.clone(),
                            k.value.as_str().unwrap_or_default().to_vec(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            mod_: ModSection {
                multi: int(reg, "MOD", "Multi", d.multi),
                single: int(reg, "MOD", "Single", d.single),
                game_saves: int(reg, "MOD", "GameSaves", d.game_saves),
                prevent_zone1_animation: int(
                    reg,
                    "MOD",
                    "PreventZone1Animation",
                    d.prevent_zone1_animation,
                ),
                force_show_tutorial: int(reg, "MOD", "ForceShowTutorial", d.force_show_tutorial),
                show_tutorial_default: reg.get_int("MOD", "ShowTutorialDefault"),
                addon_version: string(reg, "MOD", "AddonVersion"),
            },
            res: string_list_entries(reg, "RES"),
            config: ConfigSection {
                movie_ini: string(reg, "CONFIG", "movie.ini"),
                tutorial_ini: string(reg, "CONFIG", "tutorial.ini"),
                ai_reg: string(reg, "CONFIG", "ai.reg"),
                music_reg: string(reg, "CONFIG", "music.reg"),
                streamsn_reg: string(reg, "CONFIG", "streamsn.reg"),
                lights_ini,
            },
            dev: DevSection {
                game_debug_file: string(reg, "DEV", "GameDebugFile"),
                chat_html_file: string(reg, "DEV", "ChatHtmlFile"),
                plugin: string(reg, "DEV", "Plugin"),
            },
            mp_zone: MpZone {
                gipath: zone_list(reg, "Gipath"),
                ingos: zone_list(reg, "Ingos"),
                suslanger: zone_list(reg, "Suslanger"),
            },
        }
    }
}

impl AiReg {
    /// Reads the keys the addon reads out of a parsed ai.reg.
    pub fn from_registry(reg: &Registry) -> Self {
        let a = AddonSection::default();
        let i = |key: &str| int(reg, "Addon", key, 0);
        let p = AddonPerk::default();
        let x = AddonExp::default();
        Self {
            addon: AddonSection {
                show_all_quests: i("ShowAllQuests"),
                disable_runaway: i("DisableRunaway"),
                archers_defence: i("ArchersDefence"),
                distance_modifier: i("DistanceModifier"),
                weight_bonus: i("WeightBonus"),
                magic_random: float(reg, "Addon", "MagicRandom", a.magic_random),
                allow_magic_negative: i("AllowMagicNegative"),
                deagr_player_on_loose: i("DeagrPlayerOnLoose"),
                attack_anim_speed: i("AttackAnimSpeed"),
                new_runes: i("NewRunes"),
                use_prototype_actions_for_mob_attack: i("UsePrototypeActionsForMobAttack"),
                use_prototype_actions_for_mob_cast: i("UsePrototypeActionsForMobCast"),
                fast_teleport: i("FastTeleport"),
                disable_crippled_hl: i("DisableCrippledHL"),
                unlock_experimental_spells: i("UnlockExperimentalSpells"),
                load_spell_animation_from_slots_field: i("LoadSpellAnimationFromSlotsField"),
                send_spell_effect_value_to_players: i("SendSpellEffectValueToPlayers"),
                anti_crash: reg.get_int("Addon", "AntiCrash"),
                log_errors_in_file: i("LogErrorsInFile"),
                log_errors_in_console: i("LogErrorsInConsole"),
                log_errors_in_chat: i("LogErrorsInChat"),
                enable_showing_script_errors: i("EnableShowingScriptErrors"),
                improve_info_windows: i("ImproveInfoWindows"),
                remove_spell_construct_reqs: i("RemoveSpellConstructReqs"),
                add_weapons_from_db_to_all_units: i("AddWeaponsFromDbToAllUnits"),
                unlock_corpses: i("UnlockCorpses"),
                lua_main_script_path: reg
                    .get_str("Addon", "LuaMainScriptPath")
                    .map_or(a.lua_main_script_path, <[u8]>::to_vec),
                enable_lua_scripts: i("EnableLuaScripts"),
            },
            exp: AddonExp {
                charge_method: int(reg, "AddonExp", "ChargeMethod", x.charge_method),
                mul_factor: float(reg, "AddonExp", "MulFactor", x.mul_factor),
                limit: float(reg, "AddonExp", "Limit", x.limit),
            },
            perk: AddonPerk {
                melee: int(reg, "AddonPerk", "Melee", p.melee),
                archery: int(reg, "AddonPerk", "Archery", p.archery),
                elemental: int(reg, "AddonPerk", "Elemental", p.elemental),
                sense: int(reg, "AddonPerk", "Sense", p.sense),
                astral: int(reg, "AddonPerk", "Astral", p.astral),
            },
        }
    }
}
