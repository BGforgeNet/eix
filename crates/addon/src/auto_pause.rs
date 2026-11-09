//! AutoPause, from eix.ini's `[AutoPause]` in the mod folder: in single player the game pauses when a party
//! member's health drops below a limit (core's [`eix_core::auto_pause`]).
//!
//! The check runs after each server tick: `CWorldServer::Update` 0x58DFF0 calls `CWorldServer::Tick` 0x58DCD0 at
//! 0x58E20B only while the server's world is not paused. It reads the server-side health of every unit of the
//! server world's players, and pauses as the pause key does, through the client's pause request.
#![expect(
    unsafe_code,
    reason = "hooks the server's tick and reads game.exe's players and units"
)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use eix_core::auto_pause::{HEALTH_KEY, Limits, PERCENT_KEY, SECTION, Watch};
use eix_engine::ansi;
use eix_engine::game::net::{is_multiplayer_client, live_server};
use eix_engine::game::player::{PlayerIterator, UnitMapNode};
use eix_engine::game::stl::HashTable;
use eix_engine::game::unit::server_vitals;
use eix_engine::game::world::{CLIENT_WORLD, SERVER_WORLD, World};
use eix_engine::hook;

use crate::game::BeforeWinMain;
use crate::groups::{Labelled, Tally, apply_built};
use crate::log;

/// The call of `CWorldServer::Tick` in `CWorldServer::Update`.
const TICK_CALL: usize = eix_sites::C_WORLD_SERVER_UPDATE__21B;
const TICK_CALL_BYTES: [u8; 5] = [0xE8, 0xC0, 0xFA, 0xFF, 0xFF];

/// `CWorldServer::Tick` (thiscall, `ret 4`); `Update` drops what it returns.
type Tick = unsafe extern "thiscall" fn(*mut World, i32) -> u32;

/// `CWorldServer::Tick`, the call's old target.
static TICK: AtomicUsize = AtomicUsize::new(0);
/// The limits and the party's state; the game's thread alone runs the tick.
static STATE: Mutex<(Limits, Watch)> = Mutex::new((
    Limits {
        health: 0,
        percent: 0,
    },
    Watch::new(),
));

/// The tick, then the check.
extern "thiscall" fn tick(world: *mut World, arg: i32) -> u32 {
    // SAFETY: TICK holds CWorldServer::Tick, of this type, once `install` ran; the call is redirected only after.
    let tick = unsafe { hook::as_fn::<Tick>(TICK.load(Ordering::Relaxed)) };
    // SAFETY: the game's own call, with its arguments.
    let result = unsafe { tick(world, arg) };
    // SAFETY: the game's thread, between two server ticks.
    unsafe { check() };
    result
}

/// Pauses the game when a party member's health dropped below a limit.
///
/// # Safety
/// On the game's thread, right after a server tick.
unsafe fn check() {
    // A network game is never paused for one player.
    // SAFETY: the build was checked before WinMain, on the game's thread.
    if unsafe { is_multiplayer_client() } {
        return;
    }
    // SAFETY: as above.
    if unsafe { live_server() }.is_some() {
        return;
    }
    let mut units = Vec::new();
    // SAFETY: the server's world is constructed at start-up.
    let list = unsafe { World::players(SERVER_WORLD.as_ptr()) };
    if !list.is_null() {
        let mut it = PlayerIterator::new();
        // SAFETY: the server world's live player list.
        let mut player = unsafe { it.first(list) };
        while !player.is_null() {
            let add = |node: *mut UnitMapNode| {
                // SAFETY: `for_each` passes live nodes, whose units are live units of the server's world.
                let n = unsafe { &*node };
                let (id, unit) = (n.id, n.unit);
                // SAFETY: as above.
                let vitals = unsafe { server_vitals(unit) };
                if !vitals.is_null() {
                    // SAFETY: the unit's live server vitals.
                    let v = unsafe { &*vitals };
                    units.push((id, v.hit_points, v.max_hit_points));
                }
            };
            // SAFETY: a live player of the list; only the table's address is taken.
            let table = unsafe { &raw const (*player).units };
            // SAFETY: the player's unit table; reading it leaves it unchanged.
            unsafe { HashTable::for_each(table, add) };
            // SAFETY: the walk was started by `first`.
            player = unsafe { it.next() };
        }
        // SAFETY: as above.
        unsafe { it.release() };
    }
    let Ok(mut state) = STATE.lock() else {
        return;
    };
    let (limits, watch) = &mut *state;
    let Some(id) = watch.check(*limits, units.iter().copied()) else {
        return;
    };
    if let Some(&(_, hp, max)) = units.iter().find(|u| u.0 == id) {
        log::info(
            "autopause",
            &format!("unit {id} at {hp} of {max} hit points: paused"),
            "",
        );
    }
    // SAFETY: the client's world, constructed with its session in a game, on the game's thread.
    unsafe { World::send_pause(CLIENT_WORLD.as_ptr(), 1) };
}

/// Reads `[AutoPause]` from the mod folder's eix.ini (`mod_dir` ends in `\`, empty for no mod) and hooks the tick
/// when a limit is on.
pub(crate) fn install(token: &BeforeWinMain, mod_dir: &[u8]) {
    if mod_dir.is_empty() {
        log::info("autopause", "off: no mod selected", "");
        return;
    }
    let file = [mod_dir, b"eix.ini"].concat();
    let shown = String::from_utf8_lossy(&file).into_owned();
    let (limits, warnings) = Limits::from_values(
        ansi::profile_int(SECTION.as_bytes(), HEALTH_KEY.as_bytes(), 0, &file),
        ansi::profile_int(SECTION.as_bytes(), PERCENT_KEY.as_bytes(), 0, &file),
    );
    for w in &warnings {
        log::warn("autopause", w, &shown);
    }
    if !limits.any() {
        log::info(
            "autopause",
            &format!("off: {HEALTH_KEY} and {PERCENT_KEY} are 0"),
            &shown,
        );
        return;
    }
    log::info(
        "autopause",
        &format!(
            "{HEALTH_KEY} {}, {PERCENT_KEY} {}",
            limits.health, limits.percent
        ),
        &shown,
    );
    if let Ok(mut state) = STATE.lock() {
        state.0 = limits;
    }
    let built = hook::redirect_call(TICK_CALL, TICK_CALL_BYTES, tick as *const () as usize)
        .map(|(write, old)| {
            TICK.store(old, Ordering::Relaxed);
            vec![Labelled {
                label: "AutoPause".to_owned(),
                write,
            }]
        })
        .map_err(Into::into);
    let mut tally = Tally::default();
    apply_built(token, "autopause", "AutoPause", built, &mut tally);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_call_goes_to_the_servers_tick() {
        let rel = i32::from_le_bytes(TICK_CALL_BYTES[1..].try_into().unwrap());
        assert_eq!(
            (TICK_CALL + 5).wrapping_add_signed(rel as isize),
            eix_sites::C_WORLD_SERVER_TICK
        );
    }

    #[test]
    fn spelladdon_writes_nothing_at_the_call() {
        let ours = TICK_CALL..TICK_CALL + TICK_CALL_BYTES.len();
        for (name, w) in spelladdon::writes() {
            assert!(
                w.site() + w.size() <= ours.start || ours.end <= w.site(),
                "SpellAddon's {name} at {:#X}",
                w.site()
            );
        }
    }
}
