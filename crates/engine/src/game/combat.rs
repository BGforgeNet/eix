//! Combat: the attack parameters a unit's controller reports.
//!
//! Names follow ei-rebuild.
#![expect(unsafe_code, reason = "calls into game.exe's combat code")]

use core::mem::offset_of;

use super::code;

/// `AttackParams`: what `GetAttackParams` (server 0x555A00, client 0x538960) fills for a unit's attack.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttackParams {
    _unknown0: [u8; 0x14],
    /// The attack roll's random range: `CUnitServer::GetAttackParams` copies the stats' `combat[2]` here
    /// (0x555BB4).
    pub attack_spread: f32,
    /// Attack skill: `AttackParams::ScaleForDifficulty` 0x52E0D0 scales it.
    pub attack: i32,
    /// Defence skill (0x52E0D0 scales it too).
    pub defence: i32,
}

const _: () = {
    assert!(offset_of!(AttackParams, attack_spread) == 0x14);
    assert!(offset_of!(AttackParams, attack) == 0x18);
    assert!(offset_of!(AttackParams, defence) == 0x1C);
    assert!(size_of::<AttackParams>() == 0x20);
};

impl AttackParams {
    /// Fills `out` with the attack parameters of the unit server `server`: `CUnitServer::GetAttackParams` 0x555A00
    /// (thiscall, `ret 4`).
    ///
    /// # Safety
    /// `server` must be a live `CUnitServer` and `out` writable for 0x20 bytes, on the game's thread of game.exe RU
    /// 1.06.
    pub unsafe fn of_server(server: *mut super::unit::UnitServer, out: *mut Self) {
        // SAFETY: 0x555A00 is `void __thiscall (AttackParams *)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut super::unit::UnitServer, *mut Self)>(
                eix_sites::C_UNIT_SERVER_GET_ATTACK_PARAMS,
            )
        };
        // SAFETY: the caller guarantees the unit server, the buffer and the thread.
        unsafe { f(server, out) }
    }

    /// Unless `keyed`, multiplies attack and defence by the difficulty's factors: `AttackParams::ScaleForDifficulty`
    /// 0x52E0D0 (thiscall, `ret 4`; the flag is a whole stack word).
    ///
    /// # Safety
    /// `this` must point at attack parameters, on the game's thread of a supported game.exe.
    pub unsafe fn scale_for_difficulty(this: *mut Self, keyed: bool) {
        // SAFETY: 0x52E0D0 is `void __thiscall (int keyed)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, u32)>(
                eix_sites::ATTACK_PARAMS_SCALE_FOR_DIFFICULTY,
            )
        };
        // SAFETY: the caller guarantees the parameters and the thread.
        unsafe { f(this, u32::from(keyed)) }
    }
}
