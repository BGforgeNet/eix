//! LoadSpellAnimationFromSlotsField: `CUnitServer::PlanAction` takes a cast's animation kind from the spell record's
//! slots field instead of its school (and the two spell types with fixed animations lose them).
#![expect(
    unsafe_code,
    reason = "reading PlanAction's spell and writing its plan at the hooked site"
)]

use eix_engine::game::spell::Spell;
use eix_engine::game::unit::OrderPlan;
use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use super::{BuildError, Labelled, Setup};

/// Where `PlanAction` goes on once the cast kind is stored.
const AFTER_CAST_KIND: Site = eix_sites::C_UNIT_SERVER_PLAN_ACTION__6D0;

/// At 0x54D0DC: the spell is `PlanAction`'s local at EBP-0x84, the plan is ESI.
extern "C" fn cast_kind(_: usize, ctx: &mut Context) {
    let local =
        core::ptr::with_exposed_provenance::<*mut Spell>(ctx.ebp.wrapping_sub(0x84) as usize);
    // SAFETY: EBP is PlanAction's frame at 0x54D0DC; its local at -0x84 holds the spell it plans (non-null there).
    let spell = unsafe { local.read() };
    // SAFETY: a live spell, on the game's thread.
    let record = unsafe { Spell::record(spell) };
    let plan = core::ptr::with_exposed_provenance_mut::<OrderPlan>(ctx.esi as usize);
    // SAFETY: the record is the spell's database entry.
    let slots = unsafe { (*record).slots };
    // SAFETY: ESI is the plan PlanAction fills.
    unsafe { (*plan).cast_kind = slots as i32 };
    ctx.resume = AFTER_CAST_KIND;
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "LoadSpellAnimationFromSlotsField #0".to_owned(),
        write: hook::context_hook(
            eix_sites::C_UNIT_SERVER_PLAN_ACTION__69C,
            &[0x8B, 0x41, 0x18, 0xC1, 0xE8, 0x10],
            Mode::Drop,
            cast_kind,
            0,
        )?,
    }])
}
