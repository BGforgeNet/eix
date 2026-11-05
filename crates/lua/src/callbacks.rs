//! The 19 callbacks, the calling side: each pushes its arguments as the original does (pointers and ints
//! as integers, floats widened, flags as booleans) and reads the result as the original does. The patch groups hook the
//! game and call these.

use crate::convert::{bool_result, float_result, int_result, ptr_to_lua};
use crate::host::Host;

impl Host {
    fn int_callback(&self, name: &str, args: impl mlua::IntoLuaMulti) -> i32 {
        int_result(self.lua(), &self.call_global(name, args))
    }

    fn float_callback(&self, name: &str, args: impl mlua::IntoLuaMulti) -> f32 {
        float_result(self.lua(), &self.call_global(name, args))
    }

    /// `GetSkillPriceDiff(from, to)`: the experience cost of a skill step (SchoolPrice, RaiseSkill,
    /// RaiseAttribute).
    pub fn get_skill_price_diff(&self, from: i32, to: i32) -> i32 {
        self.int_callback("GetSkillPriceDiff", (from, to))
    }

    /// `GetPerkPrice(stats, perkRecord)`: the experience cost of a perk (UnitStats::SkillPrice).
    pub fn get_perk_price(&self, stats: u32, perk_record: u32) -> i32 {
        self.int_callback("GetPerkPrice", (ptr_to_lua(stats), ptr_to_lua(perk_record)))
    }

    /// `GetModifiedDamageOnServer(ecx, unit, damage)`: ScaleDamageByEffects 0x523d30; `ecx` is the caller's ECX.
    pub fn get_modified_damage_on_server(&self, ecx: u32, unit: u32, damage: f32) -> f32 {
        let args = (ptr_to_lua(ecx), ptr_to_lua(unit), f64::from(damage));
        self.float_callback("GetModifiedDamageOnServer", args)
    }

    /// `GetModifiedDamageOnClient(ecx, unit, damage)`: ScaleDamageByEffects 0x5285d0.
    pub fn get_modified_damage_on_client(&self, ecx: u32, unit: u32, damage: f32) -> f32 {
        let args = (ptr_to_lua(ecx), ptr_to_lua(unit), f64::from(damage));
        self.float_callback("GetModifiedDamageOnClient", args)
    }

    /// `CalcBaseHealth(stats)`: UnitStats::HitPointCurve and the RefreshMaxima' users.
    pub fn calc_base_health(&self, stats: u32) -> f32 {
        self.float_callback("CalcBaseHealth", ptr_to_lua(stats))
    }

    /// `CalcBaseMana(stats)`: UnitStats::ManaCurve and RefreshMaxima'.
    pub fn calc_base_mana(&self, stats: u32) -> f32 {
        self.float_callback("CalcBaseMana", ptr_to_lua(stats))
    }

    /// `GetModifiedHealth(stats, unit, hp)`: ScaleHitPointsByEffects and the RefreshMaxima' users.
    pub fn get_modified_health(&self, stats: u32, unit: u32, hp: f32) -> f32 {
        let args = (ptr_to_lua(stats), ptr_to_lua(unit), f64::from(hp));
        self.float_callback("GetModifiedHealth", args)
    }

    /// `RecalcUnitStats(stats, vitals, unit, effectMaxima)`: UnitStats::Recompute. `effect_maxima` is the address of
    /// the caller's `float[40]`.
    pub fn recalc_unit_stats(&self, stats: u32, vitals: u32, unit: u32, effect_maxima: u32) {
        let args = (
            ptr_to_lua(stats),
            ptr_to_lua(vitals),
            ptr_to_lua(unit),
            ptr_to_lua(effect_maxima),
        );
        self.call_global("RecalcUnitStats", args);
    }

    /// `RecalcInternalHeroStats(stats, newLoad)`: UnitStats::ResetToBase.
    pub fn recalc_internal_hero_stats(&self, stats: u32, new_load: f32) {
        let args = (ptr_to_lua(stats), f64::from(new_load));
        self.call_global("RecalcInternalHeroStats", args);
    }

