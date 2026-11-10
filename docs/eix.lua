---@meta
-- Definitions of the Lua API of a mod's main script, for the Lua language server (LuaLS): put this file in the
-- workspace library. It is also the source of the generated page docs/lua-reference.md.
--
-- Comment lines that start with two dashes and a space are not part of the API: a `Section:` line starts a section of
-- the page, and the comment lines after it, up to a blank line, are the section's introduction.

-- Section: Types
-- The one type of this API that is not a Lua built-in.

---A game address, passed as the 32 bits of the address read as a signed integer: an address above 2 GB is negative.
---Raw memory functions need an address that holds valid game memory. A record or slot index is zero-based and must lie
---inside the table or the slots.
---@alias Pointer integer

-- Section: Functions supplied by eix
-- Global functions that addon.dll registers before the main script runs. A signed 32-bit integer argument is read as
-- Lua's `luaL_checkinteger` reads it: a number or a numeric string, truncated. A bad argument raises LuaJIT's usual
-- argument error.

---Rounds half away from zero.
---@param value number
---@return number
function Round(value) end

---Signed integer quotient. A zero divisor or an overflow ends the game.
---@param a integer
---@param b integer
---@return integer
function IDiv(a, b) end

---Signed integer remainder. A zero divisor or an overflow ends the game.
---@param a integer
---@param b integer
---@return integer
function IMod(a, b) end

---The strength of an effect on a unit.
---@param unit Pointer
---@param effect integer
---@return number
function GetEffectValue(unit, effect) end

---The modifier of a perk, read from a unit's stats on the server.
---@param stats Pointer
---@param perk integer
---@return number
function GetPerkModifierForServer(stats, perk) end

---The modifier of a perk, read from a unit's parameters on a client.
---@param params Pointer
---@param perk integer
---@return number
function GetPerkModifierForClient(params, perk) end

---Heals the body slots and recomputes the total health.
---@param stats Pointer
---@param vitals Pointer
---@param amount number
function Heal(stats, vitals, amount) end

---Adds the maximum mana times `fraction`, capped at the maximum mana.
---@param vitals Pointer
---@param fraction number
function RegenMp(vitals, fraction) end

---Sets the body slots' maxima from `maxHp` and clamps their current health.
---@param stats Pointer
---@param maxHp number
function DistributeHp(stats, maxHp) end

---Computes the total health from the body slots.
---@param stats Pointer
---@param maxHp number
---@return number
function CollectHp(stats, maxHp) end

---The perk record at `index`, or 0 past the table.
---@param index integer
---@return Pointer
function GetPerkRecord(index) end

---The weapon record at `index`, or 0 past the table.
---@param index integer
---@return Pointer
function GetWeaponRecord(index) end

---The armor record at `index`, or 0 past the table.
---@param index integer
---@return Pointer
function GetArmorRecord(index) end

---The material record at `index`, or 0 past the table.
---@param index integer
---@return Pointer
function GetMaterialRecord(index) end

---The quick-item record at `index`, or 0 past the table.
---@param index integer
---@return Pointer
function GetQuickRecord(index) end

---The spell record at `index`, or 0 past the table.
---@param index integer
---@return Pointer
function GetSpellRecord(index) end

---The rune record at `index`, or 0 past the table.
---@param index integer
---@return Pointer
function GetRuneRecord(index) end

---The number of perk records.
---@return integer
function GetPerksTableSize() end

---The number of weapon records.
---@return integer
function GetWeaponsTableSize() end

---The number of armor records.
---@return integer
function GetArmorsTableSize() end

---The number of material records.
---@return integer
function GetMaterialsTableSize() end

---The number of quick-item records.
---@return integer
function GetQuicksTableSize() end

---The number of spell records.
---@return integer
function GetSpellsTableSize() end

---The number of rune records.
---@return integer
function GetRunesTableSize() end

---The index of a perk record, or -1 if it is not found.
---@param record Pointer
---@return integer
function GetPerkIndex(record) end

---The index of a spell record, or -1 if it is not found.
---@param record Pointer
---@return integer
function GetSpellIndex(record) end

---The index of a rune record, or -1 if it is not found.
---@param record Pointer
---@return integer
function GetRuneIndex(record) end

---The class ID of an object.
---@param object Pointer
---@return integer
function GetClassId(object) end

---The total weight of a unit's equipped items.
---@param unit Pointer
---@return number
function GetItemsWeight(unit) end

---The prototype record of an item.
---@param item Pointer
---@return Pointer
function GetItemShapeEntry(item) end

