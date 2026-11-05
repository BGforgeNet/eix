//! FixSpellSelfCasting: `CSpell::IsOffensive` 0x68B770 also counts the spell types 0x28 and 0x29 (core's
//! [`is_offensive`]), and casting on a clicked unit (`CBattleScreen::SelectUnit` 0x638EF0) always passes 0 as the
//! selected spell's flag.
#![expect(unsafe_code, reason = "reading the spell the game asks about")]

use eix_core::fixes::is_offensive;
use eix_engine::game::spell::Spell;
use eix_engine::group::SiteWrite;
use eix_engine::hook;

use super::{BuildError, Labelled, Setup};

/// `IsOffensive`'s first instructions: `mov eax, [ecx + 0x18]; shr eax, 0x10`.
const IS_OFFENSIVE: usize = eix_sites::C_SPELL_IS_OFFENSIVE;
const IS_OFFENSIVE_START: [u8; 6] = [0x8B, 0x41, 0x18, 0xC1, 0xE8, 0x10];
/// `mov cl, [eax + 0x7F]`: the selected spell's flag, which the cast call 0x637500 takes; it becomes `mov cl, 0`.
const SELECT_FLAG: usize = eix_sites::C_BATTLE_SCREEN_SELECT_UNIT__557;

/// `bool __thiscall CSpell::IsOffensive()`; the callers read AL only.
extern "thiscall" fn spell_is_offensive(this: *const Spell) -> bool {
    // SAFETY: the game calls it on a live spell, on its thread.
    is_offensive(unsafe { (*this).spell_type() })
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let (write, _) = hook::replace_function(
        IS_OFFENSIVE,
        &IS_OFFENSIVE_START,
        spell_is_offensive as *const () as usize,
    )?;
    Ok(vec![
        Labelled {
            label: "FixSpellSelfCasting #0".to_owned(),
            write,
        },
        Labelled {
            label: "FixSpellSelfCasting #1".to_owned(),
            write: SiteWrite::bytes(SELECT_FLAG, &[0x8A, 0x48, 0x7F], &[0xB1, 0x00, 0x90])?,
        },
    ])
}
