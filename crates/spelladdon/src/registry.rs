//! Every SpellAddon feature: one per INI flag that has a patch of its own, in the original's order (which decides who
//! wins where two features write the same bytes), and the module that implements it. A feature is implemented when
//! its module under [`crate::features`] has a build for it.

use eix_core::spelladdon::Values;

use crate::features::{self, Build};

/// The module under [`crate::features`] that implements a feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Area {
    Ai,
    Combat,
    Items,
    Magic,
    Multiplayer,
    Rebalance,
    Script,
    Startup,
    World,
}

impl Area {
    /// The builds of the module: `(section, key, build)`.
    pub(crate) fn builds(self) -> &'static [(&'static str, &'static str, Build)] {
        match self {
            Self::Ai => features::ai::BUILDS,
            Self::Combat => features::combat::BUILDS,
            Self::Items => features::items::BUILDS,
            Self::Magic => features::magic::BUILDS,
            Self::Multiplayer => features::multiplayer::BUILDS,
            Self::Rebalance => features::rebalance::BUILDS,
            Self::Script => features::script::BUILDS,
            Self::Startup => features::startup::BUILDS,
            Self::World => features::world::BUILDS,
        }
    }
}

/// One feature: its flag key and the module that implements it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Feature {
    pub(crate) section: &'static str,
    pub(crate) key: &'static str,
    pub(crate) area: Area,
}

impl Feature {
    const fn new(section: &'static str, key: &'static str, area: Area) -> Self {
        Self { section, key, area }
    }

    /// The feature's build, when its module has one.
    pub(crate) fn build(&self) -> Option<Build> {
        self.area
            .builds()
            .iter()
            .find(|(s, k, _)| *s == self.section && *k == self.key)
            .map(|(_, _, b)| *b)
    }
}

