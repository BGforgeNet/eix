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
//!   `EnemySighted` waits until the client shows one of them (`sight`): it draws a unit only once its party state
//!   marks it seen, and fades it in over its own ticks from then, which a pause would stop. So the fade-in of every
//!   shown enemy in view goes on over the paused frames.
//! - Aggression: each living hostile unit of the server world whose AI aggression is aimed at a party unit, as
//!   `CPlayerServer::SendPartyState` finds it for the danger state (0x66E666). `EnemyAggro` also waits until the
//!   client shows the aggressor, and fades it in so: the server's AI turns on the party before the client has the
//!   unit at all. It does not pause when the party engaged every aggressor first: a party unit's action is an attack
//!   at it, or the cast of an attack spell at it (`CUnitServer::IsFighting` 0x54B580, the target at +0x198), now or
//!   within core's [`ENGAGED_TICKS`]. The game keeps no attacker for later: a unit's AI notes the last one on a hit
//!   (`AIBrain` +0x30) and drops it at its next update (0x5D5697), before the aggression shows.
//! - Fade-in: `GameMain` 0x456440 calls `FrameStep` 0x458010 (0x4575FD) on every frame, paused or not; before it,
//!   each frame of the pause raises the fade of those enemies by the frame's time at the client ticks' pace (core's
//!   [`paused_fade`]), and draws it. The first frame after the pause, or a load, ends it: `UpdateClient` goes on
//!   from the fade reached.
//! - Deaths: `CUnitServer::OnDamaged` is the one caller of `CUnitServer::SetDead` 0x546720 (0x553BB0); its call is
//!   redirected to note the unit, and the next check pauses when the unit was in view at the check before.
#![expect(
    unsafe_code,
    reason = "hooks the server's tick, a unit's death and the frame, and reads game.exe's players and units"
)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use eix_core::auto_pause::{
    AGGRO_KEY, DEAD_KEY, ENGAGED_TICKS, Engaged, HEALTH_KEY, Limits, MAX_WAIT, PERCENT_KEY,
    ReadyOnset, SIGHTED_KEY, Start, Told, Triggers, Watch, death_in_view, is_enemy, paused_fade,
};
use eix_engine::game::flow::{DRAW_ALL, UPDATE_PERIOD};
use eix_engine::game::net::{is_multiplayer_client, live_server};
use eix_engine::game::object::{CLASS_UNIT, EngineObject};
use eix_engine::game::player::{Player, PlayerIterator, UnitMapNode};
use eix_engine::game::stl::HashTable;
use eix_engine::game::unit::{
    AiBrain, FLAG_REMOVED, FLAG_SEEN, Unit, UnitClient, UnitServer, server_vitals,
};
use eix_engine::game::world::{
    CLIENT_WORLD, ClientWorld, ObjectMap, SERVER_OBJECTS, SERVER_WORLD, World,
};
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
/// The call of `FrameStep` in `GameMain`, once a frame.
const FRAME_CALL: Site = eix_sites::GAME_MAIN__11BD;
const FRAME_CALL_BYTES: [u8; 5] = [0xE8, 0x0E, 0x0A, 0x00, 0x00];

/// `CWorldServer::Tick` (thiscall, `ret 4`); `Update` drops what it returns.
type Tick = unsafe extern "thiscall" fn(*mut World, i32) -> u32;
/// `CUnitServer::SetDead` (thiscall, `ret 4`): the dead flag to set.
type SetDead = unsafe extern "thiscall" fn(*mut UnitServer, i32);
/// `FrameStep` (fastcall, two flags in `cl` and `dl`, plain `ret`); `GameMain` reads nothing it returns.
type FrameStep = unsafe extern "fastcall" fn(u8, u8);

/// `CWorldServer::Tick`, the call's old target.
static TICK: AtomicUsize = AtomicUsize::new(0);
/// `CUnitServer::SetDead`, the call's old target.
static SET_DEAD: AtomicUsize = AtomicUsize::new(0);
/// `FrameStep`, the call's old target.
static FRAME_STEP: AtomicUsize = AtomicUsize::new(0);

/// The enemies a pause is about, whose fade-in goes on over its frames; the game's thread alone uses it.
#[derive(Debug)]
struct Fading {
    /// Their unit ids, found anew each frame.
    ids: Vec<i32>,
    /// When the last frame ran.
    last: Option<Instant>,
    /// The client's tick count the last frame saw: it falls only when a zone loads.
    ticks: i32,
    /// Whether a frame saw the game paused.
    paused: bool,
}

