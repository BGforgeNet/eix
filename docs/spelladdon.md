# SpellAddon options

The built-in SpellAddon supports `SPELLADDON.INI`. It uses the mod-folder file first, then the game-folder file.
It reads one file, without merging them. Without either file, SpellAddon is off. The built-in version takes
precedence over separate `SpellAddon*.asi` files.

Loader and INI compatibility details: [addon start-up behavior](addon.md#loader-and-start-up-behavior).

The last column of each feature table gives eix's difference from SpellAddon 20.6 (SpellAddonXa.asi), with what the
original does in parentheses. `same` means the original's behavior;
`fixed:` marks a difference that only fixes a defect of the original.

## Configuration

All feature switches default to **0** (off), including when an INI file exists. Parameter defaults below apply only
when their feature uses them; they are not the sample values from older guides. Restart the game after edits.
An INI file also enables the spell-construction diagnostic correction, independently of its feature switches.

Section and key names are case-insensitive. Spaces around names are ignored; `disablepausekey` works without the
historical trailing space. Duplicate keys use the first value and produce a warning. Canonical keys take precedence
over aliases: `[gameplay] avrgarmors` aliases `avgarmors`; `[gameplay] showallquestsinmulti`, `spellsaimod` and
`spellsarealwaysconstructable` alias their `[speed]` counterparts.

The tables give **active values**. Other values make no change unless marked skipped. A skipped value is logged
and left out, and every other feature still applies. Switches retain a signed byte: use the listed values within
-128..127; larger values wrap and warn. `1+` below means 1..127. Byte parameters retain the low byte (0..255), and
other values warn; 32-bit parameters retain 32 bits. Floating-point parameters accept decimal text with a sign and
an exponent; leading zeros in the exponent do not change its value. Use a decimal point.
`runconst`, `runconstex`, `pow` and `expforint` read only their first 21
characters, take a leading `-`, ignore any other character that is not a digit, point or comma, and also accept a
decimal comma (the original ignores the sign too). Other numeric text holds at most 31 bytes; font text holds at
most 47 bytes.
Parameters have no general range validation. Limits stated below describe their use, not an extra validator.

Features apply in a fixed order, independent of INI order. Where two features change the same code, the later one
builds on the earlier, or the earlier is skipped with a warning when the later replaces all it does; the tables call
out those interactions. A conflicting or unexpected game patch causes the affected feature to be skipped and
logged. If writing the merged patches fails, no feature applies. See the [addon options](addon.md) for the other
built-in settings.

## Combat - `[gameplay]`

| Key | Active values and effect | eix |
|---|---|---|
| `archersdefence` | 1+: bows and crossbows retain defence. | same |
| `unbreakablebodyparts` | 1+: prevent limb breaking; also use `bodypartbreakmodifier` for the limb-damage floor. | same |
| `enablekillsinmulti` | 1: remove player damage immunity so players can hurt other players. | fixed: god mode still holds (the original drops it too) |
| `rangedweaponbackstabs` | 1: allow ranged backstabs. | fixed: with `newrangedweapontypes`, its change to how orders are carried out stays (the original's `newrangedweapontypes` replaces it) |
| `backstabsforall` | 1: allow units without a player to backstab. | same |
| `newattackdefence` | 1, 2: physical damage after armour uses attack minus defence divided by `divmod`, plus `submod` and the speed term; 1 also includes the distance/weight term. 3, 4: use `max((attack - defence) * divmod + 1, submod)`; backstabs ignore defence. 4 also disables misses. | same |
| `damageboost` | 1: scale weapon damage by strength/dexterity above 25 and spell effects by intelligence above 25, dividing by `str`, `dex`, `int`. 2: multiply by those parameters instead. 3: bypass the normal effect scaling and its client display adjustment. 4: boost after weapon-effect scaling and at cast completion, using multiplication. 5: boost after scaling and mark casts so only their spell effects get the intelligence boost. | same |
| `changeweightrangebonuses` | 1+: use `weight` and `range` in hit calculations; 2 also bypasses the conditional range test. | same |
| `strengthstats` | 1: set strength's health, damage and carrying-capacity coefficients with `strhp`, `strdmg`, `strweight`. | same |
| `allowheroesarmors` | 1: retain heroes' natural armour. | same |
| `newrangedweapontypes` | 1: weapon types 11 and 15 count as ranged, alongside bows (5) and crossbows (6), throughout combat checks. | same |
| `randomlessaimedhits` | 1-4: choose a vulnerable body part using the last hit. 5-9: estimate damage from the weapon's damage range and target resistance. 2, 6, 7, 9 compare the target's reach minus the attacker's reach with `rangeddistance`; other modes use attack reach. Highest score wins; `bodypart` is the fallback. 10 is skipped. | 10 skipped (calls code the original never wrote); 5 and 8 keep the game's minimum of distance and reach |
| `increasinghitchance` | 1: after more than `tohitcheck` misses, add `misses * tohitmul - tohitsub` to the hit threshold. The defender holds the counter; a hit resets it. | fixed: with `newattackdefence` 1, 2 or 4, both apply (the original undoes part of `newattackdefence`) |
| `runawaydistcontrol` | 1: attack reach becomes `weapon range * hitdistmul + hitdistadd`. | fixed: keeps fractional `hitdistmul` and `hitdistadd` (the original truncates them to integers) |
| `nightpower` | 2: multiply attack and defence by `nightmeleeadd - P * nightmeleemul + vision * P * nightmeleemul / 100`, where P is night level (0..1). 1 is skipped. | 1 skipped (every attack and defence value becomes garbage); fixed: with `damageboost=3`, the unit panel keeps that feature's change (the original drops it) |
| `duels` | 1: suppress diplomacy reactions to damage when player 0's `und<victim id>` variable rounds to nonzero. | same |
| `acidattack` | 1, 2: unfinished acid-effect variants; neither changes the visible effect. | same |
| `armorspell` | 1 is skipped. | 1 skipped (corrupts the stats refresh) |

## Progression and movement

| Section | Key | Active values and effect | eix |
|---|---|---|---|
| `skills` | `extendedskills` | 1: replace the non-stealing skill cap with `[skills] skills`. | fixed: uses `[skills] skills` as its cap (the original uses `[stealing] stealing`) |
| `stealing` | `extendedstealing` | 1: replace the Sleight of Hand cap with `[stealing] stealing`. | fixed: uses `[stealing] stealing` as its cap (the original uses `[skills] skills`) |
| `rebalanceengine` | `rewriteskillscounting` | 1: next skill level costs `pow ^ level * mul + add`. | fixed: `pow` without a decimal point reads as a whole number (the original divides by ten for each digit) |
| `rebalanceengine` | `rewriteherolevelcounting` | 1: level is `round(mullvla * log2(exp / divlvla + addlvla) + addlvlb)` on server and client. 2 is skipped. | 2 skipped (returns a stale level) |
| `rebalanceengine` | `rewriterunabilitytime` | 1: use `runconst` as the running stamina coefficient; also supply `runconstex` for `newrunsystem=2`. | fixed: `runconst` and `runconstex` without a decimal point read as whole numbers (the original divides by ten for each digit) |
| `rebalanceengine` | `newrunsystem` | 1: drain a fixed amount instead of maximum stamina times the coefficient. 2: drain maximum stamina times the coefficient, plus `runconstex`. `rewriterunabilitytime` controls the coefficient; otherwise the game uses 1/150. | fixed: `runconstex` without a decimal point reads as a whole number (the original divides by ten for each digit) |
| `rebalanceengine` | `useonlymindamage` | 1: use minimum weapon/unit damage instead of rolling within its range. | same |
| `rebalanceengine` | `backstabvisualbase` | 1: show `bsbase` as the base backstab percentage; display only. | same |
| `rebalanceengine` | `difficultyreset` | 1: apply difficulty scaling to all units, including heroes. 2: also add difficulty-table values instead of multiplying by them. | same |
| `rebalanceengine` | `meleesum_low` | 1: non-hero server attack/defence add the first skill and dexterity minus 25. Skipped unless `meleesum_1` or `meleesum_2` is exactly 1; `meleesum_1=1` satisfies this although that feature is itself skipped. | 1 skipped without `meleesum_1` or `meleesum_2` (the original's game hangs at the first attack) |
| `rebalanceengine` | `meleesum_1` | 1 is skipped. | 1 skipped (the original crashes at the first attack) |
| `rebalanceengine` | `meleesum_2` | 1: client attack/defence add the first skill and dexterity minus 25, retaining the low byte: the client side of `meleesum_low`'s sum. | same |
| `animationsspeed` | `rewriteactionsspeed` | 1: set six animation factors from this section's parameters; also adjust client animation timing. | same |
| `gameplay` | `changeexpforint` | 1: use `expforint` as the experience multiplier per intelligence point. | fixed: `expforint` without a decimal point reads as a whole number (the original divides by ten for each digit) |
| `gameplay` | `heroalwayscanrun` | 1: remove the load-based restriction on running. | same |
| `gameplay` | `automaticruncontrol` | 1: stop running at zero stamina. | same |
| `gameplay` | `speedboost` | 1: multiply movement speed by `((action counter + speedadd) / 15) ^ speedbase`. | same |
| `gameplay` | `actionspatch` | 1: actions become `dexterity * actionsmul + actionsadd`. | same |
| `gameplay` | `actionsdexteritymovespeed` | 1+: actions and dexterity contribute to movement speed through `actmovemul` and `dexmovemul`. | same |
| `gameplay` | `cleveranimationsspeedup` | 1: set casting animation speed from cast duration and send animation speed in unit network state; also adjust client animation timing. Peers need compatible settings. | same |
| `gameplay` | `nhits` | 1: use a fixed count of five hits for the special multi-hit animations. | same |
| `gameplay` | `diffchange` | 1: ignore clicks on the difficulty spinner in game options. | same |
| `gameplay` | `newdifficultylevels` | 1: add normal between easy and hard in the new-game choice, default to normal, and use a difficulty slider in options. | fixed: the box grows for the new row and the explanation moves below it; a click on the hard box selects hard (the original's overlaps and ignores the click) |

## Items, shops and camp - `[gameplay]`

| Key | Active values and effect | eix |
|---|---|---|
| `unlockmoney` | 1: keep money unencoded in memory. | same |
| `saveall` | 1+: use the game's persistent-zone save/load handling for every zone. Value 32 also participates in `extendedmerc`'s draw-range extension. | same |
| `extendedmerc` | 1+: set the briefing mercenary limit to the low byte of `maxcountofmerc`. With value 16+, `saveall=32` and `maxcountofmerc>50`, extend the client draw range from 100 to 4096. | same |
| `shopschanges` | 1+: change trader templates' price/count fields; the final template uses price/count factors 1.0, 0.2 and 0.1. `shopsnewnormalchanges=1` supplies `jigran` for its shared factor. | same |
| `shopsnewnormalchanges` | 1: set stock ranges for weapons, armour, wands, potions, prototypes and runes, plus four traders' sell-price factors and `jigran` for `shopschanges`. | same |
| `copystatscut` | 1: `CopyStats` skips portions of the stats copy, retaining the recipient's racial traits rather than making a complete clone. | same |
| `liftchanges` | 1: unfinished carry-weight check; no game-state change. | same |
| `liftdebuffs` | 1: packed stats are scaled by `1.01 - q / (q + liftkey)`, q = load / maximum load, while load is nonzero. | same |
| `encumbrancemovespeed` | 1: subtract `r^3 * encumbrancemovemul` from movement factor, r = load / (load + maximum load). With `speedslowremaster=2`, also apply its Speed/Slow contribution, `defaultspeedmul`, `minimumspeedmul`, and the overload factor. | same |
| `encumbranceattackspeed` | 1: load slows attack animations using `(animation factor - r^3) * encumbranceattackmul`. 2: load reduces the stats' attack rate using `speed factor - r^3 * encumbranceattackmul`. r is as above. | fixed: 1 slows `rewriteactionsspeed`'s attack factor and 2 `actionspatch`'s actions (the original uses the game's values there, dropping those features' changes) |
| `improvedstealing` | 1: separate theft from corpse loot. A living hero retains equipped armour/weapons; a previously robbed hero's corpse yields those, without repeating gold or stolen items. An already robbed living unit yields nothing. | same |
| `dmgtypes` | 1: list `dmgtypescount` damage types in camp panels. | same |
| `avgarmors` | 1: show average rather than maximum armour, including Endurance's bonus. Requires `newperks>=1`; otherwise skipped. The divisor follows `dmgtypescount` when `dmgtypes` is active. | also accepts `[gameplay] avrgarmors` when absent (the original ignores it); 1 skipped without `newperks` (the original crashes entering the camp) |
| `itemslightchanges` | 1: replace eight item-highlight RGBA colours with `light00` through `light31`. | same |
| `heroscanhideweapons` | 1: press the selected weapon's key again, or click its selected slot, to sheathe it. | same |
| `camp` | 1+: `SendStringEvent(0, "constr")` enters camp. 2+: the camera-track key (Home by default) also enters when player 0's `Camp` is nonzero, then sets `Camp=2`. With `newoptions>=1`, modes 2-12 omit Home; `newoptions=1` adds the camp binding and ability button. Each of 3, 4 and 13+ adds only its own extra: 3 tracks/removes quest items; 4 limits quick quest-item slots by `QuIs`; 13+ restore zone music on return and retain Home. | fixed: the Home key without player 0 does nothing (the original runs into the next action); value 3's quest-item handling keeps the game's own steps; NewStringEvents still handles gameover |
| `newoptions` | 1: add camp (action 60) and kick bindings (61, 62). Camp's action requires `camp>=2`; actions 60/61 otherwise end safely, with no kick behavior added. | fixed: actions 60 and 61 end the battle screen's handling (the original jumps into garbage), unless `camp` defines 60 |
| `newperks` | 1+: add Endurance (armour against all seven damage types) and Ghostliness (less visibility), and separate the magic-school skills. 2 also changes the school field used by spell descriptions. | fixed: Ghostliness lowers loudness as well as visibility, and visibility stops at 0 (the original's loudness change is overwritten, and visibility can go below 0) |

For `camp=3`, quest-item creation sets player 0's `q<item id>=2`; taking the item sets it to 0, except while camp
entry strips items. Set `QuIs` to the active quest-item count when using `camp=4`.

## Spells - `[gameplay]` unless specified

| Key | Active values and effect | eix |
|---|---|---|
| `possessioncharmlink` | 1: allow casting Possession, Charm and Link. | same |
| `possessionpatch` | 1: Possession retains the caster's sight and registers both caster and target for script visibility. | same |
| `batterycharge` | 1+: reject spells containing modifier codes `ic` or `it` when placing them in wands. Invalid rune-table references are skipped. | fixed: a modifier past the rune table is skipped (the original crashes) |
| `nonstopmanaregen` | 1+: continue mana regeneration through the state that normally stops it. | same |
| `npcsmana` | 1: charge NPCs mana on cast completion. 2: require enough mana before casting. 3: both. 4: make both exemptions depend on AI class 16 instead of player ownership. | same |
| `splashdamageonair` | 1: area spells can hit flying units. 2: only Invoke Lightning gains this ability, including AI targeting. | same |
| `newspells` | 1-3: extend spell dispatch, enable Ensorcell (42), Inspiration (43), script spells (44-48), and the Fireball-based spell 49. 1 also marks spell 49's caster; 1/2 load the autorun mob. 3 adds lasting-effect cases and Ensorcell's damage type. See script variables below. New spell records/scripts must come from the mod. | fixed: the original's crashes left out: Charm on a spawned wolf, the effect types that crash the effect loop, Charm and Healing with a broken stack; `legendaryspellsystem` and `changefont` keep their data |
| `lastspellsdamagetype` | 1: use `astralbeam`, `destroyray`, `lightningbeam`, `firearrow` as damage-type IDs. | fixed: damage types whose byte displacement leaves the damage record are refused with a warning |
| `[speed] spellsaimod` | 1: extend the AI offensive/friendly spell table to 54 types. | also accepts `[gameplay] spellsaimod` when absent (the original ignores it) |
| `[speed] spellsarealwaysconstructable` | 1: bypass level checks when assembling spells or giving them to heroes. | also accepts `[gameplay] spellsarealwaysconstructable` when absent (the original ignores it) |
| `offensivespells` | 1: mark Stench and Shrink as friendly in missile hostility checks. | same |
| `inspirationintelligence` | 1: item Inspiration waits until stamina is below maximum, as Healing waits for missing health; limit repeated item casts within the tick. 2/3 are skipped. | 2 and 3 skipped (Healing items crash) |
| `teleportboost` | 1: teleport duration becomes `round(900 / (effect strength / 2 + 22)) + 2`. 2: use delay 8 instead of 30. 3: use delay 1. | same |
| `shrinkenlargerebalance` | 1, 2: Shrink and Enlarge no longer change unit size. 1 also changes the scale multiplier used by the size calculation. | same |
| `hidefireworks` | 1: omit Fireworks' drawing setup. | same |
| `wallsfrequency` | 1: wall spells deal damage every `frequencyvalue` ticks. | same |
| `wallspercentage` | 1: per-type wall fractions are `percentagevalue * period / 150`. 2: use each configured fraction directly. | same |
| `controlmagic` | 1: Stun-like spells, Weaken and Slow meet target resistance; the latter two use `weaknessmultiplier`/`slownessmultiplier`. 2, 5, 7: add the 22 configured spell bonuses. 3: add bonuses only for spell switches with odd values. Duration/intelligence formulas from older guides do not run. 4/6 are skipped. | 4 and 6 skipped (corrupt durations) |
| `effectredraws` | 1: show all configured `b<spell>` bonuses as `value+bonus` in item descriptions, and their sum in the HUD, independently of the `controlmagic` switches. | same |
| `stenchremaster` | 1: Stench increases visibility, loudness and two further perception stats by its strength times `stenchmul`. | same |
| `regenerationremaster` | 1: add Regeneration strength times `regenerationmul` to natural regeneration. | same |
| `speedslowremaster` | 1, 2: multiply Speed minus Slow by `speedslowmul`. 2 also affects movement when `encumbrancemovespeed=1`. | same |
| `strengthweakrebalance` | 1: Strengthen minus Weaken, times `strengthweakmul`, scales strength. | same |
| `linkremaster` | 1: every 32 ticks, transfer stamina from the proportionally fuller linked unit towards equal fill ratios, limited by spell strength. 2/3 are skipped. | 2 and 3 skipped (the original crashes) |
| `allowcastsonbriefings` | 1: allow casting in briefing zones. | same |
| `fakesrcrune` | 1: modifier code `f` starts a spell from its target point instead of its caster. | same |
| `filtersarealwaysusing` | 1, 2: apply target filters to every spell. | same |
| `tornadochanges` | 1+: use `tordmg` for tornado damage. 2+: also remove the unit exclusion, so it can hurt every unit. | same |
| `changesounds` | 1: use `fireboom` as the Fireball volume factor. | same |
| `vampirism` | 1 is skipped. | 1 skipped (the original runs unfinished code) |
| `maxmanatrans` | 1 is skipped. Do not use it for the advertised path-query interface. | 1 skipped (breaks every spell type but one) |
| `fhrunetrans` | 1 is skipped. | 1 skipped (the original crashes and stops every later feature) |

## AI and perception - `[gameplay]`

| Key | Active values and effect | eix |
|---|---|---|
| `rewritenpcvisionarc` | 1+: set NPC sight-arc value to `visionarc`. | skipped with a warning when `removenpcsblock` is 1 or 2, which removes the value it sets (the original writes it unused) |
| `visionfix` | 1: bypass two lost-sight exits; with `aggrmod>5`, bypass only the first shared exit. | same |
| `aggrmod` | 1: record lost-sight state, without changing the loss calculation. 2: distance-dependent random loss using `losehardcore`. 4/5: distance-dependent loss based on clock ticks, `losefactor1/2`. 6/7: per-viewer aggression counter using `losehardcore`, `losefactor1/2/3`, `difffactor`, `maxadd`. `follow` affects all these modes; `loseallyfast` affects 6/7. 3 is skipped; 6/7 omit the faulty `improve` alarm hook and warn if requested. | 3 skipped (corrupts the AI's state); 6 and 7 with `improve` nonzero leave out the alarm hook (it acts on the wrong unit), with a warning; 2, 4-7 handle a zero divisor |
| `spellsprioritys` | 1: replace seven AI target-rating constants with the priority parameters below. | same |
| `otherspellcastingai` | 1: permit area spells even with allies nearby. | same |
| `npcsspellcastingtactics` | 1: NPCs lacking stamina switch to weapons and return to spells once stamina recovers. | same |
| `removenpcsblock` | 1, 2: remove NPC-specific stats, AI and naming branches. 1 also removes the NPC difficulty-scaling exception. | same |
| `everyoneishero` | 1: broaden the shared hero-ID test to nonnegative IDs below two billion. 2: selected game systems treat all units as heroes. 3: as 2 but retain difficulty scaling for enemy heroes. | same |
| `allysseeseachothers` | 1: sight treats all units as player-owned. 2/3: also use the player branch during visibility updates. 4: sight uses hero IDs from one billion to below two billion as the ownership test. | fixed: 4: looting works with or without `improvedstealing` (the original crashes without it at the first scripted loot) |
| `newplayersafetyconditions` | 1: use `safetymuldist` in party-safety distance checks and `maxvisibilitymul` for the visibility factor. | same |
| `fearpatch` | 1: use `fearsafetydist` for retreat distance and `legsdamagedmul` for the leg-damage cutoff. | same |
| `stupidenemies` | 1: witnessing a murder no longer invokes the diplomacy attack reaction. | same |
| `hyperreactions` | 1: react to seen units/corpses without the own-side-only restriction. | same |
| `hearingreactionspatch` | 1: fix the unit/target used in reactions to sounds. 2: also offset and scatter the destination with `runtosoundsmul` and `randsoundfactor`. | same |
| `visioninweather` | 1: use `visionweathermul` in bad weather. | same |
| `nightvisionincaves` | 1+: dark zones always use night vision, without daytime growth. | same |
| `nightpowervisionpatch` | 1: ramp night level between `visionevening`/`visionnight` and `visionmorning`/`visionday`, with slope `visionmulpowbetween`. | same |
| `newbigtables` | 1 is skipped. | 1 skipped (with a rebuilt AI table, spell types from 39 on jump through code bytes) |
| `allvisible` | 1 is skipped. | 1 skipped (corrupts the options load) |
| `sizerebalance` | 1-3 are skipped. | 1-3 skipped (corrupts the unit) |
| `unitsrunfromdangers` | 1 is skipped. | 1 skipped (passes a point as a unit) |

| Section | Key | Active values and effect | eix |
|---|---|---|---|
| `aicastertable` | `rebuildedaicastertable` | 1: replace the 54 spell AI classes with keys `0` through `53` in this section. | same |
| `aioffendtable` | `rebuildedaioffendtable` | 1: replace the 38 offensive/friendly entries with keys `0` through `37` in this section. No effect while `spellsaimod=1`, whose own 54-entry table the game reads instead. | same |

## Multiplayer and speed

| Section | Key | Active values and effect | eix |
|---|---|---|---|
| `gameplay` | `fullexpforall` | 1: every recipient gets the full experience award; do not divide it among recipients. | same |
| `gameplay` | `manygameexe` | 1: allow multiple game processes. | same |
| `gameplay` | `enablecheatsinmulti` | 1: allow the console's restricted commands in multiplayer. | same |
| `gameplay` | `extendedparams` | 1: character creation uses attribute bounds 5..45 and required sum 20000. 2: use `parammin`, `parammax`, `balance` and bypass the sum check. | fixed: keeps fractional `parammin`, `parammax` and `balance` (the original truncates them to integers) |
| `gameplay` | `longnick` | 1: remove the character-name edit field's length checks. | same |
| `gameplay` | `disableexppenalty` | 1+: add experience directly to totals, bypass pending experience in training/trade, and remove its loss on respawn. | fixed: multiplayer goes on when a leader dies (the original's game stops there) |
| `speed` | `allowmpspeed` | 1+: enable multiplayer speed keys and clock-button speed changes. | same |
| `speed` | `rewritespeed` | 1: clock-button normal/fast periods use `normal` and `fast`. | same |
| `speed` | `rewritespeedkey` | 1: keyboard normal/fast periods use `normalkey` and `fastkey`. | same |
| `speed` | `rewriteotherspeed` | 1: apply both following changes. 2: reset to `normal1` on battle-screen close. 3: initial clock period uses `normal2`/`fast2`. | same |
| `speed` | `disablepause` | 1: clock pause sets period `pause` instead of pausing. | same |
| `speed` | `disablepausekey` | 1: pause key sets period `pausekey` instead of pausing. | same |
| `speed` | `showallquestsinmulti` | 1: bypass the multiplayer restriction on selectable quest zones. | also accepts `[gameplay] showallquestsinmulti` when absent (the original ignores it) |
| `speedrun` | `timervars` | 1-3: publish a timer state, 0 during mission/zone/save loading and 1 when the game interface appears. 1/3 differ in battle-screen paths; 2 uses 3D-interface activation instead of briefing creation. Requires an external timer that reads this interface. | same |

Speed periods are milliseconds per update: smaller means faster. Speed and network-format changes must be compatible
between multiplayer peers. `bigmapspatch` and `cleveranimationsspeedup` change network data handling.

## Interface and world - `[gameplay]` unless specified

| Key | Active values and effect | eix |
|---|---|---|
| `rewriteperssize` | 1: use `perssize` for the hero preview scale. | same |
| `[fonts] changefont` | 1+: use `newfont1` through `newfont4`. 2 also loads the font file `newfont5` from the INI folder for this process. | fixed: with `newspells` on, value 1 keeps the second and third font names (the original's `newspells` overwrites them) |
| `infowindowschanges` | 1: compact the unit-info layout. | same |
| `infowindowremake` | 1: combine attack/defence with hearing on one line and add two panel rows. | same |
| `showtype` | 1: show the movement-type line in unit information. | same |
| `showmanaforeveryone` | 1: show mana bars for non-heroes too. | same |
| `meterscale` | 1: use `meter` for the information ruler scale. | same |
| `allodsinfo` | 1: display the `adesc <allod>` text from `texts.res` in a frame sized by `framex`/`framey`. | same |
| `exploredcolors` | 1: set explored-zone colour with `exred`, `exgreen`, `exblue`. | fixed: keeps fractional `exred`, `exgreen` and `exblue` (the original truncates them to integers) |
| `lightsboost` | 1: replace the light-strength constant with `newlightsvalue`. | same |
| `mainmenuweather` | 1: set weather type 2 at strength 40. 2: skip menu weather. 0 leaves the game's weather unchanged. | same |
| `drawvisiblepatch` | 1: zone entry no longer resets the draw-visible setting. | same |
| `playingnewloadingscreens` | 1: name loading movies after the zone. 2: also use `gz1g` in the special initial-zone movie selection. Requires matching movie assets. | same |
| `notrash` | 1: share the speech resource path instead of requiring separate `s` and `t` folders; also change the client animation-mode timing test. The English/German build's briefing has one speech folder: there only the timing test changes. | same |
| `musictimings` | 1: wait `minmusictime + random % (maxmusictime - minmusictime)` between music tracks. | fixed: with `maxmusictime` not above `minmusictime`, the pause is `minmusictime` (the original divides by zero at the first track change, or waits far too long) |
| `newzonescpecificmusic` | 1: zone `gz12d2` uses music named `gz12d2`. 2: use the alternative music-name field in zones, briefings and camp and shorten music-mode setup. 3: as 2, but retain camp's original field. | same |
| `disabledragon` | 1: disable the quest-variable dragon response and the related flying-insect effect updates. | same |
| `noruinszoneanimation` | 1: omit zone1's built-in introduction script. | same |
| `dontmergegipathzones` | 1: keep Gipat zones separate on the global map. | same |
| `newallodsacks` | 1: add environment flags for zones named `jigran`, `neloya`, `eyn`, `bebesh`, `kvatokh`, `nokks`, `umoir`, `yazes`, `yiul`. | same |
| `newcounters` | 1: narrow the regeneration counter to one byte. | same |
| `nogameoveronleadersdeath` | 1: the leader's death does not end the game; revival must be supplied by scripts. | same |
| `hiddenzonesexits` | 1: subtract player 0's `Exits` from exit count. 2+: replace exit count with `Exits`; absent/zero means no exits. | fixed: counts above the zone's own count are capped with a warning; zero and negative counts still mean no exits |
| `segmentsonexitenter` | 1: cap zone-transition segments at `maxsegments`, add `addsegments`, and use `exitenteractivityradius` for activity radius. | same |
| `bigmapspatch` | 1: change network position scaling for larger maps; all peers need compatible encoding. | same |
| `ultrafixes` | 1: skip network objects without the companion needed to send them. | same |
| `expirementalfxes` | 1: change unit sorting. 2: mask loaded IDs to 16 bits and fix load-loop initialization. 3: combine 1/2. 4: mask IDs, change sorting, bypass the first load-section count check. 5: change sorting and zero load-loop initialization. 6: bypass three load-section checks. 7: bound load-loop iterations. 8: combine 1/7. 9: no change. 10: bound the loop, change unit sorting, add null guards in hostility/sight, and change drawing/removal paths. | fixed: each save load gets its own iteration budget |
| `importantalerts` | 1+: show the starter's configured mod path in a modal box; without it, the fallback box has no useful path. | same |
| `versionalerts` | 1: warn about unknown lowercase `.asi` files in the game folder. Known names: `EI_Plugin.asi`, `mp3dec.asi`, `SpellAddonX.asi`, `SpellAddonXa.asi`. Does not warn about the built-in SpellAddon. | does not warn about the built-in SpellAddon itself |

## Script interfaces

These options change existing commands/functions, including their argument lists. Update the mod's scripts together
with its INI. Defaults are 0. Except where shown otherwise, the active value is 1 and the section is `[gameplay]`.
Numbers used as integer indices, styles or IDs are rounded with the game's floating-point rounding mode.

| Key | Interface and effect | eix |
|---|---|---|
| `[speed] gsgetvar` | `GSGetVar(player, name)` returns difficulty for names beginning `Diff`, and update period when characters 4-7 (zero-based) are `peed`, such as `SetSpeed`. | fixed: reads the variable's name before freeing it |
| `[speed] gssetvar` | `GSSetVar(player, name, value)` also sets update period for names with `peed` at characters 4-7. The named variable is still stored normally. | same |
| `[saves] savecontrol` | 1, 2: player 0's `Save!=0` blocks escape-menu and keyboard saves. `GSSetVar(0, "Save", 0)` allows them. | fixed: reads player 0's "Save" without corrupting the stack |
| `autorunmobfile` | 1+: append loading of `Maps/AutoRunSpellAddon123.mob` to each zone's start script. `newspells=1/2` also enables this. | skipped with a warning when `newspells` is 1 or 2 (the original writes the same change twice) |
| `infiniteaddmobspawn` | 1+: `AddMob` can load the same file repeatedly, except case-sensitive names beginning `Auto`, which still load once. | same |
| `standtrans` | `Stand(unit)` also revives it. A non-unit argument causes no revival. | fixed: does nothing for an argument that is not a unit (the original crashes) |
| `waslootedtrans` | 1: `WasLooted(unit)` returns the full flag word. 2: `WasLooted(unit, bit)` returns the selected bit. Known bits: 15 moving, 19 knocked out, 20 looted, 24 stunned, 28 feebleminded. | same |
| `castscripttrans` | 1: `Cast(unit, x, y, spell)` uses the numbered spell. 2: `Cast(unit, x, y, spell, target)` casts on target if present, otherwise at the point. | fixed: 2 reads its target from the running command (the original: a stale value) |
| `castspellunittrans` | `CastSpellUnit(spell, x, y, target, caster)` adds a caster unit to the named-spell command. | same |
| `castspellpointtrans` | `CastSpellPoint(spell, x, y, x2, y2, caster)` adds a caster unit to the named-spell command. | same |
| `giveunitspelltrans` | `GiveUnitSpell` creates/adds a quick item instead of a spell, using the existing unit/name arguments. | same |
| `hptrans` | `HP(unit, index)` reads a float by index from the unit's vitals companion; valid indices 0..29. The useful fields are listed below. | same |
| `mptrans` | `Mana(unit, index)` reads a float from unit stats, indices 0..448; invalid objects/controllers/indices return 0. Also adds the index argument to `HP`, without changing its reader unless `hptrans=1`. | same |
| `legendaryspellsystem` | `MaxHP(unit, index)` reads a byte starting at the skill-byte area, indices 0..631; outside it returns 0. `GiveQuestItem(unit, value)` writes byte 10 of that area. Also changes `MaxMana` to seven numeric arguments, but supplies no working path-query implementation. | fixed: `MaxHP` stays inside the unit (the original reads any address) |
| `getplayertrans` | `GetPlayer(unit, index)` reads a 32-bit integer at four times the index within the unit (0..310); returns 0 outside it. | fixed: returns 0 outside the unit (the original reads any address) |
| `setplayertrans` | `SetPlayer(unit, value, field)` writes a rounded 32-bit integer at four times field within the unit (0..310). Fields 132..148 retain the game's normal player-setting path; out-of-unit writes are ignored. | fixed: stores nothing outside the unit (the original writes any address) |
| `groupseetrans` | `GroupSee(group, mode)` accepts an extra numeric mode argument; group traversal itself remains the game's. | same |
| `unitseetrans` | `UnitSee(unit, mode)` selects the visibility list: positive zero = visible living units; negative sign bit (including negative zero) = the second list; other values = the nearby-units list. | same |
| `getaiclasstrans` | `GetAIClass(unit)` reads the server controller's AI-class field. | same |
| `runtrans` | `Run(unit, style)` sets the requested movement style instead of always selecting run. | same |
| `crawltrans` | `Crawl(unit, slot)` requests a weapon slot instead of setting crawl style. | same |
| `killunittrans` | `KillUnit(type, strength)` sets weather with two rounded numbers instead of killing a unit. | same |
| `fixworldtimetrans` | `FixWorldTime(time, speed)` adds an explicit time-speed argument. | same |
| `movetoobjecttrans` | 1: `MoveToObject(unit, target)` performs thieving while walking. 2: run instead. 3+: `MoveToObject(unit, target, skill, unused, run, slot)` supplies four numbers for the thieving order. | same |
| `playmusictrans` | 1, 2: `PlayMusic(player, "0")` or `"1"` re-selects the local map's music theme and sets the music system's level to the digit. The first character selects this behavior. 1 uses the map's theme, 2 its alternative name field. Other text, or no map/music system, uses normal handling. | fixed: without a map or a music system, does what the game does (the original crashes) |
| `notifydeaths` | 1: a death sets player 0's variable named by the victim ID to 1. 2: set 2 for a hero killer, otherwise 1. | same |
| `notifylootings` | 1: increase the corpse-ID variable by 20 for a hero looter, 10 otherwise. 2: a hero looter's ID goes into the corpse-ID variable. 3: the corpse ID goes into the hero looter's ID variable. Variables belong to player 0. | same |
| `genocideincvars` | 1, 3: player 0's `Hits` counts hero hits on non-heroes. 2 is skipped; 1/3 are skipped with `genocidedipvars` set. | 2 skipped, and 1 or 3 with `genocidedipvars=2` (reads garbage as objects); 1 or 3 skipped with a warning when `genocidedipvars` is 1, 3, 4 or 5, which replaces its change (the original writes it unused) |
| `genocidedipvars` | 1: `Hits` holds the last non-hero victim's side when hit by a hero. 2: `Kils` holds that side on a hero kill. 3: both. 4: `Hits` records any ownerless victim's side; `Kils` records ownerless victims killed by owned units. 5: both require an owned attacker and ownerless victim. Replaces `genocideincvars`. | same |
| `removefrozenvar` | Zone 6 ignores its frozen/unique-zone restriction on entry. | same |
| `freemultiplayervars` | Skip the built-in multiplayer variables in trader creation and journal refresh. | same |
| `setspellaggressiontrans` | No change for any value; the command itself remains inactive. | same |
| `[tech] scriptchecking` | 1-7: append script errors to `SAlog.txt`. 2, 4-7 also log mob loads. 1/2 do not record errors in the script. 3/4/6/7 return nonzero from error handling. 5 permits eight script-load errors to pass. 8+ are skipped. | 8 and up skipped (calls code the original never wrote); 1 and 2 write the line where the original crashes at the first script error |
| `[tech] drawunitsids` | Prefix the unit-info name with its ID; truncate to 255 bytes. Safe with no selected unit. | fixed: works without a unit (the original crashes); lines cut at 255 bytes |

Loot and death notifications and genocide take IDs of at least one billion as heroes and lower IDs as non-heroes
(the original's death and genocide tests take IDs strictly above one billion).

With `newspells=1..3`, spell types 44-48 publish `S0e` (strength), `S1d` (duration), `S2t` (type), `S3x`/`S4y`
(point) on the first server player; scripts can read them with `GSGetVar`. They give the caster hostile mask 29 and
side `round(strength * 32)`. Mode 1 also gives spell 49's caster mask/side 29. These hooks expose script inputs;
they do not supply a summoning script or mod assets.

### Indexed stat readers

`HP` indices: 10 current HP, 11 maximum HP; 13 current stamina, 14 maximum stamina, 15 stamina regeneration.
Resistance uses base/current pairs: 16/17 piercing, 18/19 slashing, 20/21 crushing, 22/23 fire, 24/25 acid,
26/27 lightning, 28/29 general.

`Mana` reads the underlying float fields. Do not treat packed flags or skill bytes as ordinary numeric stats.

| Indices | `Mana(unit, index)` field |
|---|---|
| 1, 2 | Total and spent experience. |
| 3/4, 5/6, 7/8 | Natural/total strength, dexterity, intelligence. |
| 9/10, 11/12 | Natural/total movement and action coefficients. |
| 13-16 | Run, walk, crawl, lie speeds. |
| 17 | Vision arc. |
| 20/21 | Current/maximum load. |
| 28-32 | Natural reach, attack, attack spread, defence, speed. |
| 33/34/35 | Natural damage, damage spread, attack interval. |
| 36-42 | Natural damage weights, in damage-type order 0..6. |
| 44/45 | Base/current sight range. |
| 56/57, 62/63 | Base/current visibility and loudness. |
| 125/126/127/128 | Head HP/max HP/HP share/damage scale. |
| 186/187/188/189 | Body HP/max HP/HP share/damage scale. |
| 247/248/249/250, 308/309/310/311 | Left/right arm HP/max HP/HP share/damage scale. |
| 369/370/371/372, 430/431/432/433 | Left/right leg HP/max HP/HP share/damage scale. |
| `71 + 61*p + 18*l + t` | Armour absorption: part p = 0..5, layer l = 0..2, damage type t = 0..6. |
| 447, 448 | Fixed maximum HP and pending experience. |

## Parameters

Each row gives the default and the feature that uses it. F = floating-point; I = 32-bit integer; B = byte.
Unless stated otherwise, these are `[gameplay]` keys. Bounds and spell-ID lists below describe meaningful values;
values outside them can select unrelated fields or unsupported game data.

### Progression, speed and presentation

| Section | Key(s) | Default(s) | Type | Use |
|---|---|---|---|---|
| `skills` | `skills` | 0 | B | `extendedskills`: non-stealing skill cap. |
| `stealing` | `stealing` | 0 | B | `extendedstealing`: stealing cap. |
| `rebalanceengine` | `pow`, `mul`, `add` | 1.1, 4.5, 0.5 | F | `rewriteskillscounting`: exponential base, multiplier, addition. |
| `rebalanceengine` | `mullvla`, `addlvla`, `divlvla`, `addlvlb` | 4.5, 1.5, 245.5, 0.5 | F | `rewriteherolevelcounting`: level formula. |
| `rebalanceengine` | `runconst`, `runconstex` | 0.006666666666, 0.5 | F | `rewriterunabilitytime`/`newrunsystem`: drain coefficient and extra drain. |
| `rebalanceengine` | `bsbase` | 300 | I | `backstabvisualbase`: displayed percent. |
| `speed` | `normal`, `fast` | 55, 27 | I | `rewritespeed`: clock periods. |
| `speed` | `normalkey`, `fastkey` | 55, 27 | I | `rewritespeedkey`: keyboard periods; the game's own are 55 and 27 (the original's built-in `normalkey` is 5). |
| `speed` | `normal1` | 55 | I | `rewriteotherspeed` 1/2: close/reset period. |
| `speed` | `normal2`, `fast2` | 55, 27 | B | `rewriteotherspeed` 1/3: initial clock periods. |
| `speed` | `pause`, `pausekey` | 0, 0 | I | `disablepause`/`disablepausekey`: replacement periods. |
| `animationsspeed` | `attackspeed`, `castspeed`, `hitspeed`, `deathspeed`, `specialspeed`, `idlespeed` | All 1 | F | `rewriteactionsspeed`: attack, cast, hit reaction, death, special, idle animation factors. |
| `gameplay` | `expforint` | 1.0245 | F | `changeexpforint`: experience factor per intelligence point. |
| `gameplay` | `actionsmul`, `actionsadd` | 0.2, 10.0 | F | `actionspatch`: dexterity multiplier and action addition. |
| `gameplay` | `actmovemul`, `dexmovemul` | 0.5, 0.01 | F | `actionsdexteritymovespeed`: actions/dexterity contributions. |
| `gameplay` | `speedbase`, `speedadd` | 0.3, 5.0 | F | `speedboost`: exponent and counter addition. |
| `gameplay` | `parammin`, `parammax`, `balance` | 15.0, 35.0, 75.0 | F | `extendedparams=2`: attribute bounds and displayed balance; sum enforcement is bypassed. |
| `gameplay` | `perssize` | 1.2 | F | `rewriteperssize`: hero preview scale. |
| `gameplay` | `framex`, `framey` | 75.0, 175.0 | F | `allodsinfo`: frame width/height; integer parts also size text rectangles. |
| `gameplay` | `meter` | 15.0 | F | `meterscale`: ruler factor. |
| `gameplay` | `newlightsvalue` | 0.001 | F | `lightsboost`: light strength. |
| `gameplay` | `exred`, `exgreen`, `exblue` | All 0.5 | F | `exploredcolors`: RGB components; normally 0..1. |
| `fonts` | `newfont1`, `newfont2`, `newfont3`, `newfont4` | All `Times New Roman` | Text | `changefont`: four interface font families. |
| `fonts` | `newfont5` | `Times New Roman` | Text | `changefont=2`: font-file name relative to the INI folder, not a family name. |

### Combat, shops and load

| Key(s) | Default(s) | Type | Use |
|---|---|---|---|
| `divmod`, `submod` | 100, 1 | F | `newattackdefence`: divisor/base for modes 1/2; multiplier/floor for 3/4. |
| `str`, `dex`, `int` | 100.0012, 100.0013, 100.0014 | F | `damageboost`: strength/dexterity/intelligence scaling; divisors in mode 1, multipliers in 2/4/5, unused in 3. |
| `strhp`, `strdmg`, `strweight` | 0.005, 0.003, 12.0 | F | `strengthstats`: strength coefficients. |
| `weight`, `range` | 20.0, 10.0 | F | `changeweightrangebonuses`: weapon-weight and distance constants. |
| `bodypartbreakmodifier` | 4 | F | `unbreakablebodyparts`: replace the limb-damage floor constant (normally -5). |
| `headmod`, `legmod`, `handmod` | 3, 1, 1 | F | `randomlessaimedhits`: head/leg/arm score weights. |
| `criticalmod` | 2 | F | `randomlessaimedhits`: boost a head score reaching full head HP, and leg scores at ranged reach. |
| `healthdowntoignore` | 10.25 | F | `randomlessaimedhits`: ignore non-body parts when current HP times this value is below maximum HP. |
| `rangeddistance` | 5.65 | F | `randomlessaimedhits`: reach threshold for boosting leg scores. |
| `bodypart` | 1 | I | `randomlessaimedhits`: fallback part; 0 head, 1 body, 2/3 arms, 4/5 legs. |
| `checkhero` | 1 | I | `randomlessaimedhits` 5-9: nonzero makes player-owned attackers use `bodypart` instead of the chooser. |
| `tohitcheck` | 1 | B | `increasinghitchance`: miss count must exceed this. |
| `tohitsub`, `tohitmul` | 0.0, 5.5 | F | `increasinghitchance`: subtraction and counter multiplier. |
| `hitdistmul`, `hitdistadd` | 2.0, 5.0 | F | `runawaydistcontrol`: reach multiplier/addition. |
| `nightmeleeadd`, `nightmeleemul` | 1.0, 1.0 | F | `nightpower=2`: night-factor base and slope. |
| `maxcountofmerc` | 27 | I | `extendedmerc`: limit uses the low byte; full value controls the >50 draw-range condition (with `extendedmerc` 16+ and `saveall=32`). |
| `minsweapons`, `maxsweapons` | 1, 10 | B | `shopsnewnormalchanges`: weapon stock range. |
| `minsarmors`, `maxsarmors` | 1, 10 | B | `shopsnewnormalchanges`: armour stock range. |
| `minswands`, `maxswands` | 1, 10 | B | `shopsnewnormalchanges`: wand stock range. |
| `minspotions`, `maxspotions` | 20, 65 | B | `shopsnewnormalchanges`: potion stock range. |
| `minsprototypes`, `maxsprototypes` | 1, 10 | B | `shopsnewnormalchanges`: prototype stock range. |
| `minsrunes`, `maxsrunes` | 20, 56 | B | `shopsnewnormalchanges`: rune stock range. Use stock counts 0..127 because the game sign-extends these bytes. |
| `ingos`, `gipatz`, `gipatb`, `suslanger`, `jigran` | All 0.5 | F | `shopsnewnormalchanges`: sell-price factors for Ingos, Gipat healer, Babur, Suslanger and Jigran. `shopschanges` also reads `jigran` when `shopsnewnormalchanges=1`. |
| `liftkey` | 1.0 | F | `liftdebuffs`: load-ratio divisor term. |
| `encumbrancemovemul`, `encumbranceattackmul` | 1.0, 1.0 | F | `encumbrancemovespeed`, `encumbranceattackspeed`: load penalties for movement and attack. |
| `speedslowmovemul`, `defaultspeedmul`, `minimumspeedmul` | All 1.0 | F | `encumbrancemovespeed` with `speedslowremaster=2`: effect multiplier, overall multiplier, minimum factor. |
| `dmgtypescount` | 6 | I | `dmgtypes`: displayed count; the game has seven damage types (0..6). |
| `light00` through `light31` | All 1 | F | `itemslightchanges`: eight RGBA groups in order; 00-03 first colour, 04-07 second, through 28-31 eighth. Normally 0..1. |

### AI, spells and zones

| Key(s) | Default(s) | Type | Use |
|---|---|---|---|
| `visionarc` | -2 | F | `rewritenpcvisionarc`: NPC sight-arc value. |
| `visiondependence` | 1.05 | F | `aggrmod` 2/4/5/6/7: NPC vision-distance factor. |
| `aggrrangemulmodifier` | 4 | F | `aggrmod=1` stores it but does not use it. |
| `losehardcore` | 100000 | I | `aggrmod=2`: random modulus. 6/7: counter limit uses only 16 bits (default becomes 34464). |
| `losefactor1`, `losefactor2` | 15, 5 | I | `aggrmod` 4/5: time divisor multiplier and right-shift count. 6/7: distance multiplier and level-divisor addition. |
| `losefactor3`, `difffactor`, `maxadd` | 5, 2, 2 | I | `aggrmod` 6/7: distance addition, difficulty addition, quotient threshold. These and `losefactor1/2` use signed low bytes in 6/7. |
| `follow` | 0 | I | Nonzero lets units continue towards a lost enemy's position in active `aggrmod` modes. |
| `improve` | 0 | I | Nonzero requests the faulty alarm hook in `aggrmod` 6/7; eix omits it and warns. |
| `loseallyfast` | 0 | I | Nonzero in `aggrmod` 6/7 bypasses the counter for units not classed as enemies. |
| `sametarget`, `magicattack`, `damagemult`, `magicdamagemult`, `savedamagemult`, `rangecost`, `kickprice` | All 10.0 | F | `spellsprioritys`: same-target bias, magic bias, weapon/spell/healing priorities, casting-distance cost, quick-kill target factor. |
| `safetymuldist`, `maxvisibilitymul` | 2.0, 3.0 | F | `newplayersafetyconditions`: safety distance and visibility factors. |
| `fearsafetydist`, `legsdamagedmul` | 10.0, 0.6 | F | `fearpatch`: retreat distance and damaged-leg threshold. |
| `runtosoundsmul`, `randsoundfactor` | 1.0, 1.0 | F | `hearingreactionspatch=2`: extrapolation from sound position and random scatter factor. |
| `visionweathermul` | 0.9 | F | `visioninweather`: bad-weather vision factor. |
| `visionevening`, `visionnight`, `visionmorning`, `visionday` | 18.0, 22.0, 4.0, 8.0 | F | `nightpowervisionpatch`: hours for dusk/full night/dawn/full day. |
| `visionmulpowbetween` | 0.250 | F | `nightpowervisionpatch`: night-level ramp slope per hour. |
| `astralbeam`, `destroyray`, `firearrow`, `lightningbeam` | 5, 3, 3, 5 | B | `lastspellsdamagetype`: 0 piercing, 1 slashing, 2 crushing, 3 fire, 4 acid, 5 lightning, 6 general. |
| `slownessmultiplier`, `weaknessmultiplier` | 12, 0.5 | F | `controlmagic=1`: resistance scaling for Slow and Weaken. |
| `durcoeff`, `effcoeff`, `lvldurcoeff` | All 0.03 | F | Read but no active duration/effect formula consumes them; `lvldurcoeff` is stored by some `controlmagic` modes. |
| `mul3`, `pow3` | 1.0, 1.0 | F | Read but unused by the built-in features. |
| `frequencyvalue` | 0 | B | `wallsfrequency`: ticks per damage event; set a meaningful positive period. |
| `firepercentagevalue`, `lightningpercentagevalue`, `acidpercentagevalue` | All 0.1 | F | `wallspercentage`: fire, lightning and acid wall fractions. |
| `stenchmul`, `regenerationmul`, `speedslowmul`, `strengthweakmul` | All 0.1 | F | Respective Stench, Regeneration, Speed/Slow and Strengthen/Weaken remasters. |
| `tordmg` | 5.0 | F | `tornadochanges`: damage per tornado hit. |
| `fireboom` | 5.0 | F | `changesounds`: Fireball volume factor. |
| `minmusictime`, `maxmusictime` | 120000, 180000 | I | `musictimings`: minimum and exclusive upper delay in milliseconds. |
| `maxsegments`, `addsegments` | 1000, 0 | I | `segmentsonexitenter`: segment cap and addition. |
| `exitenteractivityradius` | 70.0 | F | `segmentsonexitenter`: transition activity radius. |

### Per-spell bonuses - `[gameplay]`

Each switch defaults to 0 (B); each bonus defaults to 0.0 (F). `controlmagic=3` applies the bonus only when the
switch is odd. Modes 2/5/7 apply all bonuses. `effectredraws=1` displays all bonuses regardless of these switches.

| Spell | Switch | Bonus |
|---|---|---|
| Protection from Fire | `prot_fire` | `bprot_fire` |
| Protection from Lightning | `prot_electro` | `bprot_electro` |
| Protection from Acid | `prot_acid` | `bprot_acid` |
| Eagle Sight | `eagle_sight` | `beagle_sight` |
| Night Vision | `infravision` | `binfravision` |
| Detect Life | `detect_life` | `bdetect_life` |
| Invisibility | `invisibility` | `binvisibility` |
| Silence | `silence` | `bsilence` |
| Lichdom | `lichdom` | `blichdom` |
| Stench | `stench` | `bstench` |
| Stun | `stun` | `bstun` |
| Antimagic | `antimagic` | `bantimagic` |
| Strengthen | `strength` | `bstrength` |
| Weaken | `weak` | `bweak` |
| Regeneration | `regeneration` | `bregeneration` |
| Feeblemind | `feeblemind` | `bfeeblemind` |
| Speed | `speed` | `bspeed` |
| Slow | `slow` | `bslow` |
| Enlarge | `enlarge` | `benlarge` |
| Shrink | `shrink` | `bshrink` |
| Necro | `necro` | `bnecro` |
| Destroy | `destroy` | `bdestroy` |

### Spell AI table entries

| Section | Keys | Default | Accepted meaning |
|---|---|---|---|
| `aicastertable` | `0` through `53` | All 8 | B; 0 missile, 1 area attack, 2 lasting area attack, 3 out-of-combat buff, 4 combat buff, 5 unused, 6 healing, 7 debuff, 8 unused. Used with `rebuildedaicastertable=1`. |
| `aioffendtable` | `0` through `37` | All 8 | B; 0 harmful, 1 friendly. Set meaningful entries before enabling `rebuildedaioffendtable=1`; the default 8 is not a normal classification. |

### Read-only placeholders - `[gameplay]`

These switches default to 0 and have no feature of their own. Changing them has no effect in the built-in version.

| Key(s) | Status |
|---|---|
| `enemysmana` | Read only; use `npcsmana` for the implemented mana rules. |
| `newspells_fireraze`, `newspells_inspircharm`, `newspells_spellsmachine`, `newspells_invislichd` | Read only; `newspells` controls the implemented spell extensions. |
| `hideobjecttrans`, `stableaggrmod`, `helloworld`, `castspellpatch` | Read only; no command/AI change. |
| `newallodsinmulti`, `autocasts` | Read only; no allod-selection or automatic-casting feature. |
| `walktrans`, `lietrans`, `resttrans`, `getleadertrans`, `inflictdamagetrans`, `isunitblockedtrans`, `setplayeraggressiontrans` | Read only; the named script interfaces retain their existing behavior. |
