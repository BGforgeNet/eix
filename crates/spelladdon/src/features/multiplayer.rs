//! Features of multiplayer rules, game speed and the speedrun timer word. `enablekillsinmulti` belongs to the combat
//! features in the registry.
#![expect(
    unsafe_code,
    reason = "the speedrun timer's handlers write the timer word and read the game's mission global"
)]

use eix_engine::group::SiteWrite;
use eix_engine::hook::Context;
use eix_sites::Site;

use super::common::{self, Bytes, fixed};
use super::{Build, BuildError, Ctx};

/// The implemented features: `(section, key, build)`.
pub(crate) const BUILDS: &[(&str, &str, Build)] = &[
    ("speed", "allowmpspeed", allow_mp_speed),
    ("gameplay", "fullexpforall", full_exp_for_all),
    ("gameplay", "manygameexe", many_game_exe),
    ("gameplay", "enablecheatsinmulti", cheats_in_multi),
    ("gameplay", "extendedparams", extended_params),
    ("gameplay", "longnick", long_nick),
    ("speed", "rewritespeed", rewrite_speed),
    ("speed", "rewritespeedkey", rewrite_speed_key),
    ("speed", "disablepausekey ", disable_pause_key),
    ("speed", "disablepause", disable_pause),
    ("speed", "showallquestsinmulti", all_quests_in_multi),
    ("speed", "rewriteotherspeed", rewrite_other_speed),
    ("gameplay", "disableexppenalty", no_exp_penalty),
    ("speedrun", "timervars", timer_vars),
];

/// `manygameexe` = 1: `GameMain` never finds a running game (`je` -> `jmp`), so several can run.
fn many_game_exe(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    fixed(&[(eix_sites::TEXT_456891, &[0x74], &[0xEB])])
}

/// `longnick` = 1: `EditFieldChar` loses its field-flag test and both length limits.
fn long_nick(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    fixed(&[
        (
            eix_sites::EDIT_FIELD_CHAR__9E,
            &[0x0F, 0x84, 0x3D, 0x01, 0x00, 0x00],
            &[0x90; 6],
        ),
        (eix_sites::EDIT_FIELD_CHAR__16A, &[0x7D, 0x59], &[0x90; 2]),
        (eix_sites::EDIT_FIELD_CHAR__1A3, &[0x7D, 0x20], &[0x90; 2]),
    ])
}

/// The multiplayer hero screen's attribute minimum, maximum and required sum: 15, 35, 75 in `.rdata`.
const HERO_PARAMS: Site = eix_sites::RDATA_741EFC;
const HERO_PARAMS_BYTES: [u8; 12] = [
    0x00, 0x00, 0x70, 0x41, 0x00, 0x00, 0x0C, 0x42, 0x00, 0x00, 0x96, 0x42,
];
/// What `extendedparams` = 1 writes there: minimum 5, maximum 45, required sum 20000.
const HERO_PARAMS_EXTENDED: [f32; 3] = [5.0, 45.0, 20000.0];
/// `fld [eax + 0x3c]; fadd [eax + 0x34]; fadd [eax + 0x2c]` in `CMpHeroScreen::Update` and the same on EDI in
/// `MpHeroCreate`, which become `nop x3; fld [0x741c98]` (the required sum).
const SUM_SITES: [Bytes; 2] = [
    (
        eix_sites::C_MP_HERO_SCREEN_UPDATE__EF,
        &[0xD9, 0x40, 0x3C, 0xD8, 0x40, 0x34, 0xD8, 0x40, 0x2C],
        &[0x90, 0x90, 0x90, 0xD9, 0x05, 0x98, 0x1C, 0x74, 0x00],
    ),
    (
        eix_sites::MP_HERO_CREATE__44,
        &[0xD9, 0x47, 0x3C, 0xD8, 0x47, 0x34, 0xD8, 0x47, 0x2C],
        &[0x90, 0x90, 0x90, 0xD9, 0x05, 0x98, 0x1C, 0x74, 0x00],
    ),
];