---The weapon in a slot (0 to 3) of a unit, or 0 if the slot is empty or out of range.
---@param unit Pointer
---@param slot integer
---@return Pointer
function GetWeapon(unit, slot) end

---The armor in a slot (0 to 6) of a unit, or 0 if the slot is empty or out of range.
---@param unit Pointer
---@param slot integer
---@return Pointer
function GetArmor(unit, slot) end

---The quick item in a slot (0 to 3) of a unit, or 0 if the slot is empty or out of range.
---@param unit Pointer
---@param slot integer
---@return Pointer
function GetQuick(unit, slot) end

---The spell in a slot (0 to 7) of a unit, or 0 if the slot is empty or out of range.
---@param unit Pointer
---@param slot integer
---@return Pointer
function GetSpell(unit, slot) end

---Writes a line to the console.
---@param text string
function ConsoleWriteLine(text) end

---Reads a variable of the unit's player. Names are case-sensitive.
---@param unit Pointer
---@param name string
---@return number? value `nil` if the unit has no player or the variable does not exist.
function GetGSVar(unit, name) end

---Writes a variable of the unit's player. Names are case-sensitive.
---@param unit Pointer
---@param name string
---@param value number
function SetGSVar(unit, name, value) end

---Tests whether the unit belongs to the hosting player.
---@param unit Pointer
---@return boolean
function UnitIsHost(unit) end

---Prints every variable of the unit's player to the console.
---@param unit Pointer
function TempDumpVars(unit) end

---Reads a 32-bit float.
---@param address Pointer
---@return number
function FloatFromMem(address) end

---Reads a 64-bit float.
---@param address Pointer
---@return number
function DoubleFromMem(address) end

---Reads a signed 8-bit integer.
---@param address Pointer
---@return integer
function Int8FromMem(address) end

---Reads a signed 16-bit integer.
---@param address Pointer
---@return integer
function Int16FromMem(address) end

---Reads a signed 32-bit integer.
---@param address Pointer
---@return integer
function Int32FromMem(address) end

---Reads an unsigned 8-bit integer.
---@param address Pointer
---@return integer
function UInt8FromMem(address) end

---Reads an unsigned 16-bit integer.
---@param address Pointer
---@return integer
function UInt16FromMem(address) end

---Reads a 32-bit integer. The result is the signed reading, as for `Int32FromMem`.
---@param address Pointer
---@return integer
function UInt32FromMem(address) end

---Writes a 32-bit float.
---@param address Pointer
---@param value number
function FloatToMem(address, value) end

---Writes a 64-bit float.
---@param address Pointer
---@param value number
function DoubleToMem(address, value) end

---Writes the low 8 bits of an integer.
---@param address Pointer
---@param value integer
function Int8ToMem(address, value) end

---Writes the low 16 bits of an integer.
---@param address Pointer
---@param value integer
function Int16ToMem(address, value) end

---Writes a 32-bit integer.
---@param address Pointer
---@param value integer
function Int32ToMem(address, value) end

---Writes the low 8 bits of an integer.
---@param address Pointer
---@param value integer
function UInt8ToMem(address, value) end

---Writes the low 16 bits of an integer.
---@param address Pointer
---@param value integer
function UInt16ToMem(address, value) end

---Writes the 32 bits of an integer, converted as `Int32ToMem` converts it.
---@param address Pointer
---@param value integer
function UInt32ToMem(address, value) end

---Reads the string pointer stored at `address`, then the NUL-terminated text it points to.
---@param address Pointer
---@return string? text `nil` if the stored pointer is null.
function StrFromMem(address) end

-- Section: Callbacks supplied by the mod
-- Global functions that the main script defines, each called by the patch group named in its description. Every
-- callback is optional: where the script defines none, the game's own code runs. A callback that returns nothing
-- updates the structures it receives through their addresses.

---Group `LuaGetPerkPrice`. The experience cost of a skill step.
---@param from integer
---@param to integer
---@return integer cost
function GetSkillPriceDiff(from, to) end

---Group `LuaGetPerkPrice`. The experience cost of a perk.
---@param stats Pointer
---@param perkRecord Pointer
---@return integer cost
function GetPerkPrice(stats, perkRecord) end

---Group `LuaGetModifiedDamage`. The damage a unit takes on the server, without the game's floor of 1.
---@param stats Pointer
---@param unit Pointer
---@param damage number
---@return number damage
function GetModifiedDamageOnServer(stats, unit, damage) end

---Group `LuaGetModifiedDamage`. The damage a unit takes on a client, without the game's floor of 1.
---@param stats Pointer
---@param unit Pointer
---@param damage number
---@return number damage
function GetModifiedDamageOnClient(stats, unit, damage) end

