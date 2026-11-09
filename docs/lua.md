# Lua

With `[Addon] EnableLuaScripts` set in the mod's ai.reg, `addon.dll` runs the mod's Lua script and calls it from nine
patch groups. The other ai.reg settings are under [addon and loader](addon.md#gameplay-aireg).

The last column of each table gives eix's difference from addon.dll 0.12.4.2, with what the original does in
parentheses. :white_check_mark: means the original's behavior;
:white_check_mark::white_check_mark: marks a difference that only fixes a defect of the original.

## Host

Set `[Addon] EnableLuaScripts` nonzero and supply `LuaMainScriptPath` (default `main.lua`). The host is LuaJIT 2.1,
with `bit` and `jit` loaded as globals; load `ffi` with `require("ffi")`. Module search paths include the mod root
and its `lua` folder. Define all callbacks that enabled gameplay paths call.
A reported Lua error shows a message, logs its traceback, and exits.
Pointers are passed as signed 32-bit integers; raw memory operations require valid game memory.

| Behaviour | eix |
|---|---|
| LuaJIT build | a newer LuaJIT 2.1 (the original: 2.1.0-beta3) |
| A Lua error | the message is shown as text (the original treats it as a printf format, so `%d` is consumed); the box is titled `addon.dll`; the message and its traceback also go to addon.log (the original logs nothing) |
| Lua state at exit | not closed at exit, since a fatal error box exits from inside a Lua call |

## Callbacks supplied by the mod

Void callbacks update the supplied structures. Other callbacks return the result shown.

| Group | Callback | Result | eix |
|---|---|---|---|
| `LuaGetPerkPrice` | `GetSkillPriceDiff(from, to)`, `GetPerkPrice(stats, perkRecord)` | Integer experience cost | :white_check_mark: |
| `LuaGetModifiedDamage` | `GetModifiedDamageOnServer(stats, unit, damage)`, `GetModifiedDamageOnClient(stats, unit, damage)` | Damage, without the game's floor of 1 | :white_check_mark: |
| `LuaRecalcUnitStats` | `CalcBaseHealth(stats)`, `CalcBaseMana(stats)`, `GetModifiedHealth(stats, unit, hp)` | Health/mana | :white_check_mark: |
| `LuaRecalcUnitStats` | `RecalcUnitStats(stats, vitals, unit, effectMaxima)`, `RecalcInternalHeroStats(stats, newLoad)` | Void | :white_check_mark::white_check_mark: sets the alive bit in the stats' flags (the original sets it in the build flags); an intelligence raise marks the unit changed (the original does not); a strength raise widens the unit by 0.2, as the game does (the original narrows it by mistake); a unit without vitals is skipped (the original crashes) |
| `LuaRegenerateHpAndMp` | `RegenerateHpAndMp(vitals, stats, unit, inCombat)` | Boolean: something changed | :white_check_mark: |
| `LuaCalcRunningManaCost` | `CalcRunningManaCost(stats, unit, vitals)` | Void; subtract mana in the callback | :white_check_mark: |
| `LuaCalcAttackParams` | `CalcAttackParamsOnServer(stats, unit, params, effectMaxima)`, `CalcAttackParamsOnClient(unitParams, unit, params, effectMaxima)` | Void; effectMaxima points to 40 floats | :white_check_mark::white_check_mark: a hit from another player's unit is ignored unless SpellAddon's `enablekillsinmulti` is on, as without the group (the original lets players' units always hurt each other) |
| `LuaGetExpModifiers` | `GetManaBonusOnServer(stats)`, `GetManaBonusOnClient(params)` | Mana factor | :white_check_mark: |
| `LuaGetExpModifiers` | `GetExpMultiplierOnServer(stats)`, `GetExpMultiplierOnClient(params)` | Experience multiplier | :white_check_mark::white_check_mark: experience for a unit the client does not have shows unchanged, without a call (the original passes the script a pointer into nothing) |
| `LuaRecalcShopsPriceMultipliers` | `RecalcShopsPriceMultipliers(multipliers, isLmp)` | Void; nine floats per shop | :white_check_mark: |
| `LuaLmpExpDistribution` | `DistributeLmpExp(exp, players)` | Each player's experience share | :white_check_mark::white_check_mark: no value left on the FPU stack per call (the original's leftovers corrupt later results) |

## Functions supplied by eix

Record and slot indices are zero-based. Use indices within the corresponding table/slot bounds.

| Function | Result/effect |
|---|---|
| `Round(value)`, `IDiv(a, b)`, `IMod(a, b)` | Half-away-from-zero rounding; signed integer quotient/remainder; division by zero or overflow ends the game |
| `GetEffectValue(unit, effect)` | Effect strength |
| `GetPerkModifierForServer(stats, perk)`, `GetPerkModifierForClient(params, perk)` | Perk modifier |
| `Heal(stats, vitals, amount)` | Heal body slots and recompute total health |
| `RegenMp(vitals, fraction)` | Add maximum mana times fraction, capped at maximum mana |
| `DistributeHp(stats, maxHp)`, `CollectHp(stats, maxHp)` | Set body-slot maxima from maxHp and clamp current health; compute total health from the slots |
| `GetPerkRecord(index)`, `GetWeaponRecord(index)`, `GetArmorRecord(index)` | Record pointer |
| `GetMaterialRecord(index)`, `GetQuickRecord(index)`, `GetSpellRecord(index)`, `GetRuneRecord(index)` | Record pointer |
| `GetPerksTableSize()`, `GetWeaponsTableSize()`, `GetArmorsTableSize()` | Record count |
| `GetMaterialsTableSize()`, `GetQuicksTableSize()`, `GetSpellsTableSize()`, `GetRunesTableSize()` | Record count |
| `GetPerkIndex(record)`, `GetSpellIndex(record)`, `GetRuneIndex(record)` | Index, or -1 if not found |
| `GetClassId(object)` | Object class ID |
| `GetItemsWeight(unit)`, `GetItemShapeEntry(item)` | Total equipped weight; item prototype pointer |
| `GetWeapon(unit, slot)`, `GetArmor(unit, slot)`, `GetQuick(unit, slot)`, `GetSpell(unit, slot)` | Equipped item pointer, or 0; slot counts are 4, 7, 4 and 8 respectively |
| `ConsoleWriteLine(text)` | Write a console line |
| `GetGSVar(unit, name)`, `SetGSVar(unit, name, value)` | Read/write a variable of the unit's player; names are case-sensitive; missing reads return nil |
| `UnitIsHost(unit)`, `TempDumpVars(unit)` | Test host ownership; print variables |
| `FloatFromMem(address)`, `DoubleFromMem(address)` | Read a 32/64-bit float |
| `Int8FromMem(address)`, `Int16FromMem(address)`, `Int32FromMem(address)` | Read a signed integer |
| `UInt8FromMem(address)`, `UInt16FromMem(address)`, `UInt32FromMem(address)` | Read an integer; `UInt32FromMem` returns the signed 32-bit interpretation |
| `FloatToMem(address, value)`, `DoubleToMem(address, value)` | Write a float |
| `Int8ToMem`, `Int16ToMem`, `Int32ToMem`, `UInt8ToMem`, `UInt16ToMem`, `UInt32ToMem` | `(address, value)` writes integer bits; 8/16-bit writes keep the low bits |
| `StrFromMem(address)` | Read the string pointer stored at address, then its NUL-terminated text; null returns nil |