/// `extendedparams` = 1: the three hero-screen constants become 5, 45, 20000. = 2: they become `parammin`,
/// `parammax`, `balance`, and the screen compares the required sum with itself, so any attributes pass.
fn extended_params(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    let params = |values: [f32; 3]| -> Result<SiteWrite, BuildError> {
        let mut new = Vec::with_capacity(12);
        for v in values {
            new.extend_from_slice(&v.to_le_bytes());
        }
        Ok(SiteWrite::bytes(HERO_PARAMS, &HERO_PARAMS_BYTES, &new)?)
    };
    match value {
        // The block copies three locals that start-up sets to 5, 45 and 20000 and nothing else writes.
        1 => Ok(vec![params(HERO_PARAMS_EXTENDED)?]),
        2 => {
            let ini = [
                ctx.values.float("gameplay", "parammin")?,
                ctx.values.float("gameplay", "parammax")?,
                ctx.values.float("gameplay", "balance")?,
            ];
            let mut writes = vec![params(ini)?];
            writes.extend(fixed(&SUM_SITES)?);
            Ok(writes)
        }
        _ => Ok(Vec::new()),
    }
}

/// `fullexpforall` = 1: `CWorldServer::GiveExperience` no longer divides by the number of receivers.
fn full_exp_for_all(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    fixed(&[
        (
            eix_sites::C_WORLD_SERVER_AWARD_EXPERIENCE__361,
            &[0xDB, 0x45, 0xE8],
            &[0x90; 3],
        ),
        (
            eix_sites::C_WORLD_SERVER_AWARD_EXPERIENCE__36B,
            &[0xD8, 0x7D, 0x0C, 0xD9, 0x5D, 0x0C],
            &[0x90; 6],
        ),
    ])
}

/// `enablecheatsinmulti` = 1: both `IsMultiplayerClient` calls of the console dispatcher become `mov eax, 0`.
fn cheats_in_multi(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    fixed(&[
        (
            eix_sites::TEXT_5DD558,
            &[0xE8, 0xE3, 0x1E, 0xFA, 0xFF],
            &[0xB8, 0, 0, 0, 0],
        ),
        (
            eix_sites::TEXT_5DD780,
            &[0xE8, 0xBB, 0x1C, 0xFA, 0xFF],
            &[0xB8, 0, 0, 0, 0],
        ),
    ])
}

/// `allowmpspeed` >= 1: the "multiplayer: skip" branches before the speed changes of the keys and the clock button
/// become NOPs.
fn allow_mp_speed(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    fixed(&[
        (
            eix_sites::BATTLE_SCREEN_KEY_DOWN__174,
            &[0x0F, 0x85, 0xEA, 0x0D, 0x00, 0x00],
            &[0x90; 6],
        ),
        (
            eix_sites::BATTLE_SCREEN_KEY_DOWN__1DA,
            &[0x0F, 0x85, 0x84, 0x0D, 0x00, 0x00],
            &[0x90; 6],
        ),
        (
            eix_sites::CLOCK_MOUSE_UP_VERIFY__8E,
            &[0x0F, 0x85, 0xBF, 0x00, 0x00, 0x00],
            &[0x90; 6],
        ),
        (
            eix_sites::CLOCK_MOUSE_UP_VERIFY__F5,
            &[0x75, 0x5C],
            &[0x90; 2],
        ),
    ])
}

/// The immediate of a `mov dword [g_updatePeriod], imm32` the game holds `was` in, set to `value`.
fn period(site: Site, was: u32, value: i32) -> Result<SiteWrite, BuildError> {
    Ok(SiteWrite::bytes(
        site,
        &was.to_le_bytes(),
        &value.to_le_bytes(),
    )?)
}

/// `rewritespeed` = 1: the clock button's normal and fast periods become `normal` and `fast`.
fn rewrite_speed(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![
        period(
            eix_sites::CLOCK_MOUSE_UP_VERIFY__BE,
            0x37,
            ctx.values.int("speed", "normal")?,
        )?,
        period(
            eix_sites::CLOCK_MOUSE_UP_VERIFY__121,
            0x1B,
            ctx.values.int("speed", "fast")?,
        )?,
    ])
}

