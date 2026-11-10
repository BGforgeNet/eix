//! The callbacks a mod's main Lua script may define, and the set it defined. A Lua group hooks a site only when the
//! script defines a callback the site reaches; elsewhere the game's own code runs.

/// One callback of the nine Lua groups, named as the script defines it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Callback {
    /// `GetSkillPriceDiff`.
    GetSkillPriceDiff,
    /// `GetPerkPrice`.
    GetPerkPrice,
    /// `GetModifiedDamageOnServer`.
    GetModifiedDamageOnServer,
    /// `GetModifiedDamageOnClient`.
    GetModifiedDamageOnClient,
    /// `CalcBaseHealth`.
    CalcBaseHealth,
    /// `CalcBaseMana`.
    CalcBaseMana,
    /// `GetModifiedHealth`.
    GetModifiedHealth,
    /// `RecalcUnitStats`.
    RecalcUnitStats,
    /// `RecalcInternalHeroStats`.
    RecalcInternalHeroStats,
    /// `RegenerateHpAndMp`.
    RegenerateHpAndMp,
    /// `CalcRunningManaCost`.
    CalcRunningManaCost,
    /// `CalcAttackParamsOnServer`.
    CalcAttackParamsOnServer,
    /// `CalcAttackParamsOnClient`.
    CalcAttackParamsOnClient,
    /// `GetManaBonusOnServer`.
    GetManaBonusOnServer,
    /// `GetManaBonusOnClient`.
    GetManaBonusOnClient,
    /// `GetExpMultiplierOnServer`.
    GetExpMultiplierOnServer,
    /// `GetExpMultiplierOnClient`.
    GetExpMultiplierOnClient,
    /// `RecalcShopsPriceMultipliers`.
    RecalcShopsPriceMultipliers,
    /// `DistributeLmpExp`.
    DistributeLmpExp,
}

impl Callback {
    /// Every callback, in the order of the Lua reference.
    pub const ALL: [Self; 19] = [
        Self::GetSkillPriceDiff,
        Self::GetPerkPrice,
        Self::GetModifiedDamageOnServer,
        Self::GetModifiedDamageOnClient,
        Self::CalcBaseHealth,
        Self::CalcBaseMana,
        Self::GetModifiedHealth,
        Self::RecalcUnitStats,
        Self::RecalcInternalHeroStats,
        Self::RegenerateHpAndMp,
        Self::CalcRunningManaCost,
        Self::CalcAttackParamsOnServer,
        Self::CalcAttackParamsOnClient,
        Self::GetManaBonusOnServer,
        Self::GetManaBonusOnClient,
        Self::GetExpMultiplierOnServer,
        Self::GetExpMultiplierOnClient,
        Self::RecalcShopsPriceMultipliers,
        Self::DistributeLmpExp,
    ];

    /// The global the script defines.
    pub const fn name(self) -> &'static str {
        match self {
            Self::GetSkillPriceDiff => "GetSkillPriceDiff",
            Self::GetPerkPrice => "GetPerkPrice",
            Self::GetModifiedDamageOnServer => "GetModifiedDamageOnServer",
            Self::GetModifiedDamageOnClient => "GetModifiedDamageOnClient",
            Self::CalcBaseHealth => "CalcBaseHealth",
            Self::CalcBaseMana => "CalcBaseMana",
            Self::GetModifiedHealth => "GetModifiedHealth",
            Self::RecalcUnitStats => "RecalcUnitStats",
            Self::RecalcInternalHeroStats => "RecalcInternalHeroStats",
            Self::RegenerateHpAndMp => "RegenerateHpAndMp",
            Self::CalcRunningManaCost => "CalcRunningManaCost",
            Self::CalcAttackParamsOnServer => "CalcAttackParamsOnServer",
            Self::CalcAttackParamsOnClient => "CalcAttackParamsOnClient",
            Self::GetManaBonusOnServer => "GetManaBonusOnServer",
            Self::GetManaBonusOnClient => "GetManaBonusOnClient",
            Self::GetExpMultiplierOnServer => "GetExpMultiplierOnServer",
            Self::GetExpMultiplierOnClient => "GetExpMultiplierOnClient",
            Self::RecalcShopsPriceMultipliers => "RecalcShopsPriceMultipliers",
            Self::DistributeLmpExp => "DistributeLmpExp",
        }
    }

    const fn bit(self) -> u32 {
        1 << self as u32
    }
}

/// The callbacks the main script defined, read once after it ran.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Defined(u32);

impl Defined {
    /// No callback.
    pub const EMPTY: Self = Self(0);

    /// The callbacks for which `is_function` holds.
    pub fn from_fn(is_function: impl Fn(&str) -> bool) -> Self {
        Self(
            Callback::ALL
                .iter()
                .filter(|c| is_function(c.name()))
                .fold(0, |bits, c| bits | c.bit()),
        )
    }

    /// Whether the script defined `callback`.
    pub const fn has(self, callback: Callback) -> bool {
        self.0 & callback.bit() != 0
    }

    /// Whether the script defined any of `callbacks`: a site that may reach them is hooked.
    pub fn any(self, callbacks: &[Callback]) -> bool {
        callbacks.iter().any(|&c| self.has(c))
    }

    /// The defined callbacks' names, in reference order.
    pub fn names(self) -> Vec<&'static str> {
        Callback::ALL
            .iter()
            .filter(|&&c| self.has(c))
            .map(|c| c.name())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_callback_has_its_own_bit_and_name() {
        let mut bits = 0u32;
        let mut names = std::collections::HashSet::new();
        for c in Callback::ALL {
            assert_eq!(bits & c.bit(), 0, "{c:?}");
            bits |= c.bit();
            assert!(names.insert(c.name()), "{c:?}");
        }
    }

    #[test]
    fn only_the_defined_callbacks_count() {
        let defined = Defined::from_fn(|n| n == "CalcBaseHealth" || n == "DistributeLmpExp");
        assert!(defined.has(Callback::CalcBaseHealth));
        assert!(!defined.has(Callback::CalcBaseMana));
        assert!(defined.any(&[Callback::CalcBaseMana, Callback::CalcBaseHealth]));
        assert!(!defined.any(&[Callback::CalcBaseMana, Callback::GetModifiedHealth]));
        assert!(!defined.any(&[]));
        assert_eq!(defined.names(), ["CalcBaseHealth", "DistributeLmpExp"]);
        assert_eq!(Defined::default().names(), Vec::<&str>::new());
    }

    /// The callbacks match the reference's: each `function` of eix.lua's callback section, in its order.
    #[test]
    fn the_callbacks_are_the_references() {
        let reference = include_str!("../../../docs/eix.lua");
        let Some((_, section)) = reference.split_once("-- Section: Callbacks supplied by the mod")
        else {
            panic!("eix.lua has no callback section");
        };
        let section = section
            .split_once("-- Section:")
            .map_or(section, |(callbacks, _)| callbacks);
        let declared: Vec<&str> = section
            .lines()
            .filter_map(|l| l.strip_prefix("function "))
            .filter_map(|l| l.split_once('(').map(|(name, _)| name))
            .collect();
        let ours: Vec<&str> = Callback::ALL.iter().map(|c| c.name()).collect();
        assert_eq!(declared, ours);
    }
}
