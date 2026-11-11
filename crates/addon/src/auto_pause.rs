//! AutoPause, from eix.ini's `[AutoPause]` in the mod folder: in single player the game pauses when a party
//! member's health drops below a limit, an enemy comes into view, an enemy turns on the party or an enemy in view dies
//! (core's [`eix_core::auto_pause`]).
//!
//! The check runs after each server tick: `CWorldServer::Update` 0x58DFF0 calls `CWorldServer::Tick` 0x58DCD0 at
//! 0x58E20B only while the server's world is not paused. It reads the server world's players and units, and pauses
//! as the pause key does, through the client's pause request.
//!
//! - Health: the server-side vitals of every unit of each player.
//! - In view: the units each party unit's AI sees (the list the script `PlayerSee` reads), less those with a player,
//!   the dead and those not hostile to the player's side (its number, as the game's own danger test takes it).
//! - Aggression: each living hostile unit of the server world whose AI aggression is aimed at a party unit, as
//!   `CPlayerServer::SendPartyState` finds it for the danger state (0x66E666).
//! - Deaths: `CUnitServer::OnDamaged` is the one caller of `CUnitServer::SetDead` 0x546720 (0x553BB0); its call is
//!   redirected to note the unit, and the next check pauses when the unit was in view at the check before.
#![expect(
    unsafe_code,
    reason = "hooks the server's tick and a unit's death, and reads game.exe's players and units"
)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use eix_core::auto_pause::{
    AGGRO_KEY, DEAD_KEY, HEALTH_KEY, Limits, Onset, PERCENT_KEY, SIGHTED_KEY, Triggers, Watch,
    death_in_view, is_enemy,
};
use eix_engine::game::net::{is_multiplayer_client, live_server};
use eix_engine::game::object::{CLASS_UNIT, EngineObject};
use eix_engine::game::player::{Player, PlayerIterator, UnitMapNode};
use eix_engine::game::stl::HashTable;
use eix_engine::game::unit::{AiBrain, FLAG_REMOVED, Unit, UnitServer, server_vitals};
use eix_engine::game::world::{CLIENT_WORLD, ObjectMap, SERVER_OBJECTS, SERVER_WORLD, World};
use eix_engine::hook;
use eix_sites::Site;

use crate::game::BeforeWinMain;
use crate::groups::{Labelled, Tally, apply_built};
use crate::log;

/// The call of `CWorldServer::Tick` in `CWorldServer::Update`.
const TICK_CALL: Site = eix_sites::C_WORLD_SERVER_UPDATE__21B;
const TICK_CALL_BYTES: [u8; 5] = [0xE8, 0xC0, 0xFA, 0xFF, 0xFF];
/// The call of `CUnitServer::SetDead` in `CUnitServer::OnDamaged`, `ecx` the dying unit's controller.
const DEATH_CALL: Site = eix_sites::C_UNIT_SERVER_ON_DAMAGED__F0;
const DEATH_CALL_BYTES: [u8; 5] = [0xE8, 0x6B, 0x2B, 0xFF, 0xFF];

/// `CWorldServer::Tick` (thiscall, `ret 4`); `Update` drops what it returns.
type Tick = unsafe extern "thiscall" fn(*mut World, i32) -> u32;
/// `CUnitServer::SetDead` (thiscall, `ret 4`): the dead flag to set.
type SetDead = unsafe extern "thiscall" fn(*mut UnitServer, i32);

/// `CWorldServer::Tick`, the call's old target.
static TICK: AtomicUsize = AtomicUsize::new(0);
/// `CUnitServer::SetDead`, the call's old target.
static SET_DEAD: AtomicUsize = AtomicUsize::new(0);

/// What the checks keep between ticks; the game's thread alone runs them.
struct State {
    triggers: Triggers,
    /// The party's health.
    watch: Watch,
    /// Enemies in view.
    sighted: Onset,
    /// Enemies aggressive to the party.
    aggro: Onset,
    /// The enemies in view at the last check.
    in_view: Vec<i32>,
}

