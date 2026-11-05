//! ExpCalc: how `CWorldServer::AwardExperience` shares an amount among a player's heroes. At 0x59152B the game
//! divides the amount by the hero count in ST0 (`fdivr dword [ebp+0xc]; fstp dword [ebp+0xc]`); the site calls
//! [`share`] instead, which applies `[AddonExp] ChargeMethod` and `MulFactor`.
#![expect(
    unsafe_code,
    reason = "a naked handler that works on the game's x87 stack and frame"
)]

use std::sync::atomic::{AtomicU32, Ordering};

use eix_engine::group::SiteWrite;

use super::{BuildError, Labelled, Setup};

/// `ChargeMethod`: 1 multiplies, 2 adds, anything else divides.
static METHOD: AtomicU32 = AtomicU32::new(0);
/// `MulFactor`, as `f32` bits.
static FACTOR: AtomicU32 = AtomicU32::new(0x3F80_0000);

/// Called from 0x59152B with the hero count in ST0 and the amount at `[ebp+0xc]`; stores
/// `share(count, amount) * MulFactor` there and pops ST0, as the replaced pair did. The next instruction is a `jle` on
/// the flags of the game's `cmp eax, edi`, so the flags and every register are kept.
#[unsafe(naked)]
extern "C" fn share() {
    core::arch::naked_asm!(
        "pushfd",
        "push eax",
        "mov eax, dword ptr [{method}]",
        "cmp eax, 1",
        "je 2f",
        "cmp eax, 2",
        "je 3f",
        // The game's own share: amount / count.
        "fdivr dword ptr [ebp + 0xc]",
        "jmp 4f",
        "2:",
        "fmul dword ptr [ebp + 0xc]",
        "jmp 4f",
        "3:",
        "fadd dword ptr [ebp + 0xc]",
        "4:",
        "fmul dword ptr [{factor}]",
        "fstp dword ptr [ebp + 0xc]",
        "pop eax",
        "popfd",
        "ret",
        method = sym METHOD,
        factor = sym FACTOR,
    );
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let exp = &setup.options.ai.exp;
    METHOD.store(exp.charge_method as u32, Ordering::Relaxed);
    FACTOR.store(exp.mul_factor.to_bits(), Ordering::Relaxed);
    Ok(vec![Labelled {
        label: "Experience".to_owned(),
        write: SiteWrite::call(
            eix_sites::C_WORLD_SERVER_AWARD_EXPERIENCE__36B,
            &[0xD8, 0x7D, 0x0C, 0xD9, 0x5D, 0x0C],
            share as *const () as usize,
        )?,
    }])
}
