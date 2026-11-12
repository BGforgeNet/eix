//! AttackAnimSpeed: attack animations stretch to the attack interval and cast animations to the cast time. The unit's
//! `animationSpeed` (CMapObject+0x178) is set by the new `CUnit::ActionSpeed` (#4) and by the cast steps (#3), sent
//! to clients in the unit's animation field group (#1, #2), and scales the client's animation unless paused (#0).
#![expect(
    unsafe_code,
    reason = "a naked handler on the game's x87 stack, and the hooks read and write the game's units"
)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use eix_core::x86_flags;
use eix_engine::game::net::DeltaStream;
use eix_engine::game::object::{CLASS_UNIT_CLIENT_SPECIFIC, EngineObject};
use eix_engine::game::unit::{
    ANIMATION_ACTION_MASK, ANIMATION_ATTACK, ANIMATION_CAST, AnimationEntry, Unit, UnitServer,
    VALUE_ATTACK_INTERVAL,
};
use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, Mode};
use eix_engine::x87::Ext;

use super::{BuildError, Labelled, Setup, bytes};

/// The original `CUnit::ActionSpeed`, through its trampoline.
static ACTION_SPEED: AtomicUsize = AtomicUsize::new(0);
/// The translated continuation after the cast-speed instructions.
static CAST_SPEED_RESUME: AtomicUsize = AtomicUsize::new(0);

type ActionSpeed = unsafe extern "thiscall" fn(*mut Unit, *mut AnimationEntry) -> f32;

/// The unit's `animationSpeed` field.
fn speed_field(unit: *mut Unit) -> *mut f32 {
    // SAFETY: only the address is computed; `unit` points at a unit.
    unsafe { &raw mut (*unit).map_object.animation_speed }
}

/// #4, the new `CUnit::ActionSpeed(entry)`: attacks take the attack interval, casts the speed set by #3; every other
/// animation the game's speed. Each result but a cast's is also stored as the unit's `animationSpeed`.
extern "thiscall" fn action_speed(unit: *mut Unit, entry: *mut AnimationEntry) -> f32 {
    let (key, frames) = {
        // SAFETY: the game asks with a live unit and one of its animation entries, on its thread.
        let e = unsafe { &*entry };
        (e.key, e.frame_count)
    };
    let field = speed_field(unit);
    let speed = match key & ANIMATION_ACTION_MASK {
        ANIMATION_CAST => {
            // SAFETY: the unit is live (above).
            return unsafe { field.read() };
        }
        ANIMATION_ATTACK => {
            // SAFETY: as above.
            let controller = unsafe { (*unit).controller };
            // SAFETY: a live unit's controller is a live engine object.
            if unsafe { EngineObject::class_id(controller) } == CLASS_UNIT_CLIENT_SPECIFIC {
                // A client's units play at the speed the server sent (#2).
                // SAFETY: the unit is live (above).
                return unsafe { field.read() };
            }
            // SAFETY: the unit is live (above).
            let slot = unsafe { (*unit).active_weapon };
            // SAFETY: as above.
            let weapons = unsafe { (*unit).weapons };
            let weapon = weapons
                .get(slot as usize)
                .copied()
                .unwrap_or(core::ptr::null_mut());
            // SAFETY: not a client controller, so the server's; the weapon is the unit's own or null.
            let ticks = unsafe {
                UnitServer::scaled_attack_value(controller.cast(), VALUE_ATTACK_INTERVAL, weapon)
            };
            if ticks == 0 {
                1.0
            } else {
                Ext::from_u32(frames).fdiv(ticks.into()).to_f32()
            }
        }
        _ => {
            // SAFETY: ACTION_SPEED holds the trampoline to the original, stored before the site was written.
            let original: ActionSpeed =
                unsafe { hook::as_fn_at(ACTION_SPEED.load(Ordering::Relaxed)) };
            // SAFETY: the game's arguments, passed on.
            unsafe { original(unit, entry) }
        }
    };
    // SAFETY: the unit is live (above).
    unsafe { field.write(speed) };
    speed
}

/// #1, `CUnit::WriteNetState` 0x52EE76 (EDI the stream, ESI the unit): the animation field group gains the speed
/// before the replaced `mov ecx, edi; call EndField`.
extern "C" fn write_speed(_: usize, ctx: &mut Context) {
    let stream = core::ptr::with_exposed_provenance_mut::<DeltaStream>(ctx.edi as usize);
    let unit = core::ptr::with_exposed_provenance_mut::<Unit>(ctx.esi as usize);
    // SAFETY: at 0x52EE76 the stream has the animation field group open and ESI is the unit writing its state.
    unsafe { DeltaStream::write_delta(stream, speed_field(unit).cast::<c_void>(), 4) };
    ctx.ecx = ctx.edi;
    // SAFETY: as above; the replaced call closes the group.
    ctx.eax = unsafe { DeltaStream::end_field(stream) };
}