static STATE: Mutex<State> = Mutex::new(State {
    triggers: Triggers {
        limits: Limits {
            health: 0,
            percent: 0,
        },
        enemy_sighted: false,
        enemy_aggro: false,
        enemy_dead: false,
    },
    watch: Watch::new(),
    sighted: Onset::new(),
    aggro: Onset::new(),
    in_view: Vec::new(),
});
/// The units without a player that died since the last check.
static DEATHS: Mutex<Vec<i32>> = Mutex::new(Vec::new());

/// The tick, then the check.
extern "thiscall" fn tick(world: *mut World, arg: i32) -> u32 {
    // SAFETY: TICK holds CWorldServer::Tick, of this type, once `install` ran; the call is redirected only after.
    let tick = unsafe { hook::as_fn_at::<Tick>(TICK.load(Ordering::Relaxed)) };
    // SAFETY: the game's own call, with its arguments.
    let result = unsafe { tick(world, arg) };
    // SAFETY: the game's thread, between two server ticks.
    unsafe { check() };
    result
}

/// The unit's death, then a note of it for the next check: pausing inside the server's tick is left to the check.
extern "thiscall" fn set_dead(controller: *mut UnitServer, dead: i32) {
    // SAFETY: SET_DEAD holds CUnitServer::SetDead, of this type, once `install` ran.
    let set_dead = unsafe { hook::as_fn_at::<SetDead>(SET_DEAD.load(Ordering::Relaxed)) };
    // SAFETY: the game's own call, with its arguments.
    unsafe { set_dead(controller, dead) };
    if dead == 0 {
        return;
    }
    // SAFETY: the dying unit's live server controller, as the game passed it.
    let unit = unsafe { (*controller).unit };
    if unit.is_null() {
        return;
    }
    // SAFETY: the controller's live unit.
    let u = unsafe { &*unit };
    if u.player.is_null()
        && let Ok(mut deaths) = DEATHS.lock()
    {
        deaths.push(u.map_object.transform.base.id);
    }
}

/// The server world's players.
///
/// # Safety
/// On the game's thread, with the server's world constructed.
unsafe fn players() -> Vec<*mut Player> {
    let mut players = Vec::new();
    // SAFETY: the server's world is constructed at start-up.
    let list = unsafe { World::players(SERVER_WORLD.as_ptr()) };
    if list.is_null() {
        return players;
    }
    let mut it = PlayerIterator::new();
    // SAFETY: the server world's live player list.
    let mut player = unsafe { it.first(list) };
    while !player.is_null() {
        players.push(player);
        // SAFETY: the walk was started by `first`.
        player = unsafe { it.next() };
    }
    // SAFETY: as above.
    unsafe { it.release() };
    players
}

/// The units of `player`.
///
/// # Safety
/// `player` must be a live player of the server's world, on the game's thread.
unsafe fn units(player: *mut Player) -> Vec<*mut Unit> {
    let mut units = Vec::new();
    let add = |node: *mut UnitMapNode| {
        // SAFETY: `for_each` passes live nodes.
        let unit = unsafe { (*node).unit };
        if !unit.is_null() {
            units.push(unit);
        }
    };
    // SAFETY: the caller guarantees a live player; only the table's address is taken.
    let table = unsafe { &raw const (*player).units };
    // SAFETY: the player's unit table; reading it leaves it unchanged.
    unsafe { HashTable::for_each(table, add) };
    units
}

/// Whether `unit` lives, has no player and is hostile to `side`.
fn is_living_enemy(unit: &Unit, side: i32) -> bool {
    unit.player.is_null()
        && unit.dead == 0
        && is_enemy(unit.hostile, unit.hostile_limit, side.cast_unsigned())
}