/// Why a feature that is switched on is skipped: its original patch faults or corrupts the game's state, so it is
/// logged and left out while every other feature applies.
pub(crate) fn skipped(f: &Feature, value: i8, values: &Values) -> Option<&'static str> {
    let flag = |section: &str, key: &str| values.flag(section, key).unwrap_or(0);
    match (f.section, f.key, value) {
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
/// bytes the later one wins, so it must not be rearranged (`later_features_win_shared_bytes` pins pairs that overlap).
pub(crate) static FEATURES: [Feature; 191] = [
    Feature::new("gameplay", "archersdefence", Area::Combat),
    Feature::new("gameplay", "rewritenpcvisionarc", Area::Ai),
    Feature::new("gameplay", "shopschanges", Area::Items),
    Feature::new("fonts", "changefont", Area::World),
    Feature::new("gameplay", "batterycharge", Area::Magic),
    Feature::new("gameplay", "infiniteaddmobspawn", Area::Script),
    Feature::new("gameplay", "splashdamageonair", Area::Magic),
    Feature::new("speed", "allowmpspeed", Area::Multiplayer),
    Feature::new("gameplay", "autorunmobfile", Area::Script),
    Feature::new("gameplay", "nonstopmanaregen", Area::Magic),
    Feature::new("gameplay", "unbreakablebodyparts", Area::Combat),
    Feature::new("gameplay", "importantalerts", Area::Startup),
    Feature::new("gameplay", "newspells", Area::Magic),
    Feature::new("gameplay", "saveall", Area::Items),
    Feature::new("gameplay", "extendedmerc", Area::Items),
    Feature::new("rebalanceengine", "rewriteskillscounting", Area::Rebalance),
    Feature::new("rebalanceengine", "rewriterunabilitytime", Area::Rebalance),
    Feature::new("gameplay", "fullexpforall", Area::Multiplayer),
    Feature::new("gameplay", "possessioncharmlink", Area::Magic),
    Feature::new("gameplay", "damageboost", Area::Combat),
    Feature::new("skills", "extendedskills", Area::Items),
    Feature::new("stealing", "extendedstealing", Area::Items),
    Feature::new("gameplay", "unlockmoney", Area::Items),
    Feature::new("gameplay", "newattackdefence", Area::Combat),
    Feature::new("gameplay", "manygameexe", Area::Multiplayer),
    Feature::new("gameplay", "enablecheatsinmulti", Area::Multiplayer),
    Feature::new("gameplay", "visionfix", Area::Ai),
    Feature::new("gameplay", "aggrmod", Area::Ai),
    Feature::new("gameplay", "enablekillsinmulti", Area::Combat),
    Feature::new("gameplay", "allvisible", Area::Ai),
    Feature::new("animationsspeed", "rewriteactionsspeed", Area::Rebalance),
    Feature::new("gameplay", "rewriteperssize", Area::World),
    Feature::new("rebalanceengine", "newrunsystem", Area::Rebalance),
    Feature::new("gameplay", "extendedparams", Area::Multiplayer),
    Feature::new("gameplay", "longnick", Area::Multiplayer),
    Feature::new("speed", "rewritespeed", Area::Multiplayer),
    Feature::new("speed", "rewritespeedkey", Area::Multiplayer),
    Feature::new("speed", "disablepausekey ", Area::Multiplayer),
    Feature::new("speed", "disablepause", Area::Multiplayer),
    Feature::new("speed", "showallquestsinmulti", Area::Multiplayer),
    Feature::new("speed", "spellsaimod", Area::Magic),
    Feature::new("gameplay", "infowindowschanges", Area::World),
    Feature::new("speed", "spellsarealwaysconstructable", Area::Magic),
    Feature::new("gameplay", "diffchange", Area::Rebalance),
    Feature::new("speed", "rewriteotherspeed", Area::Multiplayer),
    Feature::new("speed", "gsgetvar", Area::Script),
    Feature::new("speed", "gssetvar", Area::Script),
    Feature::new("gameplay", "versionalerts", Area::Startup),
    Feature::new("gameplay", "nogameoveronleadersdeath", Area::World),
    Feature::new("gameplay", "lastspellsdamagetype", Area::Magic),
    Feature::new("saves", "savecontrol", Area::Script),
    Feature::new("gameplay", "rangedweaponbackstabs", Area::Combat),
    Feature::new("gameplay", "randomlessaimedhits", Area::Combat),
    Feature::new("gameplay", "allodsinfo", Area::World),
    Feature::new(
        "rebalanceengine",
        "rewriteherolevelcounting",
        Area::Rebalance,
    ),
    Feature::new("rebalanceengine", "useonlymindamage", Area::Combat),
    Feature::new("gameplay", "npcsmana", Area::Magic),
    Feature::new("gameplay", "showtype", Area::World),
    Feature::new("gameplay", "speedboost", Area::Rebalance),
    Feature::new("gameplay", "changeweightrangebonuses", Area::Combat),
    Feature::new("gameplay", "spellsprioritys", Area::Ai),
    Feature::new("gameplay", "removenpcsblock", Area::Ai),
    Feature::new("gameplay", "changeexpforint", Area::Rebalance),
    Feature::new("gameplay", "copystatscut", Area::Items),
    Feature::new("gameplay", "strengthstats", Area::Combat),
    Feature::new("gameplay", "expirementalfxes", Area::World),
    Feature::new("gameplay", "shopsnewnormalchanges", Area::Items),
    Feature::new("gameplay", "liftchanges", Area::Items),
    Feature::new("gameplay", "liftdebuffs", Area::Items),
    Feature::new("gameplay", "everyoneishero", Area::Ai),
    Feature::new("gameplay", "heroalwayscanrun", Area::Items),
    Feature::new("gameplay", "improvedstealing", Area::Items),
    Feature::new("rebalanceengine", "backstabvisualbase", Area::Rebalance),
    Feature::new("gameplay", "teleportboost", Area::Magic),
    Feature::new("rebalanceengine", "difficultyreset", Area::Rebalance),
    Feature::new("rebalanceengine", "meleesum_low", Area::Combat),
    Feature::new("gameplay", "noruinszoneanimation", Area::World),
    Feature::new("gameplay", "showmanaforeveryone", Area::World),
    Feature::new("gameplay", "meterscale", Area::World),
    Feature::new("gameplay", "standtrans", Area::Script),
    Feature::new("gameplay", "possessionpatch", Area::Magic),
    Feature::new("rebalanceengine", "meleesum_1", Area::Combat),
    Feature::new("rebalanceengine", "meleesum_2", Area::Combat),
    Feature::new("gameplay", "waslootedtrans", Area::Script),
    Feature::new("gameplay", "castscripttrans", Area::Script),
    Feature::new("gameplay", "armorspell", Area::Combat),
    Feature::new("gameplay", "newrangedweapontypes", Area::Combat),
    Feature::new("gameplay", "inspirationintelligence", Area::Magic),
    Feature::new("gameplay", "lightsboost", Area::World),
    Feature::new("gameplay", "shrinkenlargerebalance", Area::Magic),
    Feature::new("gameplay", "hidefireworks", Area::Magic),
    Feature::new("gameplay", "wallsfrequency", Area::Magic),
    Feature::new("gameplay", "wallspercentage", Area::Magic),
    Feature::new("gameplay", "controlmagic", Area::Magic),
    Feature::new("gameplay", "newperks", Area::Rebalance),
    Feature::new("gameplay", "allowheroesarmors", Area::Combat),
    Feature::new("gameplay", "mainmenuweather", Area::World),
    Feature::new("gameplay", "visioninweather", Area::Ai),
    Feature::new("gameplay", "dmgtypes", Area::Items),
    Feature::new("gameplay", "nhits", Area::Rebalance),
    Feature::new("gameplay", "npcsspellcastingtactics", Area::Ai),
    Feature::new("gameplay", "effectredraws", Area::Magic),
    Feature::new("gameplay", "stenchremaster", Area::Ai),
    Feature::new("gameplay", "regenerationremaster", Area::Magic),
    Feature::new("gameplay", "newplayersafetyconditions", Area::Ai),
    Feature::new("gameplay", "speedslowremaster", Area::Magic),
    Feature::new("gameplay", "actionspatch", Area::Rebalance),
    Feature::new("gameplay", "encumbrancemovespeed", Area::Items),
    Feature::new("gameplay", "encumbranceattackspeed", Area::Items),
    Feature::new("gameplay", "camp", Area::Items),
    Feature::new("gameplay", "newoptions", Area::Items),
    Feature::new("gameplay", "notrash", Area::World),
    Feature::new("gameplay", "hptrans", Area::Script),
    Feature::new("gameplay", "mptrans", Area::Script),
    Feature::new("gameplay", "genocideincvars", Area::Script),
    Feature::new("gameplay", "genocidedipvars", Area::Script),
    Feature::new("gameplay", "dontmergegipathzones", Area::World),
    Feature::new("gameplay", "backstabsforall", Area::Combat),
    Feature::new("gameplay", "sizerebalance", Area::Ai),
    Feature::new("gameplay", "nightpowervisionpatch", Area::Ai),
    Feature::new("gameplay", "giveunitspelltrans", Area::Script),
    Feature::new("gameplay", "musictimings", Area::World),
    Feature::new("gameplay", "fearpatch", Area::Ai),
    Feature::new("gameplay", "playmusictrans", Area::Script),
    Feature::new("gameplay", "notifydeaths", Area::Script),
    Feature::new("gameplay", "exploredcolors", Area::World),
    Feature::new("gameplay", "disabledragon", Area::World),
    Feature::new("gameplay", "newcounters", Area::World),
    Feature::new("gameplay", "stupidenemies", Area::Ai),
    Feature::new("gameplay", "increasinghitchance", Area::Combat),
    Feature::new("gameplay", "runawaydistcontrol", Area::Combat),
    Feature::new("gameplay", "otherspellcastingai", Area::Ai),
    Feature::new("gameplay", "nightpower", Area::Combat),
    Feature::new("gameplay", "vampirism", Area::Magic),
    Feature::new("gameplay", "infowindowremake", Area::World),
    Feature::new("gameplay", "avgarmors", Area::Items),
    Feature::new("gameplay", "itemslightchanges", Area::Items),
    Feature::new("gameplay", "offensivespells", Area::Magic),
    Feature::new("gameplay", "acidattack", Area::Combat),
    Feature::new("gameplay", "castspellpointtrans", Area::Script),
    Feature::new("gameplay", "castspellunittrans", Area::Script),
    Feature::new("gameplay", "legendaryspellsystem", Area::Script),
    Feature::new("gameplay", "maxmanatrans", Area::Script),
    Feature::new("aicastertable", "rebuildedaicastertable", Area::Ai),
    Feature::new("gameplay", "newbigtables", Area::Ai),
    Feature::new("aioffendtable", "rebuildedaioffendtable", Area::Ai),
    Feature::new("gameplay", "linkremaster", Area::Magic),
    Feature::new("gameplay", "allowcastsonbriefings", Area::Magic),
    Feature::new("gameplay", "drawvisiblepatch", Area::World),
    Feature::new("gameplay", "fhrunetrans", Area::Startup),
    Feature::new("gameplay", "heroscanhideweapons", Area::Items),
    Feature::new("gameplay", "newdifficultylevels", Area::Rebalance),
    Feature::new("gameplay", "fakesrcrune", Area::Magic),
    Feature::new("gameplay", "filtersarealwaysusing", Area::Magic),
    Feature::new("gameplay", "disableexppenalty", Area::Multiplayer),
    Feature::new("gameplay", "unitsrunfromdangers", Area::Ai),
    Feature::new("gameplay", "tornadochanges", Area::Magic),
    Feature::new("gameplay", "nightvisionincaves", Area::Ai),
    Feature::new("gameplay", "actionsdexteritymovespeed", Area::Rebalance),
    Feature::new("gameplay", "removefrozenvar", Area::Script),
    Feature::new("gameplay", "freemultiplayervars", Area::Script),
    Feature::new("gameplay", "playingnewloadingscreens", Area::World),
    Feature::new("gameplay", "allysseeseachothers", Area::Ai),
    Feature::new("gameplay", "changesounds", Area::Magic),
    Feature::new("gameplay", "cleveranimationsspeedup", Area::Rebalance),
    Feature::new("gameplay", "movetoobjecttrans", Area::Script),
    Feature::new("gameplay", "hyperreactions", Area::Ai),
    Feature::new("gameplay", "groupseetrans", Area::Script),
    Feature::new("gameplay", "unitseetrans", Area::Script),
    Feature::new("gameplay", "newzonescpecificmusic", Area::World),
    Feature::new("gameplay", "notifylootings", Area::Script),
    Feature::new("gameplay", "automaticruncontrol", Area::Rebalance),
    Feature::new("gameplay", "newallodsacks", Area::World),
    Feature::new("gameplay", "segmentsonexitenter", Area::World),
    Feature::new("gameplay", "hearingreactionspatch", Area::Ai),
    Feature::new("gameplay", "strengthweakrebalance", Area::Magic),
    Feature::new("gameplay", "getplayertrans", Area::Script),
    Feature::new("gameplay", "runtrans", Area::Script),
    Feature::new("gameplay", "crawltrans", Area::Script),
    Feature::new("gameplay", "setplayertrans", Area::Script),
    Feature::new("gameplay", "getaiclasstrans", Area::Script),
    Feature::new("speedrun", "timervars", Area::Multiplayer),
    Feature::new("gameplay", "bigmapspatch", Area::World),
    Feature::new("gameplay", "ultrafixes", Area::World),
    Feature::new("tech", "scriptchecking", Area::Script),
    Feature::new("tech", "drawunitsids", Area::Script),
    Feature::new("gameplay", "killunittrans", Area::Script),
    Feature::new("gameplay", "fixworldtimetrans", Area::Script),
    Feature::new("gameplay", "hiddenzonesexits", Area::World),
    Feature::new("gameplay", "setspellaggressiontrans", Area::Script),
    Feature::new("gameplay", "duels", Area::Combat),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::Ctx;
    use crate::features::testing::{no_log, values};
    use eix_core::spelladdon::{KEYS, Kind};

    /// Features that write overlapping bytes differently, as `(earlier, value, later, value)`.
    const OVERLAPS: [(&str, i8, &str, i8); 8] = [
        ("rewritenpcvisionarc", 1, "removenpcsblock", 1),
        ("autorunmobfile", 1, "newspells", 1),
        ("genocideincvars", 1, "genocidedipvars", 1),
        ("newattackdefence", 1, "increasinghitchance", 1),
        ("rewriteactionsspeed", 1, "encumbranceattackspeed", 1),
        ("rangedweaponbackstabs", 1, "newrangedweapontypes", 1),
        ("damageboost", 3, "nightpower", 2),
        ("actionspatch", 1, "encumbranceattackspeed", 2),
    ];

    #[test]
    fn later_features_win_shared_bytes() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        let position = |key: &str| FEATURES.iter().position(|f| f.key == key).unwrap();
        let writes = |key: &str, value: i8| {
            let build = FEATURES[position(key)].build().unwrap();
            build(&ctx, value).unwrap()
        };
        for (first, first_value, second, second_value) in OVERLAPS {
            assert!(
                position(first) < position(second),
                "{first} must come before {second}"
            );
            let (a, b) = (writes(first, first_value), writes(second, second_value));
            let differs = a.iter().any(|x| {
                b.iter().any(|y| {
                    let (lo, hi) = (
                        x.site().max(y.site()),
                        (x.site() + x.expected().len()).min(y.site() + y.expected().len()),
                    );
                    (lo..hi).any(|at| x.new_bytes()[at - x.site()] != y.new_bytes()[at - y.site()])
                })
            });
            assert!(
                differs,
                "{first} and {second} no longer write different bytes to one site"
            );
        }
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
    fn every_build_belongs_to_a_feature_of_its_module() {
        for area in [
            Area::Ai,
            Area::Combat,
            Area::Items,
            Area::Magic,
            Area::Multiplayer,
            Area::Rebalance,
            Area::Script,
            Area::Startup,
            Area::World,
        ] {
            for (section, key, _) in area.builds() {
                assert!(
                    FEATURES
                        .iter()
                        .any(|f| f.section == *section && f.key == *key && f.area == area),
                    "{area:?} builds [{section}] {key}, which is not one of its features"
                );
            }
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
    }
}