/// #2, `CUnit::ReadNetState` 0x52F3A9: the speed is read after the group's four fields, then the replaced
/// `cmp dword [esi+0x25c], 1` sets the flags for the game's `jne`.
extern "C" fn read_speed(_: usize, ctx: &mut Context) {
    let stream = core::ptr::with_exposed_provenance_mut::<DeltaStream>(ctx.edi as usize);
    let unit = core::ptr::with_exposed_provenance_mut::<Unit>(ctx.esi as usize);
    // SAFETY: at 0x52F3A9 EDI is the stream positioned after the group's four fields and ESI the unit reading.
    unsafe { DeltaStream::read_delta(stream, speed_field(unit).cast::<c_void>(), 4) };
    // SAFETY: as above.
    let mode = unsafe { (*unit).motion_mode };
    ctx.eflags = x86_flags::cmp(ctx.eflags, mode as u32, 1);
}

/// #3, `CUnitServer::BuildCastSteps` 0x55071F, jumped to with the animation's frame count in ST0: the unit's speed
/// becomes `frames / [esp+0x2c]` (the cast's tick count) or 1 for no ticks, then the replaced `push eax; fstp dword
/// [esp+0x24]` run and execution goes on at 0x550724. EDI is the controller, whose +0x1C is the unit.
#[unsafe(naked)]
extern "C" fn cast_speed() {
    core::arch::naked_asm!(
        // A cast of no ticks gets speed 1, as an attack of no ticks does (the original divides by zero, giving an
        // infinite speed). The flags are free: the game's next instruction is a call.
        "cmp dword ptr [esp + 0x2c], 0",
        "jne 2f",
        "fld1",
        "jmp 3f",
        "2:",
        "fld st(0)",
        "fidiv dword ptr [esp + 0x2c]",
        "3:",
        "push edx",
        "mov edx, dword ptr [edi + 0x1c]",
        "fstp dword ptr [edx + 0x178]",
        "pop edx",
        "push eax",
        "fstp dword ptr [esp + 0x24]",
        "push dword ptr [{resume}]",
        "ret",
        resume = sym CAST_SPEED_RESUME,
    );
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    CAST_SPEED_RESUME.store(
        eix_sites::C_UNIT_SERVER_BUILD_CAST_STEPS__104.here(),
        Ordering::Relaxed,
    );
    let (replace, trampoline) = hook::replace_function(
        eix_sites::C_UNIT_ACTION_SPEED,
        &[0x8B, 0x44, 0x24, 0x04, 0x8B, 0x40, 0x14],
        action_speed as *const () as usize,
    )?;
    ACTION_SPEED.store(trampoline, Ordering::Relaxed);
    Ok(vec![
        bytes(
            "AttackAnimSpeed #0",
            eix_sites::C_UNIT_CLIENT_SPECIFIC_ADVANCE_TIME__141,
            &[0x83, 0xF8, 0x02, 0x75, 0x12],
            &[0x83, 0xF8, 0x01, 0x74, 0x12],
        )?,
        Labelled {
            label: "AttackAnimSpeed #1".to_owned(),
            write: hook::context_hook(
                eix_sites::C_UNIT_WRITE_NET_STATE__206,
                &[0x8B, 0xCF, 0xE8, 0xA3, 0xED, 0xF4, 0xFF],
                Mode::Drop,
                write_speed,
                0,
            )?,
        },
        Labelled {
            label: "AttackAnimSpeed #2".to_owned(),
            write: hook::context_hook(
                eix_sites::C_UNIT_READ_NET_STATE__419,
                &[0x83, 0xBE, 0x5C, 0x02, 0x00, 0x00, 0x01],
                Mode::Drop,
                read_speed,
                0,
            )?,
        },
        Labelled {
            label: "AttackAnimSpeed #3".to_owned(),
            write: SiteWrite::jmp(
                eix_sites::C_UNIT_SERVER_BUILD_CAST_STEPS__FF,
                &[0x50, 0xD9, 0x5C, 0x24, 0x24],
                cast_speed as *const () as usize,
            )?,
        },
        Labelled {
            label: "AttackAnimSpeed #4".to_owned(),
            write: replace,
        },
    ])
}