/// `rewritespeedkey` = 1: the speed keys' fast and normal periods become `fastkey` and `normalkey`.
fn rewrite_speed_key(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![
        period(
            eix_sites::BATTLE_SCREEN_KEY_DOWN__19B,
            0x1B,
            ctx.values.int("speed", "fastkey")?,
        )?,
        period(
            eix_sites::BATTLE_SCREEN_KEY_DOWN__1FD,
            0x37,
            ctx.values.int("speed", "normalkey")?,
        )?,
    ])
}

/// `test al, al; sete dl; push edx; mov ecx, 0x7afea4; call GameSendFlag10`: the pause toggle, of the clock button
/// at 0x648dd9 and of the pause key at 0x63a69b; the calls differ in their displacement.
fn pause_toggle(call: [u8; 4]) -> [u8; 16] {
    let mut b = [
        0x84, 0xC0, 0x0F, 0x94, 0xC2, 0x52, 0xB9, 0xA4, 0xFE, 0x7A, 0x00, 0xE8, 0, 0, 0, 0,
    ];
    b[12..].copy_from_slice(&call);
    b
}

/// The pause toggle replaced by `mov dword [g_updatePeriod], period` and six NOPs.
fn pause_write(site: Site, call: [u8; 4], period: i32) -> Result<SiteWrite, BuildError> {
    let mut new = [0x90; 16];
    new[..6].copy_from_slice(&[0xC7, 0x05, 0x34, 0xFF, 0x78, 0x00]);
    new[6..10].copy_from_slice(&period.to_le_bytes());
    Ok(SiteWrite::bytes(site, &pause_toggle(call), &new)?)
}

/// `disablepause` = 1: the clock's pause button sets the period `pause` instead of pausing.
fn disable_pause(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let pause = ctx.values.int("speed", "pause")?;
    Ok(vec![pause_write(
        eix_sites::CLOCK_MOUSE_UP_VERIFY__69,
        [0x47, 0xFF, 0xF2, 0xFF],
        pause,
    )?])
}

/// `"disablepausekey "` = 1 (the original's key name, trailing space included): the pause key sets the period
/// `pausekey` instead of pausing.
fn disable_pause_key(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let pause = ctx.values.int("speed", "pausekey")?;
    Ok(vec![pause_write(
        eix_sites::BATTLE_SCREEN_KEY_DOWN__13B,
        [0x85, 0xE6, 0xF3, 0xFF],
        pause,
    )?])
}

/// `showallquestsinmulti` = 1: `PickZones` jumps over its multiplayer test.
fn all_quests_in_multi(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    fixed(&[(eix_sites::PICK_ZONES__142, &[0x02], &[0x08])])
}

/// `rewriteotherspeed`: 2 makes `CBattleScreen::Close` reset the period to `normal1`; 3 makes the clock's first
/// period `(mask & (fast2 - normal2)) + normal2` in `CHudClock::SetupVanilla`; 1 does both.
fn rewrite_other_speed(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    let mut writes = Vec::new();
    if matches!(value, 1 | 2) {
        writes.push(period(
            eix_sites::C_BATTLE_SCREEN_CLOSE__14,
            0x37,
            ctx.values.int("speed", "normal1")?,
        )?);
    }
    if matches!(value, 1 | 3) {
        let normal = ctx.values.byte("speed", "normal2")?;
        let fast = ctx.values.byte("speed", "fast2")?;
        writes.push(SiteWrite::bytes(
            eix_sites::C_HUD_CLOCK_SETUP__38A,
            &[0xE4],
            &[fast.wrapping_sub(normal)],
        )?);
        writes.push(SiteWrite::bytes(
            eix_sites::C_HUD_CLOCK_SETUP__38D,
            &[0x37],
            &[normal],
        )?);
    }
    Ok(writes)
}