static FADING: Mutex<Fading> = Mutex::new(Fading {
    ids: Vec::new(),
    last: None,
    ticks: 0,
    paused: false,
});

/// What the checks keep between ticks; the game's thread alone runs them.
struct State {
    triggers: Triggers,
    /// The party's health.
    watch: Watch,
    /// Enemies in view, told once one is shown.
    sighted: ReadyOnset,
    /// Enemies aggressive to the party, told once the aggressor is shown, unless the party engaged them first.
    aggro: ReadyOnset,
    /// The units the party fights or fought lately.
    engaged: Engaged,
    /// The enemies in view at the last check.
    in_view: Vec<i32>,
}

static STATE: Mutex<State> = Mutex::new(State {
    triggers: Triggers {
        limits: Limits::OFF,
        enemy_sighted: false,
        enemy_aggro: false,
        enemy_dead: false,
    },
    watch: Watch::new(),
    sighted: ReadyOnset::new(),
    aggro: ReadyOnset::new(),
    engaged: Engaged::new(),
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

/// The pause's fade-in for this frame, then the frame.
extern "fastcall" fn frame_step(a: u8, b: u8) {
    // SAFETY: the game's thread, between two frames.
    unsafe { fade_paused() };
    // SAFETY: FRAME_STEP holds FrameStep, of this type, once `install` ran; the call is redirected only after.
    let frame_step = unsafe { hook::as_fn_at::<FrameStep>(FRAME_STEP.load(Ordering::Relaxed)) };
    // SAFETY: the game's own call, with its arguments.
    unsafe { frame_step(a, b) };
}

/// Raises the fade-in of the enemies the pause is about by the time since the last frame, while the game stays
/// paused, and draws it. Ends when they are faded in or gone, on the first frame after the pause (from then the
/// game's own ticks take the fade on), or after a load.
///
/// # Safety
/// On the game's thread, between two frames, with the client's world constructed.
unsafe fn fade_paused() {
    let Ok(mut fading) = FADING.lock() else {
        return;
    };
    if fading.ids.is_empty() {
        return;
    }
    let world = client_world();
    // SAFETY: the client's world is constructed at start-up; its pause flag, read only.
    let paused = unsafe { (*world).base.paused } != 0;
    // SAFETY: as above; its tick count, read only.
    let ticks = unsafe { (*world).ticks };
    let now = Instant::now();
    if !paused {
        if fading.paused {
            fading.ids.clear();
            return;
        }
        // The pause is asked for but not yet on: the client's ticks still fade the units in.
        fading.ticks = ticks;
        fading.last = Some(now);
        return;
    }
    // The client's tick counts up first thing, so the count goes on while the flag is set (its units are not
    // updated then, and the fade stays); it falls only when a zone loads.
    if ticks < fading.ticks {
        fading.ids.clear();
        return;
    }
    fading.ticks = ticks;
    fading.paused = true;
    let elapsed = fading.last.map_or_else(Default::default, |last| now - last);
    fading.last = Some(now);
    // SAFETY: the game speed, read only.
    let period = unsafe { UPDATE_PERIOD.read() };
    fading.ids.retain(|&id| {
        // SAFETY: on the game's thread between frames; a unit gone with a load is not found.
        let (sight, client) = unsafe { look(id) };
        let Sight::Present { shown: true, fade } = sight else {
            return false;
        };
        if client.is_null() || fade >= 1.0 {
            return false;
        }
        let fade = paused_fade(fade, elapsed, period);
        // SAFETY: the live unit's client controller, on the game's thread.
        unsafe { UnitClient::set_fade(client, fade) };
        fade < 1.0
    });
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

/// How the client shows a unit, as `CWorldClient::CollectVisible` 0x56A990 decides it and the fade-in goes.
#[derive(Debug, Clone, Copy)]
enum Sight {
    /// The unit is not in the client's world yet: the server's AI sees a unit before the client has it.
    Absent,
    /// The unit is in the client's world.
    Present {
        /// Whether the client draws it: every unit, or (in the party's view mode) one the party state marks seen.
        shown: bool,
        /// Its fade-in alpha, which advances on client ticks only, so a pause stops it.
        fade: f32,
    },
}

impl Sight {
    /// Whether the client draws the unit, faded in or not.
    fn shown(self) -> bool {
        matches!(self, Self::Present { shown: true, .. })
    }
}

impl std::fmt::Display for Sight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Absent => f.write_str("not in the client world"),
            Self::Present { shown, fade } => {
                write!(
                    f,
                    "fade {fade:.2}, {}",
                    if shown { "shown" } else { "hidden" }
                )
            }
        }
    }
}