/// The ids of the living enemies of `player` that its units see.
///
/// # Safety
/// As for [`units`].
unsafe fn in_view(player: *mut Player, ids: &mut Vec<i32>) {
    // SAFETY: the caller guarantees a live player.
    let side = unsafe { (*player).number };
    // SAFETY: as above.
    for unit in unsafe { units(player) } {
        // SAFETY: a live unit of the player's table; on the server its controller is a `CUnitServer` or null.
        let controller = unsafe { (*unit).controller }.cast::<UnitServer>();
        if controller.is_null() {
            continue;
        }
        // SAFETY: the controller's live AI memory.
        let mut node = unsafe { (*controller).brain.memory.visible };
        while !node.is_null() {
            // SAFETY: a node of the live list; the list holds a reference on its unit.
            let n = unsafe { &*node };
            let seen = n.unit;
            node = n.next;
            if seen.is_null() {
                continue;
            }
            // SAFETY: as above.
            let s = unsafe { &*seen };
            let id = s.map_object.transform.base.id;
            if is_living_enemy(s, side) && !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
}

/// The first living enemy of a player whose AI aggression is aimed at one of that player's units.
///
/// # Safety
/// On the game's thread right after a server tick, `players` the server world's live players.
unsafe fn aggressor(players: &[*mut Player]) -> Option<i32> {
    // SAFETY: on the game's thread after the tick; nothing changes the map during the walk.
    for node in unsafe { ObjectMap::nodes(SERVER_OBJECTS.as_ptr()) } {
        // SAFETY: a node of the map.
        let object = unsafe { (*node).object };
        if object.is_null() {
            continue;
        }
        // SAFETY: the map holds live map objects.
        let o = unsafe { &*object };
        if o.transform.base.flags & FLAG_REMOVED != 0 {
            continue;
        }
        let engine = object.cast::<EngineObject>();
        // SAFETY: a live engine object.
        if unsafe { EngineObject::class_id(engine) } != CLASS_UNIT {
            continue;
        }
        // SAFETY: the class id says it is a unit.
        let unit = unsafe { &*object.cast::<Unit>() };
        let controller = unit.controller.cast::<UnitServer>();
        if controller.is_null() {
            continue;
        }
        for &player in players {
            // SAFETY: a live player.
            let side = unsafe { (*player).number };
            if !is_living_enemy(unit, side) {
                continue;
            }
            // SAFETY: the unit's live controller; only the brain's address is taken.
            let brain = unsafe { &raw mut (*controller).brain };
            // SAFETY: the unit's live AI, on the game's thread as `SendPartyState` calls it.
            let target = unsafe { AiBrain::aggression_enemy(brain) };
            // SAFETY: a non-null target is a live engine object.
            if target.is_null() || unsafe { EngineObject::class_id(target) } != CLASS_UNIT {
                continue;
            }
            // SAFETY: the class id says it is a unit.
            if unsafe { (*target.cast::<Unit>()).player } == player.cast() {
                return Some(unit.map_object.transform.base.id);
            }
        }
    }
    None
}

/// Pauses the game on a trigger of eix.ini's `[AutoPause]`.
///
/// # Safety
/// On the game's thread, right after a server tick.
unsafe fn check() {
    let deaths = DEATHS
        .lock()
        .map(|mut d| std::mem::take(&mut *d))
        .unwrap_or_default();
    // A network game is never paused for one player.
    // SAFETY: the build was checked before WinMain, on the game's thread.
    if unsafe { is_multiplayer_client() } {
        return;
    }
    // SAFETY: as above.
    if unsafe { live_server() }.is_some() {
        return;
    }
    let Ok(mut state) = STATE.lock() else {
        return;
    };
    let triggers = state.triggers;
    // SAFETY: on the game's thread.
    let players = unsafe { players() };
    let mut reasons = Vec::new();
    if triggers.limits.any() {
        let mut party = Vec::new();
        for &player in &players {
            // SAFETY: a live player of the server's world.
            for unit in unsafe { units(player) } {
                // SAFETY: a live unit of the server's world.
                let vitals = unsafe { server_vitals(unit) };
                if !vitals.is_null() {
                    // SAFETY: the unit's live server vitals.
                    let v = unsafe { &*vitals };
                    // SAFETY: as above.
                    let id = unsafe { (*unit).map_object.transform.base.id };
                    party.push((id, v.hit_points, v.max_hit_points));
                }
            }
        }
        if let Some(id) = state.watch.check(triggers.limits, party.iter().copied())
            && let Some(&(_, hp, max)) = party.iter().find(|u| u.0 == id)
        {
            reasons.push(format!("unit {id} at {hp} of {max} hit points"));
        }
    }
    if triggers.needs_view() {
        let mut ids = Vec::new();
        for &player in &players {
            // SAFETY: a live player of the server's world.
            unsafe { in_view(player, &mut ids) };
        }
        if triggers.enemy_sighted
            && state.sighted.check(!ids.is_empty())
            && let Some(id) = ids.first()
        {
            reasons.push(format!("enemy {id} in view"));
        }
        if triggers.enemy_dead
            && let Some(id) = death_in_view(&deaths, &state.in_view)
        {
            reasons.push(format!("enemy {id} in view died"));
        }
        state.in_view = ids;
    }
    if triggers.enemy_aggro {
        // SAFETY: on the game's thread after the tick, the server world's live players.
        let found = unsafe { aggressor(&players) };
        if state.aggro.check(found.is_some())
            && let Some(id) = found
        {
            reasons.push(format!("enemy {id} turned on the party"));
        }
    }
    drop(state);
    if reasons.is_empty() {
        return;
    }
    log::info("autopause", &format!("{}: paused", reasons.join(", ")), "");
    // SAFETY: the client's world, constructed with its session in a game, on the game's thread.
    unsafe { World::send_pause(CLIENT_WORLD.as_ptr(), 1) };
}

/// A redirect of the call at `site` (its bytes `bytes`) to `to`, its old target kept in `old`.
fn redirect(
    label: &str,
    site: Site,
    bytes: [u8; 5],
    to: usize,
    old: &AtomicUsize,
) -> Result<Labelled, hook::HookError> {
    let (write, target) = hook::redirect_call(site, bytes, to)?;
    old.store(target.here(), Ordering::Relaxed);
    Ok(Labelled {
        label: label.to_owned(),
        write,
    })
}

/// Hooks the tick, and a unit's death for `EnemyDead`, when a trigger of eix.ini's `[AutoPause]` is on.
pub(crate) fn install(token: &BeforeWinMain, triggers: Triggers) {
    let limits = triggers.limits;
    let shown = format!(
        "{HEALTH_KEY} {}, {PERCENT_KEY} {}, {SIGHTED_KEY} {}, {AGGRO_KEY} {}, {DEAD_KEY} {}",
        limits.health,
        limits.percent,
        u8::from(triggers.enemy_sighted),
        u8::from(triggers.enemy_aggro),
        u8::from(triggers.enemy_dead)
    );
    if !triggers.any() {
        log::info("autopause", &format!("off: {shown}"), "");
        return;
    }
    log::info("autopause", &shown, "");
    if let Ok(mut state) = STATE.lock() {
        state.triggers = triggers;
    }
    let mut built = vec![redirect(
        "AutoPause tick",
        TICK_CALL,
        TICK_CALL_BYTES,
        tick as *const () as usize,
        &TICK,
    )];
    if triggers.enemy_dead {
        built.push(redirect(
            "AutoPause death",
            DEATH_CALL,
            DEATH_CALL_BYTES,
            set_dead as *const () as usize,
            &SET_DEAD,
        ));
    }
    let built = built
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into);
    let mut tally = Tally::default();
    apply_built(token, "autopause", "AutoPause", built, &mut tally);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_calls_go_to_the_servers_tick_and_the_death() {
        for (site, bytes, target) in [
            (TICK_CALL, TICK_CALL_BYTES, eix_sites::C_WORLD_SERVER_TICK),
            (
                DEATH_CALL,
                DEATH_CALL_BYTES,
                eix_sites::C_UNIT_SERVER_SET_DEAD,
            ),
        ] {
            let rel = i32::from_le_bytes(bytes[1..].try_into().unwrap());
            assert_eq!(
                Site::new((site.offset(5)).ru().wrapping_add_signed(rel as isize)),
                target
            );
        }
    }

    #[test]
    fn spelladdon_writes_nothing_at_the_calls() {
        for ours in [
            TICK_CALL..TICK_CALL.offset(TICK_CALL_BYTES.len()),
            DEATH_CALL..DEATH_CALL.offset(DEATH_CALL_BYTES.len()),
        ] {
            for (name, w) in spelladdon::writes() {
                assert!(
                    w.site().offset(w.size()) <= ours.start || ours.end <= w.site(),
                    "SpellAddon's {name} at {:#X}",
                    w.site()
                );
            }
        }
    }
}