/// `UnitStats::RaiseSkill`'s and `RaiseAttribute`'s move of the price into the pending experience (+0x700), which
/// becomes `fld st(0); fadd [esi + 4]; fstp [esi + 4]; fadd [esi + 8]; fstp [esi + 8]` and NOPs.
const PENDING_RAISE: [u8; 53] = [
    0xD8, 0x86, 0x00, 0x07, 0x00, 0x00, 0xD8, 0x15, 0x58, 0xB8, 0x73, 0x00, 0xD9, 0x96, 0x00, 0x07,
    0x00, 0x00, 0xDF, 0xE0, 0xF6, 0xC4, 0x41, 0x75, 0x1A, 0xD9, 0xC0, 0xD8, 0x46, 0x04, 0xC7, 0x86,
    0x00, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xD9, 0x5E, 0x04, 0xD8, 0x46, 0x08, 0xD9, 0x5E,
    0x08, 0xEB, 0x02, 0xDD, 0xD8,
];
const DIRECT_RAISE: [u8; 53] = {
    let mut b = [0x90; 53];
    let code = [
        0xD9, 0xC0, 0xD8, 0x46, 0x04, 0xD9, 0x5E, 0x04, 0xD8, 0x46, 0x08, 0xD9, 0x5E, 0x08,
    ];
    let mut i = 0;
    while i < code.len() {
        b[i] = code[i];
        i += 1;
    }
    b
};

/// `disableexppenalty` >= 1: experience goes straight to the totals (+0x04, +0x08) instead of the pending field,
/// the camp's trade panel reads +0x04 alone, and `WorldServer::Respawn` no longer takes pending experience.
fn no_exp_penalty(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    fixed(&[
        // UnitStats::AddExperience: fadd [ecx + 4]; fstp [ecx + 4]; ret 4.
        (
            eix_sites::UNIT_STATS_ADD_EXPERIENCE__5,
            &[0x81, 0x00, 0x07, 0x00, 0x00, 0xD8, 0x15, 0x58],
            &[0x41, 0x04, 0xD9, 0x59, 0x04, 0xC2, 0x04, 0x00],
        ),
        (
            eix_sites::UNIT_STATS_RAISE_SKILL__A7,
            &PENDING_RAISE,
            &DIRECT_RAISE,
        ),
        (
            eix_sites::UNIT_STATS_RAISE_ATTRIBUTE__A7,
            &PENDING_RAISE,
            &DIRECT_RAISE,
        ),
        // CTradePanel::Experience: fld [ecx + eax*4 + 0x2c].
        (
            eix_sites::C_TRADE_PANEL_EXPERIENCE__15,
            &[0x34, 0xD8, 0x44, 0x81, 0x2C],
            &[0x2C, 0x90, 0x90, 0x90, 0x90],
        ),
        // TradeBuildLists: fld [eax + 4].
        (
            eix_sites::TRADE_BUILD_LISTS__788,
            &[0x80, 0x00, 0x07, 0x00, 0x00, 0xD8, 0x40, 0x04],
            &[0x40, 0x04, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90],
        ),
        (eix_sites::WORLD_SERVER_RESPAWN__1A7, &[0x51], &[0x90]),
        (
            eix_sites::WORLD_SERVER_RESPAWN__1AF,
            &[0xE8, 0xAC, 0x45, 0xEB, 0xFF, 0x8B, 0xCE, 0xE8, 0x05, 0xF9],
            &[0x90; 10],
        ),
    ])
}

/// The speedrun timer's word, in padding of `CDiplomacy::GetHostileMask`: 0 while a mission or zone loads, 1 once a
/// battle screen is up. An external timer reads it here, so its place and values are fixed.
const TIMER_WORD: Site = eix_sites::C_DIPLOMACY_GET_HOSTILE_MASK__1D;

fn set_timer_word(value: u16) {
    // SAFETY: the word lies in game.exe's `.text` padding, which start-up made writable; only these handlers and the
    // external timer touch it.
    unsafe { common::write(TIMER_WORD.here32(), value) };
}

/// Before `CWorldServer::NewZone` or `LoadSave`: the code in the padding right before the function, which the
/// redirected calls reach; execution falls through into the function.
extern "C" fn loading(_: usize, _: &mut Context) {
    set_timer_word(0);
}