---Group `LuaRecalcUnitStats`. The base health of a unit.
---@param stats Pointer
---@return number health
function CalcBaseHealth(stats) end

---Group `LuaRecalcUnitStats`. The base mana of a unit.
---@param stats Pointer
---@return number mana
function CalcBaseMana(stats) end

---Group `LuaRecalcUnitStats`. The health of a unit after its effects.
---@param stats Pointer
---@param unit Pointer
---@param hp number
---@return number health
function GetModifiedHealth(stats, unit, hp) end

---Group `LuaRecalcUnitStats`. Recomputes a unit's statistics; `effectMaxima` points to 40 floats.
---
---Differs from addon.dll 0.12.4.2: sets the alive bit in the stats' flags (the original sets it in the build flags);
---an intelligence raise marks the unit changed (the original does not); a strength raise widens the unit by 0.2, as the
---game does (the original narrows it by mistake); a unit without vitals is skipped (the original crashes).
---@param stats Pointer
---@param vitals Pointer
---@param unit Pointer
---@param effectMaxima Pointer
function RecalcUnitStats(stats, vitals, unit, effectMaxima) end

---Group `LuaRecalcUnitStats`. Resets a hero's statistics to their base values.
---
---Differs from addon.dll 0.12.4.2: sets the alive bit in the stats' flags (the original sets it in the build flags);
---an intelligence raise marks the unit changed (the original does not); a strength raise widens the unit by 0.2, as the
---game does (the original narrows it by mistake); a unit without vitals is skipped (the original crashes).
---@param stats Pointer
---@param newLoad number
function RecalcInternalHeroStats(stats, newLoad) end

---Group `LuaRegenerateHpAndMp`. Regenerates health and mana.
---@param vitals Pointer
---@param stats Pointer
---@param unit Pointer
---@param inCombat boolean
---@return boolean changed Whether something changed.
function RegenerateHpAndMp(vitals, stats, unit, inCombat) end

---Group `LuaCalcRunningManaCost`. The mana a running unit pays; subtract it from the mana in the callback.
---@param stats Pointer
---@param unit Pointer
---@param vitals Pointer
function CalcRunningManaCost(stats, unit, vitals) end

---Group `LuaCalcAttackParams`. Computes an attack's parameters on the server; `effectMaxima` points to 40 floats.
---
---Differs from addon.dll 0.12.4.2: a hit from another player's unit is ignored unless SpellAddon's
---`enablekillsinmulti` is on, as without the group (the original lets players' units always hurt each other).
---@param stats Pointer
---@param unit Pointer
---@param params Pointer
---@param effectMaxima Pointer
function CalcAttackParamsOnServer(stats, unit, params, effectMaxima) end

---Group `LuaCalcAttackParams`. Computes an attack's parameters on a client; `effectMaxima` points to 40 floats.
---
---Differs from addon.dll 0.12.4.2: a hit from another player's unit is ignored unless SpellAddon's
---`enablekillsinmulti` is on, as without the group (the original lets players' units always hurt each other).
---@param unitParams Pointer
---@param unit Pointer
---@param params Pointer
---@param effectMaxima Pointer
function CalcAttackParamsOnClient(unitParams, unit, params, effectMaxima) end

---Group `LuaGetExpModifiers`. The mana factor of a unit on the server.
---@param stats Pointer
---@return number factor
function GetManaBonusOnServer(stats) end

---Group `LuaGetExpModifiers`. The mana factor of a unit on a client.
---@param params Pointer
---@return number factor
function GetManaBonusOnClient(params) end

---Group `LuaGetExpModifiers`. The experience multiplier of a unit on the server.
---@param stats Pointer
---@return number multiplier
function GetExpMultiplierOnServer(stats) end

---Group `LuaGetExpModifiers`. The experience multiplier of a unit on a client.
---
---Differs from addon.dll 0.12.4.2: experience for a unit the client does not have shows unchanged, without a call (the
---original passes the script a pointer into nothing).
---@param params Pointer
---@return number multiplier
function GetExpMultiplierOnClient(params) end

---Group `LuaRecalcShopsPriceMultipliers`. Sets the price multipliers of the shops; `multipliers` points to nine floats
---per shop.
---@param multipliers Pointer
---@param isLmp boolean
function RecalcShopsPriceMultipliers(multipliers, isLmp) end

---Group `LuaLmpExpDistribution`. The share of a kill's experience for each player.
---
---Differs from addon.dll 0.12.4.2: no value is left on the FPU stack per call (the original's leftovers corrupt later
---results).
---@param exp number
---@param players integer
---@return number share
function DistributeLmpExp(exp, players) end