/// The client's world.
fn client_world() -> *mut ClientWorld {
    CLIENT_WORLD.as_ptr().cast()
}

/// The client's ticks since its zone loaded.
///
/// # Safety
/// On the game's thread, with the client's world constructed.
unsafe fn client_ticks() -> i32 {
    // SAFETY: the caller guarantees the constructed client world; its tick count, read only.
    unsafe { (*client_world()).ticks }
}

/// How the client shows its unit `id`. A unit loaded with the zone is in the client's world, faded in, from the
/// start; in the party's view mode it is drawn only once a party state lists it seen, which restarts its fade-in.
///
/// # Safety
/// On the game's thread, with the client's world constructed and a zone loaded.
unsafe fn sight(id: i32) -> Sight {
    // SAFETY: the caller's guarantee.
    unsafe { look(id) }.0
}

/// How the client shows its unit `id`, and the unit's client controller (null when absent or without one).
///
/// # Safety
/// As for [`sight`].
unsafe fn look(id: i32) -> (Sight, *mut UnitClient) {
    let world = client_world();
    // SAFETY: the caller guarantees the client's world with a zone.
    let unit = unsafe { World::find_unit(world.cast(), id) };
    if unit.is_null() {
        return (Sight::Absent, std::ptr::null_mut());
    }
    // SAFETY: the client world's live local player, or null before a session.
    let local = unsafe { (*world).local };
    // SAFETY: the flag and the local player's view mode, read only, as `CollectVisible` reads them.
    let all =
        unsafe { DRAW_ALL.read() } == 0 || local.is_null() || unsafe { (*local).view_mode } != 1;
    // SAFETY: a live unit.
    let seen = unsafe { (*unit).map_object.transform.base.flags } & FLAG_SEEN != 0;
    // SAFETY: a live unit; on the client its controller is a `CUnitClientSpecific` or null.
    let client = unsafe { (*unit).controller }.cast::<UnitClient>();
    let fade = if client.is_null() {
        1.0
    } else {
        // SAFETY: the controller's live fade, read only.
        unsafe { (*client).fade }
    };
    let sight = Sight::Present {
        shown: all || seen,
        fade,
    };
    (sight, client)
}

/// Adds the client's unit `id` to `fading` when the client shows it and its fade-in is under way, so the pause's
/// frames fade it in. Returns its sight.
///
/// # Safety
/// As for [`sight`].
unsafe fn fade_in(id: i32, fading: &mut Vec<i32>) -> Sight {
    // SAFETY: the caller's guarantee.
    let (sight, client) = unsafe { look(id) };
    if let Sight::Present { shown: true, fade } = sight
        && fade < 1.0
        && !client.is_null()
        && !fading.contains(&id)
    {
        fading.push(id);
    }
    sight
}

/// The pause line's account of a told start: the enemy's sight at the pause, whether the pause fades it in, and
/// whether the wait ran out.
fn account(sight: Sight, told: Told) -> String {
    let s = if let Sight::Present { shown: true, fade } = sight
        && fade < 1.0
    {
        format!("{sight}, fading in")
    } else {
        format!("{sight}")
    };
    match told {
        Told::Ready => s,
        Told::Capped => format!("{s}; not shown after {MAX_WAIT} client ticks"),
    }
}

/// The units the party fights: each living unit of a player whose action is an attack, or the cast of an attack
/// spell, at a unit (`CUnitServer::IsFighting`, as `SendPartyState` asks it of the party), as (its id, the target's).
///
/// # Safety
/// As for [`aggressors`].
unsafe fn fights(players: &[*mut Player]) -> Vec<(i32, i32)> {
    let mut fights = Vec::new();
    for &player in players {
        // SAFETY: a live player of the server's world.
        for unit in unsafe { units(player) } {
            // SAFETY: a live unit of the player's table.
            let u = unsafe { &*unit };
            let controller = u.controller.cast::<UnitServer>();
            if controller.is_null() || u.dead != 0 {
                continue;
            }
            // SAFETY: the unit's live server controller, on the game's thread.
            if !unsafe { UnitServer::is_fighting(controller) } {
                continue;
            }
            // SAFETY: the controller's live action; its target is counted, so live while set.
            let target = unsafe { (*controller).action.target };
            // SAFETY: a non-null target is a live engine object.
            if target.is_null() || unsafe { EngineObject::class_id(target) } != CLASS_UNIT {
                continue;
            }
            // SAFETY: the class id says it is a unit.
            let target = unsafe { (*target.cast::<Unit>()).map_object.transform.base.id };
            fights.push((u.map_object.transform.base.id, target));
        }
    }
    fights
}