/// `LoadMission`+0x18 jumps to a `.data` stub, which does the replaced `mov ecx, [0x7af784]` and jumps back.
extern "C" fn mission_loading(_: usize, ctx: &mut Context) {
    set_timer_word(0);
    // SAFETY: 0x7af784 is a global of game.exe's `.data`, which the replaced instruction read here.
    ctx.ecx = unsafe { common::read(eix_sites::DATA_7AF784.here32()) };
    ctx.resume = eix_sites::LOAD_MISSION__1E;
}

/// A battle screen is up; the stub stands in for its function's return, which its stub runs after this.
extern "C" fn shown(_: usize, _: &mut Context) {
    set_timer_word(1);
}

/// The jump over `expected` at `site` to a stub that runs [`shown`], then `ret` (the function's own return).
fn shown_at(site: Site, expected: &[u8], ret: &[u8]) -> Result<SiteWrite, BuildError> {
    let stub = common::tail_stub(site, shown, 0, ret)?;
    Ok(SiteWrite::jmp(site, expected, stub)?)
}

/// The calls the stubs serve: each call's displacement byte that moves it from `CWorldServer::NewZone` (0x595480)
/// to its stub 0x595474, or from `LoadSave` (0x596e70) to 0x596e67.
const TIMER_CALLS: [Bytes; 8] = [
    (eix_sites::START_FIRST_MAP__B1, &[0x8B], &[0x7F]),
    (eix_sites::LOAD_MISSION__10F, &[0xFD], &[0xF4]),
    (eix_sites::ENTER_ZONE__3FC, &[0x10], &[0x07]),
    (eix_sites::ENTER_ZONE__5B7, &[0x65], &[0x59]),
    (eix_sites::START_NETWORK_ZONE__1C7, &[0xB5], &[0xAC]),
    (eix_sites::START_NETWORK_ZONE__2AF, &[0xDD], &[0xD1]),
    (eix_sites::GAME_FLOW_START_ZONE__1F9, &[0x23], &[0x17]),
    (eix_sites::C_WORLD_SERVER_ENSURE_ZONE__3C, &[0xF0], &[0xE4]),
];
/// `CGlobalMap::Create`'s `ret; nop x4`, which becomes `jmp 0x635c73`.
const GLOBAL_MAP_RET: Bytes = (
    eix_sites::C_GLOBAL_MAP_CREATE__1A40,
    &[0xC3, 0x90, 0x90, 0x90, 0x90],
    &[0xE9, 0xDE, 0x6E, 0x00, 0x00],
);
/// `CBriefingScreen::Create`'s `ret; nop`, which becomes `jmp 0x602ba7` (padding the next jump fills).
const BRIEFING_RET: Bytes = (
    eix_sites::C_BRIEFING_SCREEN_CREATE__6CD,
    &[0xC3, 0x90],
    &[0xEB, 0x28],
);
const NOPS_5: &[u8] = &[0x90; 5];
const RET_NOPS_4: &[u8] = &[0xC3, 0x90, 0x90, 0x90, 0x90];
const RET: &[u8] = &[0xC3];

