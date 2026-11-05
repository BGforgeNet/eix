# game: typed access to game.exe RU 1.06

The structures, functions and globals that patch groups, Lua functions and SpellAddon reach in game.exe. Each item's
address and the game.exe code that shows its meaning are in its doc comment; this page lists what is covered and what
was left out.

## How an item gets in

- A structure field is named only when a game.exe function reads or writes it at that offset with that meaning; the
  rest stays `_unknownNN` padding. Sizes come from the allocation (`push size; call operator new`) or from the clear
  or copy that covers the whole object.
- A function pointer's calling convention is read from its body: `ecx` taken as `this`, the `ret N` that cleans its
  stack arguments, `edx` written before it is read for fastcall.
- A static object's address is the `ecx` its static initialiser passes to the constructor.
- Names follow ei-rebuild. ei-universal-mod's globals serve as a cross-check only
  (the tests compare their addresses with field offsets).
- Some fields the original addon reads or writes at a fixed offset (the script commands' unit statistics, the
  weapon record, the AI brain's alarm records) carry ei-rebuild's name and the addon's use as their evidence, where no
  game.exe reader was looked up; their doc comments say so.

## Covered

| File | Items |
|---|---|
| `mod.rs` | `Global<T>` (a typed global), the function-pointer helper |
| `mfc.rs` | `GameString` (`CString`) and its header, `GameArray<T>` (`CArray`), `PtrList` (`CList` of pointers) and its walk, constructor with a block size, remove-all and destructor; `operator new` / `delete`; `CString` construct (empty or from text), assign, append, destroy, format with the game's printf (`FormatV`) into a string or bytes; `CDC::FromHandle` |
| `object.rs` | `EngineObject` and the release, class-name and class-id vtable slots, release, a map object's dead flag (slot 55), the unit, client-controller and server-half class ids; `TransformObject`; `Vector3`, `Quaternion` |
| `unit.rs` | `MapObject` (transform, scale, companion, update countdown, animation speed, packed vitals, effects, base scale) and its effect value and change mark, the removal flag, `MapEffect`, `PackedVitals`, `MapObjectServer` (vitals), `UnitVitals` (and its protection pairs), `StatPair`, `Unit` (script name, controller, player, side, motion mode, active and pending weapon, dead, params, weapon / quick / spell / armour slots, carried items), the slot getters, prototype, add weapon, equip armour, protection block; `UnitStats` (build flags, experience, spent and pending experience, base and current strength, dexterity, intelligence, base and current rate, loads, combat values, natural damage, attack interval, damage weights and weapon type, hit-point slots and their armour layers, attribute and skill bytes, perk levels, flags, fixed maximum) and its heal, reduce, slot maxima, pack, scaled experience, experience factor, perk level value and record, recompute, `UnitStatSlot`, `UnitStatLayer`, `UnitParams` (combat bytes, perk levels, flags), the server vitals lookup, `UnitController` (unit), `UnitServer` (unit, action, stats, brain), its level, scaled attack value, add experience, move style, wanted weapon, model and equipment refresh, `UnitAction` (kind, source, target), `AiBrain` (alarm slot and records, memory) and apply behaviour, `AiMemory` reset, `UnitPrototype` (name, model, protection scale, combat), `AnimationEntry` (key, frame count), `OrderPlan` (cast kind), `ActionItem` |
| `combat.rs` | `AttackParams` (attack spread, attack, defence), its difficulty scaling and the unit server's `GetAttackParams` |
| `player.rs` | `Player` (server half, number, units, party, variables, god mode), `PlayerClient` (player), `UnitMapNode`, `ItemList`, `PartyRecord` (unit id, item lists) and the party lookup, `NamedFloatTable` walk, get, set and clear; `PlayerServer`: set variable, send a unit's experience, resend the party, send a string event; `PlayerIterator` over a player manager (first, next, release); save party into a memory stream; the hero file name global |
| `stl.rs` | STLport `HashTable` (buckets) and its walk |
| `database.rs` | the perk, weapon, armour, material, quick item, spell and rune record vectors; `SkillDatabase` and its perk index |
| `item.rs` | `Item` (condition, charge, weight, price, spell) and its prototype and weapon type, `WeaponRecord` (attack interval, bonuses, reach, damage types), `ItemWeapon` (damage, spread), `ItemArmor` (absorb blocks) |
| `spell.rs` | `Spell` (type, damage, mana cost, cast time, modifiers) and its record, `SpellRecord` (school, slots, damage, cast time, damage flag), `ModifierRecord` (code, strength, skill), `SpellContainer` (cast time) |
| `world.rs` | `World` (time base, time per tick, tick, paused), the server and client worlds; pause, find a map object, find a unit, find any object of the server world, the player manager (player count), get a player, show a number; the server's object maps and their walk; add to the update map; the script switch; the sector map and its radius query; the server world's create item, award experience, loot, remove object, load map objects, set weather, load zone scripts, setup, new zone, remove all units, sync party records, clear player items, loot dead units, map path; the client's pause request; the save version; the loading-movie flag |
| `net.rs` | the game client and server globals, is a network client, a session's +4 word, announce; `DeltaStream`: write a field as a delta or always, close a field group, read either kind |
| `crt.rs` | the game's `rand`, `atof` and `_CIpow`; `_tolower_lk`'s address and the LC_CTYPE locale it reads; the `HeapAlloc` and `HeapReAlloc` import slots and the heap the C runtime chose |
| `flow.rs` | the game flow's stop sounds, stop ambience, camera recording start and stop; the random generator's next value and range; the draw-all flag; the game speed and the difficulty; the camp variable and leaving the zone |
| `mob.rs` | `MobSpec` (a mob file's object record: parent chunk, id) and its deleting destructor |
| `camera.rs` | `Camera` (transform, zoom target, focus, distance, pitch, near, far, field of view), the view camera |
| `console.rs` | the public and cheat command maps, the console stream; run a line, a command's slot, register a command, print a line; `ConsoleWindow` (history, input, caret, anchor, lines, ctrl, shift, shown), its add-text and draw-input |
| `ui.rs` | `Interface` (console window), the interface; add console text, play a sound, push a window; construct a message box; `UiContainer` (children), a window's enabled state; the cursor and its image; `Rect`, `Point`; `Surface` (device context, holder) and its GetDC, ReleaseDC, text draw and wrapped text draw; `UiRegion` (vertices, bounds, flags); `UiMesh` (place, visible, tint) and set rectangle; `UiWindow` (rectangle, sprites) and add sprite; `HudUnitPanel` (slide fields); `HelpWindow` (message, text, page); `BriefingScreen` (topic); the 2D layout's constants |
| `media.rs` | the `_BinkOpen@8` import slot and its type, the twelve `binkw32.dll` import slots by name, `_BinkSetVolume@12`'s type; the loading screen's movie |
| `res.rs` | `MappedResource` (size only); construct, open, `GetFile`, `ListFiles`; `CStringArray` construct, append, destroy; the figure, linked-figure and texture managers' add-archive calls |
| `script.rs` | `ScriptKeyword` and the keyword table, `ScriptCommand` (`SSCCmd`: id, float, string, object and group arguments) and resizing its argument arrays, `ScriptObjectRef` and assign, `ScriptGroup` insert and clear, `ScriptVars`, `Script` (`CScript`: globals, load error) and the zone's script; load a file, tick, construct, reset, load text, destruct |
| `ai.rs` | `AiMap` (opaque) and its path cost search, `RegionMap` (edge and representative-cell grids per size class), `SearchGrid<T>`; the region map's build and `SaveBlocks`; the `MeasureBlockEdges` call site and type |
| `start.rs` | what WinMain loads before a zone: the registry key and start folder strings, store start folder, the options and their database word, the logic settings, the unit, skill, spell and item databases and the lever records |
| `stream.rs` | the memory and file streams (`ByteStream`): construct, destruct, open for writing, `CopyFrom`, save-file write, close the file, the virtual destructor in place |
| `surface.rs` | `TextureDevice` (DirectDraw object, primary, render target), `DxSurface` (`CDXSurface`: device, surface, size, bits per pixel); the DirectDraw lock, enter and leave |
| `text.rs` | `TextManager` (cursor, open text, length) and the shared text manager; open a text |

## Left out

| Item | Why |
|---|---|
| `CItem` stack count (+0x48) | only `CopyToImpl`'s write of 1 was found; no reader shows it is the count |
| `CItemWeapon` reach, `CItemQuick`, `CItemLoot` | copied from the prototype only; no reader checked |
| the other `CSpell` fields (duration, radius, speed, range, targets, price, skill) | `InitFromPrototype` copies them from the record; their meaning rests on the record's, unverified |
| `UnitStats` speeds, sight values | not checked against a reader yet |
| `UnitVitals` hit-point regeneration (+0x10) | `Regenerate` was read for mana only |
| `CUnit` relations, movement modes, fight-at-will | no group need found yet |
| `CWorldServer` / `CWorldClient` fields past the shared head (zone, diplomacy, camera, managers) | large and unverified; the shared head covers tick and pause |
| `GameFlow`'s fields | no field verified; its static calls are covered, its functions are hooked |
| `CScript` blocks, definitions, instances, runs, effects; `SSCCmd` bindings and nested calls; `ScriptRun` | no group reads them; the hooks take what they need from registers |
| `CPlayer` past the server half, units, party, variables and god mode; the party manager's records past the unit id and item lists; quests | no reader checked |
| the screen-size globals | WideScreen keeps its own layout and reads none |
| the console's stream buffers and the other log streams | only the console stream is needed to print |
| the game's functions at large | most of those functions are hooked, not called; typed pointers cover the functions the rewrites call |