    /// `RegenerateHpAndMp(vitals, stats, unit, inCombat)`: UnitVitals::Regenerate; the game's "something changed".
    pub fn regenerate_hp_and_mp(
        &self,
        vitals: u32,
        stats: u32,
        unit: u32,
        in_combat: bool,
    ) -> bool {
        let args = (
            ptr_to_lua(vitals),
            ptr_to_lua(stats),
            ptr_to_lua(unit),
            in_combat,
        );
        bool_result(&self.call_global("RegenerateHpAndMp", args))
    }

    /// `CalcRunningManaCost(stats, unit, vitals)`: the running drain in CUnitServer::TickMovement.
    pub fn calc_running_mana_cost(&self, stats: u32, unit: u32, vitals: u32) {
        let args = (ptr_to_lua(stats), ptr_to_lua(unit), ptr_to_lua(vitals));
        self.call_global("CalcRunningManaCost", args);
    }

    /// `CalcAttackParamsOnServer(stats, unit, params, effectMaxima)`: CUnitServer::GetAttackParams.
    pub fn calc_attack_params_on_server(
        &self,
        stats: u32,
        unit: u32,
        params: u32,
        effect_maxima: u32,
    ) {
        let args = (
            ptr_to_lua(stats),
            ptr_to_lua(unit),
            ptr_to_lua(params),
            ptr_to_lua(effect_maxima),
        );
        self.call_global("CalcAttackParamsOnServer", args);
    }

    /// `CalcAttackParamsOnClient(unitParams, unit, params, effectMaxima)`: CUnitClientSpecific::GetAttackParams.
    pub fn calc_attack_params_on_client(
        &self,
        unit_params: u32,
        unit: u32,
        params: u32,
        effect_maxima: u32,
    ) {
        let args = (
            ptr_to_lua(unit_params),
            ptr_to_lua(unit),
            ptr_to_lua(params),
            ptr_to_lua(effect_maxima),
        );
        self.call_global("CalcAttackParamsOnClient", args);
    }

    /// `GetManaBonusOnServer(stats)`: UnitStats::Period20.
    pub fn get_mana_bonus_on_server(&self, stats: u32) -> f32 {
        self.float_callback("GetManaBonusOnServer", ptr_to_lua(stats))
    }

    /// `GetManaBonusOnClient(params)`: UnitParams::Period.
    pub fn get_mana_bonus_on_client(&self, params: u32) -> f32 {
        self.float_callback("GetManaBonusOnClient", ptr_to_lua(params))
    }

    /// `GetExpMultiplierOnServer(stats)`: AddScaledExperience and the info panel percentage.
    pub fn get_exp_multiplier_on_server(&self, stats: u32) -> f32 {
        self.float_callback("GetExpMultiplierOnServer", ptr_to_lua(stats))
    }

    /// `GetExpMultiplierOnClient(params)`: the experience numbers over heroes.
    pub fn get_exp_multiplier_on_client(&self, params: u32) -> f32 {
        self.float_callback("GetExpMultiplierOnClient", ptr_to_lua(params))
    }

    /// `RecalcShopsPriceMultipliers(multipliers, isLmp)`: `multipliers` is the address of the `count * 9` floats the
    /// caller gathered from the shop records.
    pub fn recalc_shops_price_multipliers(&self, multipliers: u32, is_lmp: bool) {
        let args = (ptr_to_lua(multipliers), is_lmp);
        self.call_global("RecalcShopsPriceMultipliers", args);
    }

    /// `DistributeLmpExp(exp, players)`: each player's share of a kill's experience in an LMP game.
    pub fn distribute_lmp_exp(&self, exp: f32, players: i32) -> f32 {
        self.float_callback("DistributeLmpExp", (f64::from(exp), players))
    }
}