/// `timervars` 1, 2 or 3: the timer word's writes, at the original's places (1 and 3 differ in which battle-screen
/// paths set it; 2 sets it from `CInterface3D::Activate` instead of the briefing).
fn timer_vars(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if !matches!(value, 1..=3) {
        return Ok(Vec::new());
    }
    let mut writes = fixed(&TIMER_CALLS)?;
    writes.push(SiteWrite::bytes(
        eix_sites::LOAD_MISSION__18,
        &[0x8B, 0x0D, 0x84, 0xF7, 0x7A, 0x00],
        &[0x90, 0xE9, 0x62, 0x07, 0x22, 0x00],
    )?);
    writes.extend(common::jump_at(
        eix_sites::C_WORLD_SERVER_SEND_FRESH_OBJECTS__2D4,
        &[0x90; 9],
        0,
        loading,
        0,
    )?);
    writes.extend(common::jump_at(
        eix_sites::C_WORLD_SERVER_CLEAR_MAP_OBJECTS__207,
        &[0x90; 9],
        0,
        loading,
        0,
    )?);
    writes.extend(common::jump_at(
        eix_sites::DATA_79A5E0,
        &[0; 5],
        0,
        mission_loading,
        0,
    )?);
    writes.extend(fixed(&[GLOBAL_MAP_RET])?);
    match value {
        1 => {
            writes.extend(fixed(&[
                BRIEFING_RET,
                (
                    eix_sites::INPUT_SIDE_DESTROY__7,
                    NOPS_5,
                    &[0xE9, 0xC7, 0x30, 0x03, 0x00],
                ),
            ])?);
            writes.push(shown_at(
                eix_sites::C_BATTLE_SCREEN_CREATE__A23,
                RET_NOPS_4,
                RET,
            )?);
        }
        2 => {
            // `CInterface3D::Activate`'s `ret 4; nop x3` becomes `nop; jmp 0x5957d2`. The original also writes a
            // copy of that stub at 0x5f5f24, which nothing reaches; it is left out.
            writes.push(SiteWrite::bytes(
                eix_sites::C_INTERFACE_3D_ACTIVATE__18A,
                &[0xC2, 0x04, 0x00, 0x90, 0x90, 0x90],
                &[0x90, 0xE9, 0x82, 0xE4, 0xF9, 0xFF],
            )?);
            writes.push(shown_at(
                eix_sites::C_WORLD_SERVER_PREPARE_SAVE__F2,
                NOPS_5,
                &[0xC2, 0x04, 0x00],
            )?);
        }
        _ => {
            // The global map's jump lands on `CBattleScreen::Create`'s own `ret` at 0x635c73, one byte before the
            // stub, so the global map does not set the word.
            writes.extend(fixed(&[
                BRIEFING_RET,
                (
                    eix_sites::INPUT_SIDE_DESTROY__7,
                    NOPS_5,
                    &[0xE9, 0xC8, 0x30, 0x03, 0x00],
                ),
            ])?);
            writes.push(shown_at(
                eix_sites::C_BATTLE_SCREEN_CREATE__A24,
                NOPS_5,
                RET,
            )?);
            writes.push(shown_at(
                eix_sites::C_BATTLE_SCREEN_POLL_SHOWN__56,
                RET_NOPS_4,
                RET,
            )?);
        }
    }
    Ok(writes)
}

#[cfg(test)]
mod tests {
    use super::super::testing::{no_log, plain, values};
    use super::*;

    #[test]
    fn hero_screen_constants_are_fixed_then_from_the_ini() {
        // As the original, value 1 writes 5, 45, 20000 whatever the INI says; value 2 the INI's.
        let v = values(&[
            ("gameplay", "parammin", "11"),
            ("gameplay", "parammax", "44.9"),
            ("gameplay", "balance", "99"),
        ]);
        let ctx = Ctx::new(&v, no_log);
        let w = plain(&extended_params(&ctx, 1).unwrap());
        let want = [
            0x00, 0x00, 0xA0, 0x40, 0x00, 0x00, 0x34, 0x42, 0x00, 0x40, 0x9C, 0x46,
        ];
        assert_eq!(
            w,
            [(HERO_PARAMS, HERO_PARAMS_BYTES.to_vec(), want.to_vec())]
        );
        let w = plain(&extended_params(&ctx, 2).unwrap());
        // The fractional parameter is kept.
        let want = [
            0x00, 0x00, 0x30, 0x41, 0x9A, 0x99, 0x33, 0x42, 0x00, 0x00, 0xC6, 0x42,
        ];
        assert_eq!((w.len(), &w[0].2[..]), (3, &want[..]));
        assert!(extended_params(&ctx, 3).unwrap().is_empty());
    }