/// The living enemies of a player whose AI aggression is aimed at one of that player's units.
///
/// # Safety
/// On the game's thread right after a server tick, `players` the server world's live players.
unsafe fn aggressors(players: &[*mut Player]) -> Vec<i32> {
    let mut found = Vec::new();
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
                found.push(unit.map_object.transform.base.id);
                break;
            }
        }
    }
    found
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
    // SAFETY: on the game's thread, the client's world constructed.
    let ticks = unsafe { client_ticks() };
    let mut reasons = Vec::new();
    let mut fading = Vec::new();
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
        if let Some((id, crossed)) = state.watch.check(triggers.limits, party.iter().copied())
            && let Some(&(_, hp, max)) = party.iter().find(|u| u.0 == id)
        {
            reasons.push(format!(
                "unit {id} at {hp} of {max} hit points, below {crossed}"
            ));
        }
    }
    if triggers.needs_view() {
        let mut ids = Vec::new();
        for &player in &players {
            // SAFETY: a live player of the server's world.
            unsafe { in_view(player, &mut ids) };
        }
        if triggers.enemy_sighted {
            let mut shown = None;
            let told = state.sighted.check(!ids.is_empty(), ticks, || {
                shown = ids.iter().copied().find(|&id| {
                    // SAFETY: on the game's thread after a server tick, the client's world constructed.
                    unsafe { sight(id) }.shown()
                });
                shown.is_some()
            });
            if let Some(told) = told
                && let Some(id) = shown.or_else(|| ids.first().copied())
            {
                let mut sight = None;
                for &other in &ids {
                    // SAFETY: as above.
                    let s = unsafe { fade_in(other, &mut fading) };
                    if other == id {
                        sight = Some(s);
                    }
                }
                let account = account(sight.unwrap_or(Sight::Absent), told);
                reasons.push(format!("enemy {id} in view ({account})"));
            }
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
        let fights = unsafe { fights(&players) };
        state.engaged.tick(fights.iter().map(|&(_, target)| target));
        // SAFETY: as above.
        let found = unsafe { aggressors(&players) };
        // The enemy a pause names: the first the party did not engage.
        let named = found
            .iter()
            .copied()
            .find(|&id| !state.engaged.contains(id))
            .or_else(|| found.first().copied());
        let provoked = found.iter().all(|&id| state.engaged.contains(id));
        let start = state.aggro.check_provoked(
            !found.is_empty(),
            ticks,
            || provoked,
            || {
                // SAFETY: on the game's thread after a server tick, the client's world constructed.
                named.is_some_and(|id| unsafe { sight(id) }.shown())
            },
        );
        match (start, named) {
            (Some(Start::Told(told)), Some(id)) => {
                // SAFETY: as above.
                let account = account(unsafe { fade_in(id, &mut fading) }, told);
                reasons.push(format!("enemy {id} turned on the party ({account})"));
            }
            (Some(Start::Provoked), Some(id)) => {
                let by = fights
                    .iter()
                    .find(|&&(_, target)| target == id)
                    .map_or_else(
                        || format!("fought it within {ENGAGED_TICKS} server ticks"),
                        |&(unit, _)| format!("unit {unit} fights it"),
                    );
                log::info(
                    "autopause",
                    &format!(
                        "enemy {id} turned on the party, which engaged it first ({by}): no pause"
                    ),
                    "",
                );
            }
            _ => {}
        }
    }
    drop(state);
    if reasons.is_empty() {
        return;
    }
    log::info("autopause", &format!("{}: paused", reasons.join(", ")), "");
    if let Ok(mut f) = FADING.lock() {
        *f = Fading {
            ids: fading,
            last: Some(Instant::now()),
            ticks,
            paused: false,
        };
    }
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
    if triggers.enemy_sighted || triggers.enemy_aggro {
        built.push(redirect(
            "AutoPause frame",
            FRAME_CALL,
            FRAME_CALL_BYTES,
            frame_step as *const () as usize,
            &FRAME_STEP,
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
    fn the_calls_go_to_the_servers_tick_the_death_and_the_frame() {
        for (site, bytes, target) in [
            (TICK_CALL, TICK_CALL_BYTES, eix_sites::C_WORLD_SERVER_TICK),
            (
                DEATH_CALL,
                DEATH_CALL_BYTES,
                eix_sites::C_UNIT_SERVER_SET_DEAD,
            ),
            (FRAME_CALL, FRAME_CALL_BYTES, eix_sites::FRAME_STEP),
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
            FRAME_CALL..FRAME_CALL.offset(FRAME_CALL_BYTES.len()),
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
