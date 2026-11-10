# Recipes

Common changes: which file to edit, which keys to set, and to what. Each key is described in full on its reference
page.

## The files

| File | Where | Reference |
|---|---|---|
| `spelladdon.ini` | The mod's folder (`Mods\<mod>`, beside its `config.reg`); without one there, the game folder's. eix reads one of the two, without merging them. | [SpellAddon options](spelladdon.md) |
| `eix.ini` | The mod's folder | [eix.ini](eix-ini.md) |

- Edit them in any text editor, then restart the game.
- Write fractions with a decimal point: `0.25`.
- Updating or reinstalling a mod can replace its files: keep a note of the changes.

## Running drain (stamina)

Running drains mana at every game tick. The game takes the maximum mana / 150 per tick, so a full bar always lasts
the same time, however large it is. `[rebalanceengine]` in `spelladdon.ini` changes that:

```ini
[rebalanceengine]
rewriterunabilitytime=1
newrunsystem=1
runconst=0.5
```

| Drain per tick | `newrunsystem` | `runconst` |
|---|---|---|
| A fixed amount: a larger bar runs longer | `1` | The amount |
| A share of the maximum, as in the game | `0` | The share; the game's is `0.006666666666` (1/150) |
| A share of the maximum plus a fixed amount, `runconstex` | `2` | The share |

`rewriterunabilitytime=1` makes the game use `runconst`; keep it in every row.

With a fixed amount:

- Lower `runconst` to run longer: `0.25` runs twice as long as `0.5`.
- `runconst` = M / 150 drains as the game does for a hero with a maximum of M mana: `0.5` matches the game at 75.
  A smaller bar empties sooner than in the game, a larger one later.

Related:

- eix's [encumbrance](engine.md#encumbrance) (`[Encumbrance] Enabled=1` in `eix.ini`) raises the drain with the
  load: 1.25 times at the maximum load with the defaults. `RunDrain=0` there removes that extra, `50` halves it.
- `[gameplay] automaticruncontrol=1` in `spelladdon.ini` stops a unit from running when its mana runs out.
- Any other formula: the [Lua](lua.md) callback [CalcRunningManaCost](lua-reference.md#calcrunningmanacost).