    #[test]
    fn speed_periods_land_in_their_immediates() {
        let v = values(&[
            ("speed", "normal", "31"),
            ("speed", "fast", "62"),
            ("speed", "normalkey", "33"),
            ("speed", "fastkey", "66"),
            ("speed", "normal1", "40"),
            ("speed", "normal2", "50"),
            ("speed", "fast2", "20"),
            ("speed", "pause", "99999999999"),
        ]);
        let ctx = Ctx::new(&v, no_log);
        let w = plain(&rewrite_speed(&ctx, 1).unwrap());
        assert_eq!(
            w[0],
            (
                eix_sites::CLOCK_MOUSE_UP_VERIFY__BE,
                vec![0x37, 0, 0, 0],
                vec![31, 0, 0, 0]
            )
        );
        assert_eq!(w[1].2, [62, 0, 0, 0]);
        let w = plain(&rewrite_speed_key(&ctx, 1).unwrap());
        assert_eq!(
            (w[0].0, w[0].2[0], w[1].0, w[1].2[0]),
            (
                eix_sites::BATTLE_SCREEN_KEY_DOWN__19B,
                66,
                eix_sites::BATTLE_SCREEN_KEY_DOWN__1FD,
                33
            )
        );
        let w = plain(&rewrite_other_speed(&ctx, 1).unwrap());
        assert_eq!(
            w,
            [
                (
                    eix_sites::C_BATTLE_SCREEN_CLOSE__14,
                    vec![0x37, 0, 0, 0],
                    vec![40, 0, 0, 0]
                ),
                (eix_sites::C_HUD_CLOCK_SETUP__38A, vec![0xE4], vec![0xE2]),
                (eix_sites::C_HUD_CLOCK_SETUP__38D, vec![0x37], vec![50]),
            ]
        );
        assert_eq!(rewrite_other_speed(&ctx, 2).unwrap().len(), 1);
        assert_eq!(rewrite_other_speed(&ctx, 3).unwrap().len(), 2);
        assert!(rewrite_other_speed(&ctx, 4).unwrap().is_empty());
        // The OS keeps the low 32 bits of 99999999999.
        let w = plain(&disable_pause(&ctx, 1).unwrap());
        assert_eq!(
            w[0].2,
            [
                0xC7, 0x05, 0x34, 0xFF, 0x78, 0x00, 0xFF, 0xE7, 0x76, 0x48, 0x90, 0x90, 0x90, 0x90,
                0x90, 0x90
            ]
        );
    }

    #[test]
    fn switches_take_only_their_tested_values() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        for b in [
            many_game_exe,
            long_nick,
            full_exp_for_all,
            cheats_in_multi,
            all_quests_in_multi,
        ] {
            assert!(!b(&ctx, 1).unwrap().is_empty());
            assert!(b(&ctx, 2).unwrap().is_empty());
        }
        for b in [allow_mp_speed, no_exp_penalty] {
            assert!(!b(&ctx, 2).unwrap().is_empty());
            assert!(b(&ctx, 0).unwrap().is_empty());
            assert!(b(&ctx, -1).unwrap().is_empty());
        }
        assert!(timer_vars(&ctx, 4).unwrap().is_empty());
    }

    #[test]
    fn timer_calls_reach_their_stubs() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        for value in 1..=3 {
            let w = plain(&timer_vars(&ctx, value).unwrap());
            // The redirected calls and jumps keep the original's bytes; the stubs hold our jumps.
            assert!(w.contains(&(eix_sites::START_FIRST_MAP__B1, vec![0x8B], vec![0x7F])));
            let stub = w
                .iter()
                .find(|s| s.0 == eix_sites::C_WORLD_SERVER_SEND_FRESH_OBJECTS__2D4)
                .unwrap();
            assert_eq!(stub.2[0], 0xE9);
        }
        let w = plain(&timer_vars(&ctx, 2).unwrap());
        assert!(
            w.iter()
                .any(|s| s.0 == eix_sites::C_WORLD_SERVER_PREPARE_SAVE__F2)
        );
        assert!(
            !w.iter()
                .any(|s| s.0 == eix_sites::C_BRIEFING_SCREEN_CREATE__6CD)
        );
    }
}
