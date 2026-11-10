//! The game flow's static calls (sounds, camera recording), the game's random generator and the draw-all flag.
//!
//! Names follow ei-rebuild.
#![expect(unsafe_code, reason = "calls into game.exe's game flow and generator")]

use core::ffi::{CStr, c_void};

use super::{Global, code};

/// The object the game passes to its game-flow calls (`ScriptRun::Execute` 0x4B9A36 loads it into ECX).
fn game_flow() -> *mut c_void {
    core::ptr::with_exposed_provenance_mut(eix_sites::here(eix_sites::DATA_7AFEA5))
}

/// Stops the sounds playing: `GameStopSounds` 0x578920 (thiscall, one flag on the stack, `ret 4`).
///
/// # Safety
/// The process must be game.exe RU 1.06, on the game's thread.
pub unsafe fn stop_sounds(all: bool) {
    // SAFETY: 0x578920 is `void __thiscall (int)`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void, u32)>(eix_sites::GAME_STOP_SOUNDS)
    };
    // SAFETY: the caller guarantees the build and the thread.
    unsafe { f(game_flow(), u32::from(all)) }
}

/// Stops the ambient sound: `GameStopAmbience` 0x578940 (thiscall, plain `ret`).
///
/// # Safety
/// As for [`stop_sounds`].
pub unsafe fn stop_ambience() {
    // SAFETY: 0x578940 is `void __thiscall ()`.
    let f =
        unsafe { code::<unsafe extern "thiscall" fn(*mut c_void)>(eix_sites::GAME_STOP_AMBIENCE) };
    // SAFETY: as for `stop_sounds`.
    unsafe { f(game_flow()) }
}

/// Starts recording the camera's path into `file`: `GameStartCameraRecording` 0x5787F0 (thiscall, `ret 4`).
///
/// # Safety
/// As for [`stop_sounds`].
pub unsafe fn start_camera_recording(file: &CStr) {
    // SAFETY: 0x5787F0 is `void __thiscall (const char*)`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void, *const u8)>(
            eix_sites::GAME_START_CAMERA_RECORDING,
        )
    };
    // SAFETY: as for `stop_sounds`; `file` is NUL-terminated.
    unsafe { f(game_flow(), file.as_ptr().cast()) }
}

/// Stops the camera recording: `GameStopCameraRecording` 0x5787D0 (thiscall, plain `ret`).
///
/// # Safety
/// As for [`stop_sounds`].
pub unsafe fn stop_camera_recording() {
    // SAFETY: 0x5787D0 is `void __thiscall ()`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void)>(eix_sites::GAME_STOP_CAMERA_RECORDING)
    };
    // SAFETY: as for `stop_sounds`.
    unsafe { f(game_flow()) }
}

/// The game's random generator (`IsaacRandom`), constructed by its static initialiser; `HooverCollectLoot` 0x461E06
/// draws from it.
fn random() -> *mut c_void {
    core::ptr::with_exposed_provenance_mut(eix_sites::here(eix_sites::G_RANDOM))
}

/// The generator's next value: `IsaacRandom::Next` 0x4667F0 (thiscall, plain `ret`).
///
/// # Safety
/// As for [`stop_sounds`].
pub unsafe fn random_next() -> u32 {
    // SAFETY: 0x4667F0 is `unsigned __thiscall ()`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void) -> u32>(eix_sites::ISAAC_RANDOM_NEXT)
    };
    // SAFETY: as for `stop_sounds`.
    unsafe { f(random()) }
}

/// A value between `low` and `high` from the generator: `IsaacRandom::Range` 0x488970 (thiscall, two floats on the
/// stack, `ret 8`, the result in ST0).
///
/// # Safety
/// As for [`stop_sounds`].
pub unsafe fn random_range(low: f32, high: f32) -> f32 {
    // SAFETY: 0x488970 is `float __thiscall (float, float)`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut c_void, f32, f32) -> f32>(
            eix_sites::ISAAC_RANDOM_RANGE,
        )
    };
    // SAFETY: as for `stop_sounds`.
    unsafe { f(random(), low, high) }
}

/// The draw-all flag (ei-rebuild's `drawVisibleOnly`): `CWorldClient::CollectVisible` 0x56A9A8 reads it;
/// `GameFlow::StartZone` 0x57C6AF sets it for the main menu.
pub const DRAW_ALL: Global<u32> = Global::at(eix_sites::DATA_79BE6C);

/// The game speed (`g_updatePeriod`): milliseconds per game tick, which the clock button writes
/// (`ClockMouseUpVerify` 0x648E28) and the game's timing reads (0x457257).
pub const UPDATE_PERIOD: Global<i32> = Global::at(eix_sites::DATA_78FF34);

/// The difficulty level: `AttackParams::ScaleForDifficulty` 0x52E0DE indexes its factors by it;
/// `COptionsWindow::Apply` 0x652700 writes it.
pub const DIFFICULTY: Global<i32> = Global::at(eix_sites::DATA_79BEC0);

/// Sets player 0's camp variable "constr.current" to `value`: `GameSetConstructionVariable` 0x578560
/// (stdcall, the float on the stack, `ret 4`), which the battle screen calls before it leaves for the camp
/// (0x62FE70).
///
/// # Safety
/// As for [`stop_sounds`]; the server world must be constructed.
pub unsafe fn set_construction_variable(value: f32) {
    // SAFETY: 0x578560 is `void __stdcall (float)`; it reads no ECX.
    let f = unsafe {
        code::<unsafe extern "stdcall" fn(f32)>(eix_sites::GAME_SET_CONSTRUCTION_VARIABLE)
    };
    // SAFETY: as for `stop_sounds`.
    unsafe { f(value) }
}

/// Leaves the zone for `target` (5: the camp): `GameSendLeave04` 0x578B00 (stdcall, the target on the stack,
/// `ret 4`), which sends the request through the client world's local view (0x578B11).
///
/// # Safety
/// As for [`stop_sounds`]; the client world must be constructed.
pub unsafe fn send_leave(target: i32) {
    // SAFETY: 0x578B00 is `void __stdcall (int)`; it loads ECX itself.
    let f = unsafe { code::<unsafe extern "stdcall" fn(i32)>(eix_sites::GAME_SEND_LEAVE_04) };
    // SAFETY: as for `stop_sounds`.
    unsafe { f(target) }
}
