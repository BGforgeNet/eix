//! Item, shop and HUD features.
//!
//! Where the original jumps to added code, the site becomes a hook whose handler does that code's work; the added
//! code's own bytes are not written. Data the original keeps outside that code (header slack, padding) is written as
//! it does, and the handlers read it there.
#![expect(
    unsafe_code,
    reason = "the handlers read and write the game's units, shop templates and HUD, and call its functions"
)]

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use eix_engine::game::flow;
use eix_engine::game::unit::UnitStats;
use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, Mode};
use eix_engine::x87::Ext;
use eix_sites::Site;

use super::common::{self, Obj, read};
use super::{Build, BuildError, Ctx, rebalance};

/// `UnitStats`' size (its whole-object copy in `CopyStats`).
const UNIT_STATS: u32 = 0x704;
/// `CUnit`'s size.
const UNIT: u32 = 0x4DC;
/// The HUD weapon bar's fields reach to its unit at +0x154.
const WEAPON_BAR: u32 = 0x158;

/// The implemented features: `(section, key, build)`.
pub(crate) const BUILDS: &[(&str, &str, Build)] = &[
    ("skills", "extendedskills", extended_skills),
    ("gameplay", "unlockmoney", unlock_money),
    ("stealing", "extendedstealing", extended_stealing),
    ("gameplay", "saveall", save_all),
    ("gameplay", "extendedmerc", extended_merc),
    ("gameplay", "shopschanges", shops_changes),
    ("gameplay", "copystatscut", copy_stats_cut),
    (
        "gameplay",
        "shopsnewnormalchanges",
        shops_new_normal_changes,
    ),
    ("gameplay", "liftchanges", lift_changes),
    ("gameplay", "liftdebuffs", lift_debuffs),
    ("gameplay", "heroalwayscanrun", hero_always_can_run),
    ("gameplay", "encumbrancemovespeed", encumbrance_move_speed),
    (
        "gameplay",
        "encumbranceattackspeed",
        encumbrance_attack_speed,
    ),
    ("gameplay", "dmgtypes", dmg_types),
    ("gameplay", "itemslightchanges", items_light_changes),
    ("gameplay", "heroscanhideweapons", heroes_can_hide_weapons),
    ("gameplay", "avgarmors", avg_armors),
    ("gameplay", "camp", camp),
    ("gameplay", "improvedstealing", improved_stealing),
    ("gameplay", "newoptions", new_options),
];

fn bytes(site: Site, expected: &[u8], new: &[u8]) -> Result<SiteWrite, BuildError> {
    Ok(SiteWrite::bytes(site, expected, new)?)
}

// --- The camp's skill trade panel ------------------------------------------------------------------------------

/// One `cmp ..., 0x64` of the trade panel and the instructions around it: what the handler redoes with the INI's
/// limit.
#[derive(Debug, Clone, Copy)]
enum SkillCap {
    /// `cmp dword ptr [eax + ebp + 0x10], 0x64`.
    AtEaxEbp,
    /// `mov edx, dword ptr [edx + eax + 0x10]; cmp edx, 0x64`.
    LoadEdx,
    /// `add edx, ecx; cmp dword ptr [edx + 0x10], 0x64`.
    AddEdx,
    /// `mov eax, dword ptr [esi + 0x180]; cmp dword ptr [eax + 0x10], 0x64`.
    LoadEax,
}

/// A trade-panel cap: where it was and the limit it now compares with.
#[derive(Debug)]
struct Cap {
    form: SkillCap,
    limit: u32,
    resume: Site,
}

extern "C" fn skill_cap(arg: usize, ctx: &mut Context) {
    // SAFETY: `arg` is the `Cap` its build kept.
    let cap = unsafe { common::kept::<Cap>(arg) };
    // The skill record the replaced instructions address, and its +0x10 level.
    let record = match cap.form {
        SkillCap::AtEaxEbp => ctx.eax.wrapping_add(ctx.ebp),
        SkillCap::LoadEdx => ctx.edx.wrapping_add(ctx.eax),
        SkillCap::AddEdx => {
            ctx.edx = ctx.edx.wrapping_add(ctx.ecx);
            ctx.edx
        }
        SkillCap::LoadEax => {
            // SAFETY: ESI is the trade panel, whose +0x180 the replaced `mov eax, [esi + 0x180]` reads.
            let panel = unsafe { Obj::new(ctx.esi, 0x184) };
            ctx.eax = panel.get(0x180);
            ctx.eax
        }
    };
    // SAFETY: the record is the one the replaced `cmp` reads at +0x10.
    let skill: u32 = unsafe { Obj::new(record, 0x14) }.get(0x10);
    if let SkillCap::LoadEdx = cap.form {
        ctx.edx = skill;
    }
    ctx.eflags = eix_core::x86_flags::cmp(ctx.eflags, skill, cap.limit);
    ctx.resume = cap.resume;
}

/// The three caps of one block: `(site, game bytes, NOPs before the jump, form, resume)`.
type CapSite = (Site, &'static [u8], usize, SkillCap, Site);

fn skill_caps(sites: &[CapSite], limit: u8) -> Result<Vec<SiteWrite>, BuildError> {
    let mut writes = Vec::new();
    for &(site, expected, at, form, resume) in sites {
        let arg = common::keep(Cap {
            form,
            limit: limit.into(),
            resume,
        });
        writes.extend(common::jump_at(site, expected, at, skill_cap, arg)?);
    }
    Ok(writes)
}

/// `extendedskills` = 1: three of the panel's caps of 100 become `[skills] skills`.
fn extended_skills(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    const SITES: [CapSite; 3] = [
        (
            eix_sites::TRADE_DRAW_LISTS__21A,
            &[0x83, 0x7C, 0x28, 0x10, 0x64],
            0,
            SkillCap::AtEaxEbp,
            eix_sites::TRADE_DRAW_LISTS__21F,
        ),
        (
            eix_sites::TEXT_626A1E,
            &[0x8B, 0x54, 0x02, 0x10, 0x83, 0xFA, 0x64],
            2,
            SkillCap::LoadEdx,
            eix_sites::TEXT_626A25,
        ),
        (
            eix_sites::C_TRADE_PANEL_UPDATE_HELD__63,
            &[0x03, 0xD1, 0x83, 0x7A, 0x10, 0x64],
            1,
            SkillCap::AddEdx,
            eix_sites::C_TRADE_PANEL_UPDATE_HELD__69,
        ),
    ];
    skill_caps(&SITES, ctx.values.byte("skills", "skills")?)
}

/// `extendedstealing` = 1: the other three caps become `[stealing] stealing`.
fn extended_stealing(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    const SITES: [CapSite; 3] = [
        (
            eix_sites::TRADE_DRAW_LISTS__3B5,
            &[0x8B, 0x86, 0x80, 0x01, 0x00, 0x00, 0x83, 0x78, 0x10, 0x64],
            0,
            SkillCap::LoadEax,
            eix_sites::TRADE_DRAW_LISTS__3BF,
        ),
        (
            eix_sites::TEXT_626AAD,
            &[0x8B, 0x54, 0x10, 0x10, 0x83, 0xFA, 0x64],
            2,
            SkillCap::LoadEdx,
            eix_sites::TEXT_626AB4,
        ),
        (
            eix_sites::C_TRADE_PANEL_UPDATE_HELD__C2,
            &[0x03, 0xD1, 0x83, 0x7A, 0x10, 0x64],
            1,
            SkillCap::AddEdx,
            eix_sites::C_TRADE_PANEL_UPDATE_HELD__C8,
        ),
    ];
    skill_caps(&SITES, ctx.values.byte("stealing", "stealing")?)
}

// --- Plain switches -----------------------------------------------------------------------------------------------

/// `unlockmoney` = 1: `GuardPlayers` stops XOR-encoding the money (`xor edx, ebp` at 0x579210 NOPed).
fn unlock_money(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![SiteWrite::nop(
        eix_sites::GUARD_PLAYERS__50,
        &[0x33, 0xD5],
    )])
}

/// `heroalwayscanrun` = 1: `UnitStats::MovementState` always takes the "may run" branch.
fn hero_always_can_run(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![bytes(
        eix_sites::UNIT_STATS_MOVEMENT_STATE__70,
        &[0x74],
        &[0xEB],
    )?])
}

/// `saveall` >= 1: `LoadGame` and `SaveGame` treat every zone as "bz7g". Its value 32 also takes part in
/// `extendedmerc`'s draw range (that block tests it).
fn save_all(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    Ok(vec![
        bytes(eix_sites::GAME_FLOW_SAVE_GAME__24E, &[0x74], &[0xEB])?,
        bytes(eix_sites::GAME_FLOW_LOAD_GAME__40D, &[0x74], &[0xEB])?,
    ])
}

/// The operands of `CWorldClient::DrawPass`'s `fadd`/`fsub dword ptr [0x73BAB0]` that `extendedmerc` repoints.
const DRAW_PASS_RANGE: [Site; 4] = [
    eix_sites::C_WORLD_CLIENT_DRAW_PASS__10A,
    eix_sites::C_WORLD_CLIENT_DRAW_PASS__15C,
    eix_sites::C_WORLD_CLIENT_DRAW_PASS__19A,
    eix_sites::C_WORLD_CLIENT_DRAW_PASS__201,
];
/// `CWorldClient::Render`'s `push 0x42C80000` (100.0) operands, one of which the repointed operands then read.
const RENDER_RANGE: [Site; 2] = [
    eix_sites::C_WORLD_CLIENT_RENDER__279,
    eix_sites::C_WORLD_CLIENT_RENDER__1ED,
];

/// `extendedmerc` >= 1: the briefings' mercenary limit (`cmp [esp + 0x40], 2`) becomes `maxcountofmerc`'s low byte.
/// With `extendedmerc` >= 16, `saveall` = 32 and `maxcountofmerc` > 50 the client's draw range also grows: the
/// render's 100.0 becomes 4096.0, and the draw pass's four range operands point at it.
fn extended_merc(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    let count = ctx.values.int("gameplay", "maxcountofmerc")?;
    let mut writes = vec![bytes(
        eix_sites::C_WORLD_SERVER_UPDATE_MERC_BRIEFINGS__3C1,
        &[0x02],
        &[count as u8],
    )?];
    if count > 50 && value >= 16 && ctx.values.flag("gameplay", "saveall")? == 32 {
        for site in DRAW_PASS_RANGE {
            writes.push(SiteWrite::pointer(
                site,
                eix_sites::RDATA_73BAB0.ru(),
                RENDER_RANGE[0].ru(),
            ));
        }
        for site in RENDER_RANGE {
            writes.push(bytes(site, &100f32.to_le_bytes(), &4096f32.to_le_bytes())?);
        }
    }
    Ok(writes)
}

/// `dmgtypes` = 1: the number of damage types the camp lists, in `CItemInfoPanel::DescribeStats` (`cmp eax, 6`) and
/// `CHeroStatsPanel::DrawPart` (`mov esi, 6`).
fn dmg_types(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let count = ctx.values.int("gameplay", "dmgtypescount")?;
    Ok(vec![
        bytes(
            eix_sites::C_HERO_STATS_PANEL_DRAW_PART__14C,
            &6i32.to_le_bytes(),
            &count.to_le_bytes(),
        )?,
        bytes(
            eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_STATS__C08,
            &[6],
            &[count as u8],
        )?,
    ])
}

/// The item highlight colours in `.rdata`: eight RGBA entries.
const LIGHTS: Site = eix_sites::K_SCHOOL_GLOW;
const LIGHTS_GAME: [f32; 32] = [
    1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0,
    1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0,
];

/// `itemslightchanges` = 1: the 32 floats `light00` .. `light31`.
fn items_light_changes(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let mut expected = Vec::with_capacity(128);
    let mut new = Vec::with_capacity(128);
    for (i, game) in LIGHTS_GAME.iter().enumerate() {
        expected.extend_from_slice(&game.to_le_bytes());
        new.extend_from_slice(
            &ctx.values
                .float("gameplay", &format!("light{i:02}"))?
                .to_le_bytes(),
        );
    }
    Ok(vec![bytes(LIGHTS, &expected, &new)?])
}

// --- Shops ---------------------------------------------------------------------------------------------------------

/// The stock generators' `push max; push min` pairs: weapons, armours, wands, potions (`ClassTemplateGenerateItems`),
/// prototypes, runes (`ClassTemplateGenerateMagic`), each with game.exe's `(max, min)`.
const STOCK_RANGES: [(Site, &str, &str, u8, u8); 6] = [
    (
        eix_sites::CLASS_TEMPLATE_GENERATE_ITEMS__AE,
        "maxsweapons",
        "minsweapons",
        3,
        1,
    ),
    (
        eix_sites::CLASS_TEMPLATE_GENERATE_ITEMS__BE,
        "maxsarmors",
        "minsarmors",
        3,
        1,
    ),
    (
        eix_sites::CLASS_TEMPLATE_GENERATE_ITEMS__CC,
        "maxswands",
        "minswands",
        3,
        1,
    ),
    (
        eix_sites::CLASS_TEMPLATE_GENERATE_ITEMS__DE,
        "maxspotions",
        "minspotions",
        0x32,
        0x14,
    ),
    (
        eix_sites::CLASS_TEMPLATE_GENERATE_MAGIC__7B,
        "maxsprototypes",
        "minsprototypes",
        3,
        1,
    ),
    (
        eix_sites::CLASS_TEMPLATE_GENERATE_MAGIC__89,
        "maxsrunes",
        "minsrunes",
        0x32,
        0x14,
    ),
];

/// The traders' price factors: the `mov ecx, imm32` operands in `ClassTemplatesBuild`, each 0.5.
const PRICE_FACTORS: [(Site, &str); 4] = [
    (eix_sites::CLASS_TEMPLATES_BUILD__5F0, "ingos"),
    (eix_sites::CLASS_TEMPLATES_BUILD__506, "gipatz"),
    (eix_sites::CLASS_TEMPLATES_BUILD__415, "gipatb"),
    (eix_sites::CLASS_TEMPLATES_BUILD__6DA, "suslanger"),
];
/// Where the original keeps `jigran`, in game.exe's `.data` padding.
const JIGRAN_SLOT: Site = eix_sites::DATA_79A356;
/// That slot's value before `shopsnewnormalchanges` writes it: 0.1.
const SHOP_FIELD_DEFAULT: f32 = f32::from_bits(0x3DCC_CCCD);

/// `shopsnewnormalchanges` = 1: the INI's stock ranges and price factors (the bytes kept, as `push imm8`).
fn shops_new_normal_changes(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let mut writes = Vec::new();
    for (site, max, min, game_max, game_min) in STOCK_RANGES {
        writes.push(bytes(
            site,
            &[game_max],
            &[ctx.values.byte("gameplay", max)?],
        )?);
        writes.push(bytes(
            site.offset(2),
            &[game_min],
            &[ctx.values.byte("gameplay", min)?],
        )?);
    }
    for (site, key) in PRICE_FACTORS {
        writes.push(bytes(
            site,
            &0.5f32.to_le_bytes(),
            &ctx.values.float("gameplay", key)?.to_le_bytes(),
        )?);
    }
    // Into the `.data` padding, whether or not `shopschanges` is on.
    writes.push(bytes(
        JIGRAN_SLOT,
        &[0; 4],
        &ctx.values.float("gameplay", "jigran")?.to_le_bytes(),
    )?);
    Ok(writes)
}

/// The `shopschanges` field values: `[+0x14]` and `[+0x24]` (0.1, or `jigran` once `shopsnewnormalchanges`
/// wrote its slot).
#[derive(Debug)]
struct ShopFields {
    first: u32,
}

/// The last template of `ClassTemplatesBuild` (0x4A7DE5): the original's added code and the rewritten tail store 1.0,
/// 0.2 and 0.1 into its price and count fields instead of the per-field registers, and set ECX to the template. Its
/// `mov [esp + 0x20], edi` runs after its four pushes, so it stores into the site's `[esp + 0x10]` (which the game
/// overwrites before reading) and leaves `[esp + 0x20]` alone.
extern "C" fn shop_template(arg: usize, ctx: &mut Context) {
    // SAFETY: `arg` is the `ShopFields` the build kept.
    let first = unsafe { common::kept::<ShopFields>(arg) }.first;
    let one = 1f32.to_bits();
    let fifth = 0x3E4C_CCCD;
    let tenth = SHOP_FIELD_DEFAULT.to_bits();
    // SAFETY: ESI is the class template `ClassTemplatesBuild` just built, whose fields up to +0x34 the game's own
    // code stores here with other values.
    let t = unsafe { Obj::new(ctx.esi, 0x34) };
    let fields: [(u32, u32); 9] = [
        (0x10, one),
        (0x14, first),
        (0x18, fifth),
        (0x1C, tenth),
        (0x20, one),
        (0x24, first),
        (0x28, fifth),
        (0x2C, tenth),
        (0x30, fifth),
    ];
    t.set::<u32>(8, 5);
    t.set::<u8>(0xC, 1);
    t.set::<u8>(0xD, 1);
    for (offset, v) in fields {
        t.set(offset, v);
    }
    ctx.set_stack_word(4, ctx.edi);
    ctx.ecx = t.addr();
    ctx.resume = eix_sites::CLASS_TEMPLATES_BUILD__7E5;
}

/// `shopschanges` >= 1: `ClassTemplatesBuild` stores EBP into the price and count fields of four templates instead
/// of the per-field registers, and its last template gets fixed values (see [`shop_template`]).
fn shops_changes(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value < 1 {
        return Ok(Vec::new());
    }
    let first = if ctx.values.flag("gameplay", "shopsnewnormalchanges")? == 1 {
        ctx.values.float("gameplay", "jigran")?.to_bits()
    } else {
        SHOP_FIELD_DEFAULT.to_bits()
    };
    let arg = common::keep(ShopFields { first });
    let mut writes = vec![
        bytes(
            eix_sites::CLASS_TEMPLATES_BUILD__53D,
            &[0x56, 0x20, 0x89, 0x7E, 0x28, 0x89, 0x46, 0x2C, 0x89, 0x7E],
            &[0x6E, 0x20, 0x89, 0x6E, 0x28, 0x89, 0x6E, 0x2C, 0x89, 0x6E],
        )?,
        bytes(eix_sites::CLASS_TEMPLATES_BUILD__518, &[0x4E], &[0x6E])?,
        bytes(eix_sites::CLASS_TEMPLATES_BUILD__41F, &[0x4E], &[0x6E])?,
        bytes(
            eix_sites::CLASS_TEMPLATES_BUILD__448,
            &[0x56, 0x10, 0x89, 0x7E, 0x18, 0x89, 0x46],
            &[0x6E, 0x10, 0x89, 0x6E, 0x18, 0x89, 0x6E],
        )?,
    ];
    writes.push(hook::context_hook(
        eix_sites::CLASS_TEMPLATES_BUILD__7B5,
        &[0x8B, 0xCE, 0x89, 0x7C, 0x24, 0x20],
        Mode::Drop,
        shop_template,
        arg,
    )?);
    Ok(writes)
}

// --- Scripts ---------------------------------------------------------------------------------------------------------

/// `UnitStats`' size in dwords, which `CopyStats` copies whole.
const UNIT_STATS_DWORDS: u32 = 0x1C1;
/// The pieces `copystatscut` copies (`true`) and skips (`false`), in bytes; the game copies the last 0x11
/// dwords itself.
const COPY_STATS_PIECES: [(bool, u32); 13] = [
    (true, 0x34),
    (false, 0x104),
    (true, 0x31 * 4),
    (false, 8),
    (true, 0x3B * 4),
    (false, 8),
    (true, 0x3B * 4),
    (false, 8),
    (true, 0x3B * 4),
    (false, 8),
    (true, 0x3B * 4),
    (false, 8),
    (true, 0x3B * 4),
];

/// `CopyStats` (0x4B8ACB, `mov ecx, 0x1C1` before `rep movsd`): copies the stats in pieces from ESI to EDI and
/// leaves the last 0x11 dwords to the game's `rep movsd`.
extern "C" fn copy_stats(_: usize, ctx: &mut Context) {
    let (mut from, mut to) = (ctx.esi, ctx.edi);
    for (copy, len) in COPY_STATS_PIECES {
        if copy {
            // SAFETY: ESI and EDI are the two units' `UnitStats` (+0x20 of each unit), each 0x704 bytes, and the
            // pieces stay inside them; `copy` handles a unit copied onto itself.
            unsafe {
                core::ptr::copy(
                    from as usize as *const u8,
                    to as usize as *mut u8,
                    len as usize,
                );
            }
        }
        from = from.wrapping_add(len);
        to = to.wrapping_add(len);
    }
    ctx.esi = from;
    ctx.edi = to;
    ctx.ecx = 0x11;
    ctx.resume = eix_sites::SCRIPT_RUN_EXECUTE__2370;
}

/// `copystatscut` = 1: `CopyStats` leaves out the blocks [`COPY_STATS_PIECES`] skips.
fn copy_stats_cut(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let mut expected = [0xB9, 0, 0, 0, 0];
    expected[1..].copy_from_slice(&UNIT_STATS_DWORDS.to_le_bytes());
    Ok(vec![hook::context_hook(
        eix_sites::SCRIPT_RUN_EXECUTE__236B,
        &expected,
        Mode::Drop,
        copy_stats,
        0,
    )?])
}

// --- Load ----------------------------------------------------------------------------------------------------------

/// The stats refresh after `CUnit::TotalItemWeight` (0x524BB8): the original loads `[[ebp + 0xC] + 0x440]`, compares
/// the weight with 0 and acts on nothing; what stays is ECX = `[ebp + 0xC]` and the two replaced loads.
extern "C" fn lift_check(_: usize, ctx: &mut Context) {
    // SAFETY: EBP is the stats refresh's frame, whose argument at +0xC and local at -0x20 the game reads here.
    let frame = unsafe { Obj::new(ctx.ebp.wrapping_sub(0x20), 0x30) };
    // SAFETY: EBX is the refresh's `UnitStats`.
    let stats = unsafe { Obj::new(ctx.ebx, UNIT_STATS) };
    ctx.ecx = frame.get(0x2C);
    ctx.eax = stats.get(0x60);
    ctx.edi = frame.get(0);
    ctx.resume = eix_sites::UNIT_STATS_RECOMPUTE__65E;
}

/// `liftchanges` = 1: the hook of [`lift_check`], which changes no game state.
fn lift_changes(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(common::jump_at(
        eix_sites::UNIT_STATS_RECOMPUTE__658,
        &[0x8B, 0x43, 0x60, 0x8B, 0x7D, 0xE0],
        1,
        lift_check,
        0,
    )?)
}

/// The load ratio's debuff: `stat * (1.01 - q / (q + liftkey))` with `q = load / maxload`, on the x87 as the original
/// computes it (1.01 is game.exe's float at 0x73CD40).
fn lift_debuff(stat: f32, load: f32, max_load: f32, lift_key: f32) -> f32 {
    let q = Ext::new(load.into()).fdiv(max_load.into()).to_f32();
    let share = Ext::new(q.into())
        .fadd(lift_key.into())
        .fdivr(q.into())
        .to_f32();
    let factor = Ext::new(f64::from(f32::from_bits(0x3F81_47AE)))
        .fsub(share.into())
        .to_f32();
    Ext::new(stat.into()).fmul(factor.into()).to_f32()
}

/// `UnitStats::Pack` at 0x522E26, before `fistp`: a stat scaled by the load ratio while the load's bits are not 0.
extern "C" fn lift_pack(arg: usize, ctx: &mut Context) {
    let lift_key = f32::from_bits(arg as u32);
    // SAFETY: EBX is the `UnitStats` being packed; +0x50 and +0x54 are its load and maximum load.
    let stats = unsafe { Obj::new(ctx.ebx, UNIT_STATS) };
    let load: u32 = stats.get(0x50);
    if load != 0 {
        common::set_st0(lift_debuff(
            common::st0(),
            f32::from_bits(load),
            stats.get(0x54),
            lift_key,
        ));
    }
}

/// `liftdebuffs` = 1: [`lift_pack`]; `liftkey` goes where the original keeps it, in game.exe's data padding.
fn lift_debuffs(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let key = ctx.values.float("gameplay", "liftkey")?;
    Ok(vec![
        // The handler runs before the copied `fistp; mov cl`, which pop the scaled value as the original's own copy
        // does.
        hook::context_hook(
            eix_sites::UNIT_STATS_PACK__146,
            &[0xDB, 0x5D, 0xF4, 0x8A, 0x4D, 0xF4],
            Mode::After,
            lift_pack,
            key.to_bits() as usize,
        )?,
        bytes(eix_sites::DATA_799FFC, &[0; 4], &key.to_le_bytes())?,
    ])
}

/// Header slack where `encumbrancemovespeed` keeps `encumbrancemovemul`.
const MOVE_MUL: Site = Site::new(0x40_02D8);
/// Where it keeps `speedslowmovemul` with `speedslowremaster` = 2.
const SLOW_MOVE_MUL: Site = Site::new(0x40_02CC);
/// Padding in `GameBuffer::ReadBytes` where it keeps `defaultspeedmul` and `minimumspeedmul`.
const DEFAULT_SPEED_MUL: Site = eix_sites::GAME_BUFFER_READ_BYTES__64;
const MINIMUM_SPEED_MUL: Site = eix_sites::GAME_BUFFER_READ_BYTES__68;
/// Header slack where `encumbranceattackspeed` keeps `encumbranceattackmul`.
const ATTACK_MUL: Site = Site::new(0x40_02DC);

/// The multipliers `encumbrancemovespeed` keeps in game.exe.
#[derive(Debug, Clone, Copy)]
struct Muls {
    movement: f32,
    slow: f32,
    default: f32,
    minimum: f32,
}

impl Muls {
    /// # Safety
    /// The image must be mapped, as it is whenever game code runs.
    unsafe fn read() -> Self {
        // SAFETY: header slack and padding of game.exe's mapped image, which only the build writes.
        let at = |a: Site| unsafe { read::<f32>(a.here32()) };
        Self {
            movement: at(MOVE_MUL),
            slow: at(SLOW_MOVE_MUL),
            default: at(DEFAULT_SPEED_MUL),
            minimum: at(MINIMUM_SPEED_MUL),
        }
    }
}

/// `r = load / (load + maxload)` and `r^3`, as the original computes them.
fn load_cubed(load: f32, max_load: f32) -> f64 {
    let r = Ext::new(load.into())
        .fadd(max_load.into())
        .fdivr(load.into())
        .to_f64();
    Ext::new(r).fmul(r).fmul(r).to_f64()
}

/// The move speed of `encumbrancemovespeed`: `base - r^3 * mul`; with `speedslowremaster` = 2 also `+ slow * smul`,
/// `* defaultspeedmul`, at least `minimumspeedmul` (a NaN too becomes the minimum), and `* overload`.
fn move_speed(
    base: f32,
    load: f32,
    max_load: f32,
    mul: f32,
    remaster: Option<(f32, f32, f32, f32, f32)>,
) -> f32 {
    let slowed = Ext::new(base.into())
        .fsub(
            Ext::new(load_cubed(load, max_load))
                .fmul(mul.into())
                .to_f64(),
        )
        .to_f64();
    let Some((slow, slow_mul, default_mul, minimum, overload)) = remaster else {
        return slowed as f32;
    };
    let v = Ext::new(Ext::new(slow.into()).fmul(slow_mul.into()).to_f64())
        .fadd(slowed)
        .fmul(default_mul.into())
        .to_f64();
    let v = if v >= f64::from(minimum) {
        v
    } else {
        minimum.into()
    };
    Ext::new(v).fmul(overload.into()).to_f32()
}

/// The stats refresh's move speed (0x524BDA, after the copied `fld [ebx + 0x24]; fmul [ebx + 0x58]`): replaced by
/// [`move_speed`]. `arg` is 1 with `speedslowremaster` = 2, which leaves its slow-down at `[ebp - 0x14]`.
extern "C" fn move_speed_hook(arg: usize, ctx: &mut Context) {
    // SAFETY: EBX is the `UnitStats` the refresh computes (+0x24 base speed, +0x50 load, +0x54 maximum, +0x58 the
    // overload factor).
    let s = unsafe { Obj::new(ctx.ebx, UNIT_STATS) };
    // SAFETY: the header slack and padding this feature's build wrote its multipliers to.
    let muls = unsafe { Muls::read() };
    let remaster = (arg == 1).then(|| {
        // SAFETY: EBP is the refresh's frame, whose local at -0x14 `speedslowremaster` stores.
        let slow: f32 = unsafe { Obj::new(ctx.ebp.wrapping_sub(0x14), 4) }.get(0);
        (slow, muls.slow, muls.default, muls.minimum, s.get(0x58))
    });
    common::set_st0(move_speed(
        s.get(0x24),
        s.get(0x50),
        s.get(0x54),
        muls.movement,
        remaster,
    ));
}

/// `encumbrancemovespeed` = 1: [`move_speed_hook`] and the multipliers it reads.
fn encumbrance_move_speed(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let v = ctx.values;
    let remaster = v.flag("gameplay", "speedslowremaster")? == 2;
    let mut writes = vec![hook::context_hook(
        eix_sites::UNIT_STATS_RECOMPUTE__67A,
        &[0xD9, 0x43, 0x24, 0xD8, 0x4B, 0x58],
        Mode::Before,
        move_speed_hook,
        usize::from(remaster),
    )?];
    if remaster {
        writes.push(bytes(
            SLOW_MOVE_MUL,
            &[0; 4],
            &v.float("gameplay", "speedslowmovemul")?.to_le_bytes(),
        )?);
    }
    writes.push(bytes(
        MOVE_MUL,
        &[0; 4],
        &v.float("gameplay", "encumbrancemovemul")?.to_le_bytes(),
    )?);
    let mut muls = v
        .float("gameplay", "defaultspeedmul")?
        .to_le_bytes()
        .to_vec();
    muls.extend_from_slice(&v.float("gameplay", "minimumspeedmul")?.to_le_bytes());
    writes.push(bytes(DEFAULT_SPEED_MUL, &[0x90; 8], &muls)?);
    Ok(writes)
}

/// `CUnit::AnimDurationFor` (0x5327E3, after the copied attack factor load): the attack factor becomes
/// `(factor - r^3) * mul`, the load from the unit's stats (`[[ecx + 0x240] + 0x230]`, `+0x234`).
extern "C" fn attack_factor(_: usize, ctx: &mut Context) {
    // SAFETY: ECX is the unit `AnimDurationFor` runs on.
    let unit = unsafe { Obj::new(ctx.ecx, UNIT) };
    // SAFETY: the unit's +0x240 is its server half, whose `UnitStats` at +0x1E0 hold the load at +0x50 (+0x230).
    let stats = unsafe { Obj::new(unit.get::<u32>(0x240).wrapping_add(0x1E0), UNIT_STATS) };
    // SAFETY: the header slack this feature's build wrote `encumbranceattackmul` to.
    let mul = unsafe { read::<f32>(ATTACK_MUL.here32()) };
    let v = Ext::new(common::st0().into())
        .fsub(load_cubed(stats.get(0x50), stats.get(0x54)))
        .fmul(mul.into())
        .to_f32();
    common::set_st0(v);
}

/// The stats refresh at 0x524BFE (after the copied `fadd`, of 10 or `actionsadd`): `(x + add) * (speed - r^3 *
/// mul)`.
extern "C" fn attack_rate(_: usize, ctx: &mut Context) {
    // SAFETY: EBX is the `UnitStats` the refresh computes, as in `move_speed_hook`.
    let s = unsafe { Obj::new(ctx.ebx, UNIT_STATS) };
    // SAFETY: the header slack this feature's build wrote `encumbranceattackmul` to.
    let mul = unsafe { read::<f32>(ATTACK_MUL.here32()) };
    let factor = Ext::new(s.get::<f32>(0x24).into())
        .fsub(
            Ext::new(load_cubed(s.get(0x50), s.get(0x54)))
                .fmul(mul.into())
                .to_f64(),
        )
        .to_f64();
    common::set_st0(Ext::new(common::st0().into()).fmul(factor).to_f32());
}

/// `encumbranceattackspeed`: 1 slows attack animations by the load and has `AdvanceTime` apply its speed factor in
/// motion mode 1 instead of 2; 2 slows the stats' attack rate instead. Each hook copies the instruction as
/// `rewriteactionsspeed` (1) or `actionspatch` (2) left it, so the slowing applies to that feature's value (the
/// original's hook copies the game's instruction, dropping that feature's value there).
fn encumbrance_attack_speed(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    let mul = ctx
        .values
        .float("gameplay", "encumbranceattackmul")?
        .to_le_bytes();
    match value {
        1 => {
            let speeds = ctx.values.flag("animationsspeed", "rewriteactionsspeed")?;
            let (site, load) = rebalance::animation_load(speeds, 0);
            Ok(vec![
                bytes(
                    eix_sites::C_UNIT_CLIENT_SPECIFIC_ADVANCE_TIME__143,
                    &[0x02, 0x75],
                    &[0x01, 0x74],
                )?,
                hook::context_call(site, &load, Mode::Before, attack_factor, 0)?,
                bytes(ATTACK_MUL, &[0; 4], &mul)?,
            ])
        }
        2 => {
            let actions = ctx.values.flag("gameplay", "actionspatch")?;
            let (site, add) = rebalance::actions_add(actions);
            Ok(vec![
                bytes(ATTACK_MUL, &[0; 4], &mul)?,
                hook::context_call(site, &add, Mode::Before, attack_rate, 0)?,
            ])
        }
        _ => Ok(Vec::new()),
    }
}

// --- The weapon bar -------------------------------------------------------------------------------------------------

type Highlight = extern "thiscall" fn(bar: u32, slot: i32, on: i32) -> u32;
type PlaySound = extern "thiscall" fn(interface: u32, name: u32, arg: u32) -> u32;
type LocalView = extern "thiscall" fn(world: u32) -> u32;
type SetWeapon = extern "thiscall" fn(client: u32, unit: u32, slot: i32) -> u32;

const HIGHLIGHT: Site = eix_sites::C_HUD_WEAPON_BAR_HIGHLIGHT;
const PLAY_SOUND: Site = eix_sites::C_INTERFACE_3D_PLAY_SOUND;
const LOCAL_VIEW: Site = eix_sites::C_WORLD_CLIENT_LOCAL_VIEW;
const SET_WEAPON: Site = eix_sites::C_PLAYER_CLIENT_SET_WEAPON;
/// `CWorldClient`'s static instance, `LocalView`'s `this` in the bar's own code.
const CLIENT_WORLD: Site = eix_sites::DATA_7AF5B8;
/// The weapon-select sound's name, as the bar passes it.
const SELECT_SOUND: Site = eix_sites::DATA_794444;
/// `BattleScreenKeyDown`'s common exit, and the jump to it after the bar's `SetWeapon`.
const KEY_DOWN_EXIT: Site = eix_sites::BATTLE_SCREEN_KEY_DOWN__F64;
const KEY_DOWN_AFTER_SET_WEAPON: Site = eix_sites::BATTLE_SCREEN_KEY_DOWN__764;

/// The weapon keys 1-4 of `BattleScreenKeyDown`, taken when the key's slot is already selected: the original
/// unhighlights it, plays the select sound, stores slot 1 as the bar's selection (`mov [esi + 0x150], 1`) and has the
/// local view set the unit's weapon to -1, sheathing it.
extern "C" fn sheathe_by_key(_: usize, ctx: &mut Context) {
    // SAFETY: ESI is the HUD weapon bar the key handler works on; +0x98 is its interface, +0x150 its selected slot,
    // +0x154 its unit.
    let bar = unsafe { Obj::new(ctx.esi, WEAPON_BAR) };
    // SAFETY: the game's functions, with the conventions their bodies show (thiscall; `ret 8`, `ret 8`, `ret`,
    // `ret 8`).
    let highlight: Highlight = unsafe { hook::as_fn(HIGHLIGHT) };
    // SAFETY: as above.
    let play_sound: PlaySound = unsafe { hook::as_fn(PLAY_SOUND) };
    // SAFETY: as above.
    let local_view: LocalView = unsafe { hook::as_fn(LOCAL_VIEW) };
    // SAFETY: as above.
    let set_weapon: SetWeapon = unsafe { hook::as_fn(SET_WEAPON) };
    bar.set::<i32>(0x150, -1);
    highlight(bar.addr(), ctx.eax as i32, 0);
    highlight(bar.addr(), -1, 1);
    let interface = bar.get::<u32>(0x98);
    bar.set::<i32>(0x150, 1);
    ctx.eax = play_sound(interface, SELECT_SOUND.here32(), 0);
    ctx.edi = u32::MAX;
    ctx.esi = bar.get(0x154);
    if ctx.esi == 0 {
        ctx.resume = KEY_DOWN_EXIT;
        return;
    }
    let client = local_view(CLIENT_WORLD.here32());
    ctx.ecx = client;
    ctx.eax = set_weapon(client, ctx.esi, -1);
    ctx.resume = KEY_DOWN_AFTER_SET_WEAPON;
}

/// `WeaponBarMouseUpVerify`'s click on the selected slot, which the game ignored: the original makes the click select
/// slot -1 (EDI and `[esi + 0x150]`) and goes on with the bar's own selection code at 0x63C77B.
extern "C" fn sheathe_by_click(_: usize, ctx: &mut Context) {
    ctx.edi = u32::MAX;
    // SAFETY: ESI is the HUD weapon bar the click verifies.
    unsafe { Obj::new(ctx.esi, WEAPON_BAR) }.set::<i32>(0x150, -1);
    ctx.resume = eix_sites::WEAPON_BAR_MOUSE_UP_VERIFY__6B;
}

/// The `je 0x63B4C4` of each weapon key when its slot is selected: `(the jump's rel32 address, its game bytes)`.
const KEY_SELECTED_JUMPS: [(Site, [u8; 4]); 4] = [
    (
        eix_sites::BATTLE_SCREEN_KEY_DOWN__70F,
        [0x51, 0x08, 0x00, 0x00],
    ),
    (
        eix_sites::BATTLE_SCREEN_KEY_DOWN__789,
        [0xD7, 0x07, 0x00, 0x00],
    ),
    (
        eix_sites::BATTLE_SCREEN_KEY_DOWN__7F9,
        [0x67, 0x07, 0x00, 0x00],
    ),
    (
        eix_sites::BATTLE_SCREEN_KEY_DOWN__869,
        [0xF7, 0x06, 0x00, 0x00],
    ),
];

/// `heroscanhideweapons` = 1: a weapon key or a click on the selected slot sheathes the weapon.
fn heroes_can_hide_weapons(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let mut writes = vec![
        // `je 0x63C7C4` becomes `je 0x63C7CA`, the NOP before the padding the hook takes.
        bytes(eix_sites::WEAPON_BAR_MOUSE_UP_VERIFY__62, &[0x51], &[0x57])?,
        hook::context_hook(
            eix_sites::WEAPON_BAR_MOUSE_UP_VERIFY__BB,
            &[0x90; 5],
            Mode::Drop,
            sheathe_by_click,
            0,
        )?,
    ];
    let entry = common::entry(sheathe_by_key, 0)?;
    for (at, game) in KEY_SELECTED_JUMPS {
        writes.push(bytes(at, &game, &common::rel32(at.offset(4), entry))?);
    }
    Ok(writes)
}

// --- Stealing ------------------------------------------------------------------------------------------------------

/// `HooverCollectLoot` 0x461CB0 (`ret 0x18`; EBP's frame: +0x8 the world, +0xC the unit, +0x10 the drop list, +0x1C
/// the gold, -0x1 the unit's hero test, -0x8 the item source): the looted test's `xor eax, eax`, which ours turns
/// into a jump to the `push ebx; push edi` the other way takes.
const HOOVER_LOOTED: Site = eix_sites::HOOVER_COLLECT_LOOT__1F;
/// `mov eax, [ecx]; call [eax + 0x48]` (ECX the world, ESI the unit): the item source.
const HOOVER_SOURCE: Site = eix_sites::HOOVER_COLLECT_LOOT__2D;
const HOOVER_SOURCE_WAS: [u8; 5] = [0x8B, 0x01, 0xFF, 0x50, 0x48];
/// The epilogue after `pop esi`, where the looted test returns 0.
const HOOVER_RETURN: Site = eix_sites::HOOVER_COLLECT_LOOT__22;
/// The hero test before a hero's armour and weapons (`mov al, [ebp - 1]`; ESI the drop list, ECX the unit).
const HOOVER_ARMOUR: Site = eix_sites::HOOVER_COLLECT_LOOT__366;
/// The unit's looted flag in its +0x8, which the routine's end sets.
const LOOTED: u32 = 0x10_0000;

/// [`HOOVER_SOURCE`] under `improvedstealing`: the original's three copies of the routine, as the game's own one
/// steered, by the looted flag and the dead flag (+0x274 = 1):
/// - looted and alive: 0, as the game returns;
/// - not looted, dead: the game's whole loot;
/// - not looted, alive (stealing): the same without a hero's armour and weapons;
/// - looted, dead: no gold and only a hero's armour and weapons, the flag set again.
extern "C" fn hoover_case(_: usize, ctx: &mut Context) {
    let unit = ctx.esi;
    // SAFETY: ESI is the `CUnit` the routine collects from.
    let u = unsafe { Obj::new(unit, UNIT) };
    let looted = u.get::<u32>(0x8) & LOOTED != 0;
    let dead = u.get::<i32>(0x274) == 1;
    // SAFETY: the routine's frame, from its saved ESI at EBP-0x10 to its arguments' end.
    let frame = unsafe { Obj::new(ctx.ebp - 0x10, 0x10 + 0x20) };
    if looted && !dead {
        ctx.eax = 0;
        ctx.esi = frame.get(0);
        ctx.resume = HOOVER_RETURN;
        return;
    }
    type ItemSource = unsafe extern "thiscall" fn(world: u32) -> u32;
    let world = ctx.ecx;
    // SAFETY: ECX is the world the routine was given; its first word is its vtable.
    let vtable = unsafe { read::<u32>(world) };
    // SAFETY: the vtable's slot 0x48, which the routine calls.
    let slot = unsafe { read::<u32>(vtable + 0x48) };
    // SAFETY: that slot's function takes the world and nothing else.
    let source = unsafe { hook::as_fn_at::<ItemSource>(slot as usize) };
    // SAFETY: the world the routine was given, on the game's thread.
    ctx.eax = unsafe { source(world) };
    match (looted, dead) {
        (false, false) => frame.set::<u8>(0x10 - 1, 0),
        (true, _) => {
            // The game's stores before its sections: the item source and no gold.
            frame.set(0x10 - 8, ctx.eax);
            // SAFETY: the routine's gold out-parameter.
            unsafe { common::write::<u32>(frame.get(0x10 + 0x1C), 0) };
            ctx.esi = frame.get(0x10 + 0x10);
            ctx.ecx = unit;
            ctx.edi = 0;
            ctx.resume = HOOVER_ARMOUR;
        }
        (false, true) => {}
    }
}

/// `improvedstealing` = 1: looting a body and stealing from the living take different things ([`hoover_case`]).
fn improved_stealing(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    Ok(vec![
        // `xor eax, eax` -> `jmp 0x461CD8`.
        bytes(HOOVER_LOOTED, &[0x33, 0xC0], &[0xEB, 0x07])?,
        hook::context_hook(
            HOOVER_SOURCE,
            &HOOVER_SOURCE_WAS,
            Mode::Drop,
            hoover_case,
            0,
        )?,
    ])
}

// --- The camp ------------------------------------------------------------------------------------------------------

/// `CHeroStatsPanel::DrawPart`'s loop at 0x6246E0 that keeps the largest of the body parts' absorptions.
const AVG_ARMORS_WAS: [u8; 51] = [
    0x8B, 0x0A, 0xD9, 0x5D, 0x10, 0x89, 0x4D, 0xE0, 0xD9, 0x45, 0xE0, 0xD8, 0x5D, 0x10, 0xDF, 0xE0,
    0x8B, 0x4D, 0x10, 0xC1, 0xE0, 0x17, 0xC1, 0xF8, 0x1F, 0x23, 0xC8, 0xF7, 0xD0, 0x23, 0x45, 0xE0,
    0x0B, 0xC1, 0x89, 0x45, 0x0C, 0xD9, 0x45, 0x0C, 0x83, 0xC2, 0x04, 0x4E, 0x75, 0xD2, 0xDD, 0xD8,
    0xD9, 0x45, 0x0C,
];

/// The original's rewrite: the loop sums the absorptions (`fld [edx]; faddp`), the sum is divided by the count the
/// panel lists (`fild` of `mov esi, 6`'s operand, which `dmgtypes` sets), and with a unit's stats in `[esp + 8]` (into
/// EBX) the call at +42 adds what the toughness perk gives.
const AVG_ARMORS_NOW: [u8; 51] = [
    0xD9, 0x02, 0xDE, 0xC1, 0x83, 0xC2, 0x04, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90, 0x90, 0x90, 0x90, 0x4E, 0x75, 0xE9, 0xDB, 0x05, 0xDC, 0x46, 0x62, 0x00, 0xDC, 0xF9, 0xDD,
    0xD8, 0x8B, 0x5C, 0x24, 0x08, 0x83, 0xFB, 0x00, 0x74, 0x05, 0xE8, 0, 0, 0, 0, 0x90, 0x90, 0x90,
    0x90,
];
const AVG_ARMORS: Site = eix_sites::C_HERO_STATS_PANEL_DRAW_PART__150;
/// Where the rewrite calls `newperks`' helper (+42) and the instruction after the call.
const AVG_ARMORS_CALL: usize = 42;
const AVG_ARMORS_AFTER_CALL: Site = AVG_ARMORS.offset(AVG_ARMORS_CALL).offset(5);
/// The toughness perk that `newperks` adds.
const TOUGHNESS_PERK: u32 = 0x17;

/// `newperks`' helper the average calls (EBX the unit's `UnitStats`): the value of the unit's toughness level is
/// added to the average on the x87 stack.
extern "C" fn toughness_armor(_: usize, ctx: &mut Context) {
    let stats = core::ptr::with_exposed_provenance_mut::<UnitStats>(ctx.ebx as usize);
    // SAFETY: the rewrite calls only with a non-null `[esp + 8]`, the drawn unit's live `UnitStats`.
    let value = unsafe { UnitStats::skill_level_value(stats, TOUGHNESS_PERK) };
    common::set_st0(Ext::new(common::st0().into()).fadd(value.into()).to_f32());
}

/// `avgarmors` = 1: the camp's left panel shows the average armour instead of the largest. The registry skips it
/// without `newperks`, whose helper the rewrite calls.
fn avg_armors(_: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    let stub = common::call_stub(toughness_armor, 0, AVG_ARMORS_AFTER_CALL)?;
    Ok(vec![common::branch(
        AVG_ARMORS,
        &AVG_ARMORS_WAS,
        &AVG_ARMORS_NOW,
        AVG_ARMORS_CALL,
        stub,
    )?])
}

/// `CInterface3D::OnStringEvent` 0x5F80EF, after the copied `add esp, 8; test eax, eax` of its "say" test: the event
/// "constr" (case-insensitive, as the game matches its other events) leaves for the camp as the battle screen's camp
/// button does, and the function goes to its end. `strip` (`camp` 3): [`enter_camp`]'s.
extern "C" fn camp_event(strip: usize, ctx: &mut Context) {
    let Some(tokens) = ctx.stack_word(5) else {
        return;
    };
    // SAFETY: `[esp + 0x14]` is the event's token array, whose first token the "say" test read.
    let first = unsafe { read::<u32>(tokens) };
    if first == 0 {
        return;
    }
    // SAFETY: a token's characters, NUL-terminated.
    let token =
        unsafe { core::ffi::CStr::from_ptr(core::ptr::with_exposed_provenance(first as usize)) };
    if !token.to_bytes().eq_ignore_ascii_case(b"constr") {
        return;
    }
    enter_camp(strip);
    ctx.resume = eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__1E9;
}

/// While [`enter_camp`] strips player 0's quest items: `TakeLoot`'s hook then leaves their variables alone, as the
/// original's tells by its return address.
static STRIPPING: AtomicBool = AtomicBool::new(false);

/// Leaves the zone for the camp, as the battle screen's camp button does (0x62FE6A). With `strip` (`camp` 3), player
/// 0 first gives up every quest item: `CPlayer::TakeLoot` of each id from 0x1000 down to 1, then
/// `CParty::RemoveQuestItems` of its party (+0x70).
fn enter_camp(strip: usize) {
    if strip != 0
        && let Some(player) = common::player_zero()
    {
        type TakeLoot = unsafe extern "thiscall" fn(player: u32, id: i32) -> u32;
        type RemoveQuestItems = unsafe extern "thiscall" fn(party: u32);
        let player = player as u32;
        // SAFETY: `CPlayer::TakeLoot` 0x665780 (`ret 4`).
        let take = unsafe { hook::as_fn::<TakeLoot>(eix_sites::C_PLAYER_TAKE_LOOT) };
        STRIPPING.store(true, Ordering::Relaxed);
        for id in (1..=0x1000).rev() {
            // SAFETY: player 0, on the game's thread.
            unsafe { take(player, id) };
        }
        STRIPPING.store(false, Ordering::Relaxed);
        // SAFETY: `CParty::RemoveQuestItems` 0x661FB0 (no arguments).
        let remove =
            unsafe { hook::as_fn::<RemoveQuestItems>(eix_sites::C_PARTY_REMOVE_QUEST_ITEMS) };
        // SAFETY: player 0's party.
        unsafe { remove(player + 0x70) };
    }
    // SAFETY: the game's camp entry, on its thread with a zone loaded (an event or a key is being handled).
    unsafe { flow::set_construction_variable(0.0) };
    // SAFETY: as above.
    unsafe { flow::send_leave(5) };
}

/// While player 0's variable "Camp" is not 0 (0 also without player 0): leaves for the camp ([`enter_camp`] with
/// `strip`) and sets "Camp" to 2. Returns whether it did.
fn camp_if_open(strip: usize) -> bool {
    // The original's `fcomp 0; test ah, 0x40` takes an unordered value as 0 too.
    let open = common::get_variable(c"Camp");
    if open == 0.0 || open.is_nan() {
        return false;
    }
    enter_camp(strip);
    common::set_variable(c"Camp", 2.0);
    true
}

/// Player 0's variable "q<id>" for the quest item at `item` (its id at +0x4C) becomes `value`.
///
/// # Safety
/// `item` must be a live item.
unsafe fn quest_variable(item: u32, value: f32) {
    // SAFETY: the caller vouches for the item.
    let id = unsafe { read::<i32>(item + 0x4C) };
    let mut name = [0; common::ID_NAME_LEN];
    common::set_variable(common::id_variable(b"q", id, &mut name), value);
}

/// The item type of quest items.
const QUEST_ITEM: u32 = 0x3009;

/// `CWorldServer::CreateItemNamed` after creating the item (0x594B2D, EAX and EDI the item, ESI the world): a quest
/// item sets player 0's "q<id>" to 2 (`camp` 3); then the replaced `mov al, [esi + 0x100]`. The original skips that
/// load when it sets the variable, so the game branches on the item pointer's low byte, and returns to address 0
/// without player 0; ours does neither.
extern "C" fn quest_item_made(_: usize, ctx: &mut Context) {
    let item = ctx.eax;
    // SAFETY: a created item's type at +0x10.
    let quest = item != 0 && unsafe { read::<u32>(item + 0x10) } == QUEST_ITEM;
    if quest && common::player_zero().is_some() {
        // SAFETY: as above.
        unsafe { quest_variable(item, 2.0) };
    }
    // SAFETY: ESI is the server world, whose +0x100 the replaced instruction loads.
    let flag = unsafe { read::<u8>(ctx.esi + 0x100) };
    ctx.eax = (ctx.eax & !0xFF) | u32::from(flag);
}

/// `CPlayer::TakeLoot` on the quest item it found (0x6657B9, EBX the item, ESI the player): player 0's "q<id>"
/// becomes 0 unless [`enter_camp`] strips the items (`camp` 3); then the replaced `mov ecx, [esi + 0x20];
/// mov eax, [ecx]`, with the `push ebx` between them moved after, as the original's. Without player 0 the original
/// skips those loads and calls through garbage; ours loads them.
extern "C" fn quest_item_taken(_: usize, ctx: &mut Context) {
    if !STRIPPING.load(Ordering::Relaxed) && common::player_zero().is_some() {
        // SAFETY: EBX is the quest item `TakeLoot` found.
        unsafe { quest_variable(ctx.ebx, 0.0) };
    }
    // SAFETY: ESI is the player; its +0x20 is its server half, whose first word is its vtable.
    ctx.ecx = unsafe { read::<u32>(ctx.esi + 0x20) };
    // SAFETY: as above.
    ctx.eax = unsafe { read::<u32>(ctx.ecx) };
}

/// `CHudQuickItems::SetUnit`'s loop over the unit's quest items (0x63D6C5, ECX the slot, 0 to 3, ESI the item):
/// with player 0, the panel stops at the slot that its "QuIs", rounded and compared unsigned, does not exceed (its end
/// at 0x63D97F); then the replaced `mov eax, [esi]; lea ecx, [esp + 0x30]`. The author: keep "QuIs" at the count of
/// active key items, or items show twice on leaving the camp [D: forum].
extern "C" fn quest_item_slot(_: usize, ctx: &mut Context) {
    if common::player_zero().is_some() {
        let shown = Ext::new(f64::from(common::get_variable(QUEST_ITEMS_SHOWN))).fistp() as u32;
        if shown <= ctx.ecx {
            ctx.resume = eix_sites::C_HUD_QUICK_ITEMS_SET_UNIT__6CF;
            return;
        }
    }
    // SAFETY: ESI is the quest item; its first word is its vtable.
    ctx.eax = unsafe { read::<u32>(ctx.esi) };
    ctx.ecx = ctx.stack() as u32 + 0x30;
}

/// `camp` 4's variable, its name in `ClampUnit`'s padding at 0x40230B as the original's.
const QUEST_ITEMS_SHOWN: &core::ffi::CStr = c"QuIs";

/// `camp` 4's quick-panel limit ([`quest_item_slot`]).
fn quest_item_slots() -> Result<Vec<SiteWrite>, BuildError> {
    let mut writes = vec![bytes(
        eix_sites::CLAMP_UNIT__3B,
        &[0x90; 5],
        QUEST_ITEMS_SHOWN.to_bytes_with_nul(),
    )?];
    writes.extend(common::call_at(
        eix_sites::C_HUD_QUICK_ITEMS_SET_UNIT__415,
        &[0x8B, 0x06, 0x8D, 0x4C, 0x24, 0x30],
        1,
        quest_item_slot,
        0,
    )?);
    Ok(writes)
}

/// `CBattleScreen::Create` before the replaced `mov edx, [0x79B2B0]` (0x635A4D): the zone music comes back to the local
/// player's map after a stay in the camp (`camp` 13 up): its theme (the record's +4) with the mode "client world's
/// +0x6E0 is 1", the music system's +0x44 that word and +0x38 1. The original's null test compares the map with an
/// unrelated register, builds its string below the stack pointer and calls without a music system; ours checks both.
extern "C" fn camp_music(_: usize, _: &mut Context) {
    // SAFETY: the client world's +0x6E0, a word of game.exe's `.data` (0x7AF5B8 + 0x6E0).
    let zone = unsafe { read::<u32>(eix_sites::DATA_7AF5B8.offset(0x6E0).here32()) };
    common::zone_music(4, i32::from(zone == 1), zone, 1);
}

/// `camp` 13 up's music hook ([`camp_music`]), a jump over the 6-byte load as the original's.
fn camp_music_hook() -> Result<Vec<SiteWrite>, BuildError> {
    Ok(vec![hook::context_hook(
        eix_sites::C_BATTLE_SCREEN_CREATE__7FD,
        &[0x8B, 0x15, 0xB0, 0xB2, 0x79, 0x00],
        Mode::After,
        camp_music,
        0,
    )?])
}

/// `camp` 3's quest-item hooks ([`quest_item_made`], [`quest_item_taken`]).
fn quest_item_hooks() -> Result<Vec<SiteWrite>, BuildError> {
    let mut writes = common::call_at(
        eix_sites::C_WORLD_SERVER_CREATE_ITEM_NAMED__7D,
        &[0x8A, 0x86, 0x00, 0x01, 0x00, 0x00],
        0,
        quest_item_made,
        0,
    )?;
    writes.extend(common::call_at(
        eix_sites::C_PLAYER_TAKE_LOOT__39,
        &[0x8B, 0x4E, 0x20, 0x53, 0x8B],
        0,
        quest_item_taken,
        0,
    )?);
    // The `push ebx` after the call, over `mov eax, [ecx]`'s second byte.
    writes.push(bytes(eix_sites::C_PLAYER_TAKE_LOOT__3E, &[0x01], &[0x53])?);
    Ok(writes)
}

/// The end of `BattleScreenKeyDown`'s action switch.
const KEY_DOWN_END: Site = eix_sites::BATTLE_SCREEN_KEY_DOWN__F64;

/// `BattleScreenKeyDown`'s action 11 (`camera_track`, by default the Home key), after the game's own work on the unit
/// it found (0x63B3D9, `jmp` to the end): the key also opens the camp ([`camp_if_open`]). The original returns into
/// the next action's code when there is no player 0; ours goes to the end.
extern "C" fn camp_key(strip: usize, ctx: &mut Context) {
    ctx.resume = KEY_DOWN_END;
    camp_if_open(strip);
}

/// The battle screen's action 60 ("camp", which `newoptions` adds to the key bindings), as the original's copy of
/// action 11's case (ESI the screen): with a unit of flag 0x10 in its selection list (+0xD8, +0xDC its count), the
/// camp ([`camp_if_open`]) instead of the camera's tracking; without, the camera's reset as action 11's.
extern "C" fn camp_action(strip: usize, ctx: &mut Context) {
    ctx.resume = KEY_DOWN_END;
    // SAFETY: ESI is the battle screen the key goes to.
    let screen = unsafe { Obj::new(ctx.esi, 0xE0) };
    let count: i32 = screen.get(0xDC);
    let list: u32 = screen.get(0xD8);
    let found = (0..count.max(0) as u32).any(|i| {
        // SAFETY: the screen's list of `count` unit pointers.
        let unit = unsafe { read::<u32>(list + 4 * i) };
        // SAFETY: a listed unit's flags.
        let flags = unsafe { read::<u32>(unit + 8) };
        flags & 0x10 != 0
    });
    if found {
        camp_if_open(strip);
        return;
    }
    type Reset = unsafe extern "thiscall" fn(camera: u32, target: i32);
    // SAFETY: `CCamera` 0x40D420 (thiscall, `ret 4`), the call action 11 makes without a unit.
    let reset = unsafe { hook::as_fn::<Reset>(eix_sites::C_CAMERA_SET_TARGET_ID) };
    // SAFETY: the game's camera, on its thread.
    unsafe { reset(CAMERA.here32(), -1) };
}

/// The game's camera (`TheCamera`).
const CAMERA: Site = eix_sites::DATA_79B2A8;

/// `CHudActionColumn::SetUnit`'s end (ESI the column): one more action, the camp ("initqi1item", the icon
/// "skill0008"), with its tip (text 0x2910) on its region, as the original's arena code adds it.
extern "C" fn camp_button(_: usize, ctx: &mut Context) {
    type AddAction = unsafe extern "thiscall" fn(column: u32, name: u32, icon: u32, flag: i32);
    type SetTip = unsafe extern "thiscall" fn(region: u32, text: u32, a: u32, b: u32);
    let column = ctx.esi;
    // SAFETY: `CHudActionColumn::AddAction` 0x63F500 (`ret 0xC`); the strings are the game's.
    let add = unsafe { hook::as_fn::<AddAction>(eix_sites::C_HUD_ACTION_COLUMN_ADD_ACTION) };
    // SAFETY: the column being set up, on the game's thread.
    unsafe {
        add(
            column,
            eix_sites::DATA_79457C.here32(),
            eix_sites::DATA_794564.here32(),
            -1,
        )
    };
    // SAFETY: the column's region list (+0x128) and its count (+0x14C).
    let c = unsafe { Obj::new(column, 0x150) };
    let count: u32 = c.get(0x14C);
    // SAFETY: the list's last region, the one just added.
    let region = unsafe { read::<u32>(c.get::<u32>(0x128) + 4 * count - 4) };
    // SAFETY: `CUiRegion::SetTip` 0x5FC730 (`ret 0xC`).
    let tip = unsafe { hook::as_fn::<SetTip>(eix_sites::C_UI_REGION_SET_TIP) };
    // SAFETY: that region.
    unsafe { tip(region, 0x2910, 1, 0x3C) };
}

/// `ActionColumnMouseUpVerify` on the action at index 2, the camp button (ESI the column's region, its +0xC0 the
/// column's unit): without player 0 the game's selection of it; else the camp when open ([`camp_if_open`]), then the
/// camera on the unit, and no selection either way.
extern "C" fn camp_click(strip: usize, ctx: &mut Context) {
    /// The selection of the clicked action, and the function's end past it.
    const SELECT: Site = eix_sites::ACTION_COLUMN_MOUSE_UP_VERIFY__4E;
    const END: Site = eix_sites::ACTION_COLUMN_MOUSE_UP_VERIFY__5B;
    if common::player_zero().is_none() {
        ctx.resume = SELECT;
        return;
    }
    ctx.resume = END;
    if !camp_if_open(strip) || common::player_zero().is_none() {
        return;
    }
    type SetTarget = unsafe extern "thiscall" fn(camera: u32, unit: u32);
    // SAFETY: the region's +0xC0, the column's unit.
    let unit = unsafe { read::<u32>(ctx.esi + 0xC0) };
    // SAFETY: `CCamera::SetTarget` 0x40D3B0 (`ret 4`).
    let set_target = unsafe { hook::as_fn::<SetTarget>(eix_sites::C_CAMERA_SET_TARGET) };
    // SAFETY: the game's camera, on its thread.
    unsafe { set_target(CAMERA.here32(), unit) };
}

/// The battle screen's action jump table's entries for actions 60 and 61, NOP padding after the game's 59.
const ACTION_60: Site = eix_sites::BATTLE_SCREEN_KEY_DOWN__1068;
const ACTION_61: Site = eix_sites::BATTLE_SCREEN_KEY_DOWN__106C;
const ACTION_PADDING: usize = 0x9090_9090;

/// What `camp` 2 and 5 to 12 add with `newoptions` >= 1: action 60's case ([`camp_action`]), the camp button at the end
/// of `CHudActionColumn::SetUnit` ([`camp_button`], its epilogue moved down as the original's, the early way's `je`
/// with it) and its click in `ActionColumnMouseUpVerify` ([`camp_click`], its tail moved likewise, three branches with
/// it).
fn camp_controls(strip: usize) -> Result<Vec<SiteWrite>, BuildError> {
    const SET_UNIT_END_WAS: [u8; 35] = [
        0x5D, 0x5B, 0x8B, 0x4C, 0x24, 0x2C, 0x5F, 0x5E, 0x64, 0x89, 0x0D, 0x00, 0x00, 0x00, 0x00,
        0x83, 0xC4, 0x30, 0xC2, 0x08, 0x00, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
        0x90, 0x90, 0x90, 0x90, 0x90,
    ];
    const SET_UNIT_END_NOW: [u8; 35] = [
        0xE9, 0, 0, 0, 0, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x5D, 0x5B, 0x8B,
        0x4C, 0x24, 0x2C, 0x5F, 0x5E, 0x64, 0x89, 0x0D, 0x00, 0x00, 0x00, 0x00, 0x83, 0xC4, 0x30,
        0xC2, 0x08, 0x00,
    ];
    const CLICK_WAS: [u8; 29] = [
        0x0C, 0x50, 0x8B, 0xCF, 0xC6, 0x46, 0x08, 0x01, 0xE8, 0x20, 0xFD, 0xFF, 0xFF, 0x5F, 0x5E,
        0xC2, 0x0C, 0x00, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    ];
    // `jge` to the moved tail; `cmp eax, 2; jne +5; call camp_click; nop`; then the selection and the tail.
    const CLICK_NOW: [u8; 29] = [
        0x17, 0x83, 0xF8, 0x02, 0x75, 0x05, 0xE8, 0, 0, 0, 0, 0x90, 0x50, 0x8B, 0xCF, 0xC6, 0x46,
        0x08, 0x01, 0xE8, 0x15, 0xFD, 0xFF, 0xFF, 0x5F, 0x5E, 0xC2, 0x0C, 0x00,
    ];
    let action = common::entry(camp_action, strip)?;
    let button = common::jump_stub(
        common::NOWHERE,
        &[],
        camp_button,
        0,
        eix_sites::C_HUD_ACTION_COLUMN_SET_UNIT__2DB,
    )?;
    let click = common::call_stub(
        camp_click,
        strip,
        eix_sites::ACTION_COLUMN_MOUSE_UP_VERIFY__4E,
    )?;
    let mut writes = vec![SiteWrite::pointer(ACTION_60, ACTION_PADDING, action)];
    writes.extend(common::fixed(&[
        (
            eix_sites::C_HUD_ACTION_COLUMN_SET_UNIT__B3,
            &[0x18],
            &[0x26],
        ),
        (
            eix_sites::ACTION_COLUMN_MOUSE_UP_VERIFY__19,
            &[0x36],
            &[0x41],
        ),
        (
            eix_sites::ACTION_COLUMN_MOUSE_UP_VERIFY__3B,
            &[0x14],
            &[0x1F],
        ),
    ])?);
    writes.push(common::branch(
        eix_sites::C_HUD_ACTION_COLUMN_SET_UNIT__2CD,
        &SET_UNIT_END_WAS,
        &SET_UNIT_END_NOW,
        0,
        button,
    )?);
    writes.push(common::branch(
        eix_sites::ACTION_COLUMN_MOUSE_UP_VERIFY__43,
        &CLICK_WAS,
        &CLICK_NOW,
        6,
        click,
    )?);
    Ok(writes)
}

/// `camp`: 1 the script event `SendStringEvent(0, "constr")` opens the camp ([`camp_event`]); 2 and up also the Home
/// key ([`camp_key`]), which 2 to 12 leave out with `newoptions` >= 1; with `newoptions` = 1 the camp's own key
/// binding and a button ([`camp_controls`], from the original's `newoptions` block). 3 also tracks quest items
/// ([`quest_item_hooks`]) and strips them on every way into the camp ([`enter_camp`]); 4 limits the quick panel's
/// quest items by "QuIs" ([`quest_item_slots`]); 13 up bring the zone music back with the battle screen
/// ([`camp_music`]).
fn camp(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    let strip = usize::from(value == 3);
    let event = hook::context_hook(
        eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__7F,
        &[0x83, 0xC4, 0x08, 0x85, 0xC0],
        Mode::Before,
        camp_event,
        strip,
    );
    if value < 1 {
        return Ok(Vec::new());
    }
    let mut writes = vec![event?];
    if value == 1 {
        return Ok(writes);
    }
    let options = ctx.values.flag("gameplay", "newoptions")?;
    if value >= 13 || options < 1 {
        writes.extend(common::call_at(
            eix_sites::BATTLE_SCREEN_KEY_DOWN__E79,
            &[0xE9, 0xE6, 0x00, 0x00, 0x00],
            0,
            camp_key,
            strip,
        )?);
    }
    if options == 1 {
        writes.extend(camp_controls(strip)?);
    }
    match value {
        3 => writes.extend(quest_item_hooks()?),
        4 => writes.extend(quest_item_slots()?),
        13.. => writes.extend(camp_music_hook()?),
        _ => {}
    }
    Ok(writes)
}

// --- New options ---------------------------------------------------------------------------------------------------

/// The game's key actions (RU 1.06's table at 0x787070, which `LoadKeyBindings`, `SaveKeyBindings` and the options
/// window walk): each action's id, its name, its options group and its row there.
#[rustfmt::skip]
const KEY_ACTIONS: [[u32; 4]; 60] = [
    [0, eix_sites::K_EMPTY_TEXT.ru32(), 0, 0], [1, eix_sites::DATA_787A30.ru32(), 3, 4], [2, eix_sites::DATA_787A28.ru32(), 3, 3], [3, eix_sites::DATA_787A20.ru32(), 3, 2], [4, eix_sites::DATA_787A1C.ru32(), 3, 6],
    [5, eix_sites::DATA_787A10.ru32(), 6, 1], [6, eix_sites::DATA_787A04.ru32(), 6, 2], [7, eix_sites::DATA_7879F8.ru32(), 6, 3], [8, eix_sites::DATA_7879E8.ru32(), 6, 4], [9, eix_sites::DATA_7879D8.ru32(), 6, 5],
    [10, eix_sites::DATA_7879C8.ru32(), 6, 6], [11, eix_sites::DATA_7879B8.ru32(), 6, 0], [12, eix_sites::DATA_7879AC.ru32(), 6, 8], [13, eix_sites::DATA_7879A4.ru32(), 6, 9], [14, eix_sites::DATA_78799C.ru32(), 6, 10],
    [15, eix_sites::DATA_787994.ru32(), 6, 11], [16, eix_sites::DATA_78798C.ru32(), 6, 12], [17, eix_sites::DATA_787984.ru32(), 9, 0], [18, eix_sites::DATA_78797C.ru32(), 9, 1], [19, eix_sites::DATA_787974.ru32(), 9, 2],
    [20, eix_sites::DATA_78796C.ru32(), 9, 3], [21, eix_sites::DATA_787964.ru32(), 9, 4], [22, eix_sites::DATA_78795C.ru32(), 9, 5], [23, eix_sites::DATA_787954.ru32(), 5, 7], [24, eix_sites::DATA_78794C.ru32(), 5, 6],
    [25, eix_sites::DATA_787944.ru32(), 5, 5], [26, eix_sites::DATA_787940.ru32(), 5, 4], [27, eix_sites::DATA_787938.ru32(), 5, 2], [28, eix_sites::DATA_787930.ru32(), 5, 0], [29, eix_sites::DATA_787924.ru32(), 5, 1],
    [30, eix_sites::DATA_78791C.ru32(), 8, 0], [31, eix_sites::DATA_787914.ru32(), 8, 1], [32, eix_sites::DATA_78790C.ru32(), 8, 2], [33, eix_sites::DATA_787904.ru32(), 8, 3], [34, eix_sites::DATA_7878FC.ru32(), 8, 4],
    [35, eix_sites::DATA_7878F4.ru32(), 8, 5], [36, eix_sites::DATA_7878EC.ru32(), 8, 6], [37, eix_sites::DATA_7878E4.ru32(), 8, 7], [38, eix_sites::DATA_7878DC.ru32(), 7, 0], [39, eix_sites::DATA_7878D4.ru32(), 7, 1],
    [40, eix_sites::DATA_7878CC.ru32(), 7, 2], [41, eix_sites::DATA_7878C4.ru32(), 7, 3], [42, eix_sites::DATA_7878BC.ru32(), 7, 5], [43, eix_sites::DATA_7878B4.ru32(), 7, 6], [44, eix_sites::DATA_7878AC.ru32(), 7, 7],
    [45, eix_sites::DATA_7878A4.ru32(), 7, 8], [46, eix_sites::DATA_787898.ru32(), 10, 3], [47, eix_sites::DATA_78788C.ru32(), 10, 4], [48, eix_sites::DATA_787880.ru32(), 10, 5], [49, eix_sites::DATA_787874.ru32(), 10, 6],
    [50, eix_sites::DATA_787868.ru32(), 10, 0], [51, eix_sites::DATA_78785C.ru32(), 10, 1], [52, eix_sites::DATA_787850.ru32(), 10, 8], [53, eix_sites::DATA_787844.ru32(), 3, 8], [54, eix_sites::DATA_787838.ru32(), 3, 9],
    [55, eix_sites::DATA_78782C.ru32(), 4, 0], [56, eix_sites::DATA_787820.ru32(), 4, 1], [57, eix_sites::DATA_787814.ru32(), 4, 2], [58, eix_sites::DATA_787808.ru32(), 4, 4], [59, eix_sites::DATA_7877F8.ru32(), 10, 10],
];

/// Where `newoptions` writes the new actions' names, padding in `CSceneRenderer::SetFogColourStateImpl`, as the
/// original: "camp" and "kick".
const KEY_NAMES: Site = eix_sites::C_SCENE_RENDERER_SET_FOG_COLOUR_STATE_IMPL__33;

/// The key actions with `newoptions`: the game's 60, then "camp" (60), "kick" (61) and "kick" again (62), in the
/// options' group 5, rows 9 to 11, as the original's table, its names at their RU 1.06 addresses.
const KEY_TABLE_RU: [u32; 63 * 4] = {
    let mut table = [0; 63 * 4];
    let mut i = 0;
    while i < 60 {
        let mut j = 0;
        while j < 4 {
            table[4 * i + j] = KEY_ACTIONS[i][j];
            j += 1;
        }
        i += 1;
    }
    let added = [
        [60, KEY_NAMES.ru32(), 5, 9],
        [61, KEY_NAMES.offset(5).ru32(), 5, 10],
        [62, KEY_NAMES.offset(5).ru32(), 5, 11],
    ];
    let mut k = 0;
    while k < 3 {
        let mut j = 0;
        while j < 4 {
            table[4 * (60 + k) + j] = added[k][j];
            j += 1;
        }
        k += 1;
    }
    table
};

/// [`KEY_TABLE_RU`] as the game reads it once `newoptions` points it here, the names at their place in the running
/// build ([`name_key_table`]). Writable, as the game's own table is.
static KEY_TABLE: [AtomicU32; 63 * 4] = {
    let mut table = [const { AtomicU32::new(0) }; 63 * 4];
    let mut i = 0;
    while i < 63 * 4 {
        table[i] = AtomicU32::new(KEY_TABLE_RU[i]);
        i += 1;
    }
    table
};

/// Points each action's name word of [`KEY_TABLE`] at the name in the running build.
fn name_key_table() {
    for (slot, &ru) in KEY_TABLE.iter().zip(&KEY_TABLE_RU).skip(1).step_by(4) {
        slot.store(Site::new(ru as usize).here32(), Ordering::Relaxed);
    }
}

/// The game's table, and each reference to it that `newoptions` moves: the site of the address and the offset it then
/// points at in [`KEY_TABLE`] (the original's ends: 63 entries for the bindings and the options' build, 62 for the
/// options' group display).
const KEY_TABLE_GAME: Site = eix_sites::K_ACTION_DEFINITIONS;
const KEY_TABLE_REFS: [(Site, Site, usize); 10] = [
    (
        eix_sites::LOAD_KEY_BINDINGS__12B,
        eix_sites::DATA_787074,
        0x4,
    ),
    (
        eix_sites::LOAD_KEY_BINDINGS__14C,
        eix_sites::DATA_787434,
        0x3F4,
    ),
    (
        eix_sites::LOAD_KEY_BINDINGS__168,
        eix_sites::K_ACTION_DEFINITIONS,
        0x0,
    ),
    (
        eix_sites::SAVE_KEY_BINDINGS__DD,
        eix_sites::K_ACTION_DEFINITIONS,
        0x0,
    ),
    (
        eix_sites::SAVE_KEY_BINDINGS__ED,
        eix_sites::K_OPTION_DEFINITIONS,
        0x3F0,
    ),
    (
        eix_sites::SAVE_KEY_BINDINGS__101,
        eix_sites::DATA_787074,
        0x4,
    ),
    (
        eix_sites::C_OPTIONS_WINDOW_BUILD_BINDINGS__5B,
        eix_sites::DATA_787084,
        0x14,
    ),
    (
        eix_sites::C_OPTIONS_WINDOW_BUILD_BINDINGS__27F,
        eix_sites::DATA_787434,
        0x3F4,
    ),
    (
        eix_sites::C_OPTIONS_WINDOW_SHOW_GROUP__109,
        eix_sites::DATA_78708C,
        0x1C,
    ),
    (
        eix_sites::C_OPTIONS_WINDOW_SHOW_GROUP__183,
        eix_sites::DATA_78743C,
        0x3EC,
    ),
];
const _: () = assert!(KEY_TABLE_GAME.ru() == KEY_TABLE_REFS[2].1.ru());

/// `newoptions` = 1: the key bindings gain "camp" and "kick", and the battle screen passes actions up to 61 to its
/// switch (`cmp eax, 0x3A` -> `0x3C`). Actions 60 and 61's entries of the switch's table are NOP padding in the
/// original, which jumps there when a key is bound to them; ours ends the switch, unless `camp` builds action 60.
fn new_options(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError> {
    if value != 1 {
        return Ok(Vec::new());
    }
    name_key_table();
    let table = KEY_TABLE.as_ptr() as usize;
    let mut writes: Vec<SiteWrite> = KEY_TABLE_REFS
        .iter()
        .map(|&(site, was, offset)| SiteWrite::pointer(site, was.ru(), table + offset))
        .collect();
    writes.push(bytes(KEY_NAMES, &[0x90; 13], b"camp\0kick\0\0\0\0")?);
    writes.push(bytes(
        eix_sites::BATTLE_SCREEN_KEY_DOWN__C4,
        &[0x3A],
        &[0x3C],
    )?);
    let camp_builds_it = ctx.values.flag("gameplay", "camp")? >= 2;
    if !camp_builds_it {
        writes.push(SiteWrite::pointer(
            ACTION_60,
            ACTION_PADDING,
            KEY_DOWN_END.ru(),
        ));
    }
    writes.push(SiteWrite::pointer(
        ACTION_61,
        ACTION_PADDING,
        KEY_DOWN_END.ru(),
    ));
    Ok(writes)
}

#[cfg(test)]
mod tests {
    use super::super::testing::{assert_original, no_log, plain, values};
    use super::*;

    fn sites(writes: &[SiteWrite]) -> Vec<(Site, usize)> {
        writes.iter().map(|w| (w.site(), w.size())).collect()
    }

    #[test]
    fn skill_and_stealing_hooks_use_their_own_caps() {
        let v = values(&[("skills", "skills", "50"), ("stealing", "stealing", "90")]);
        let ctx = Ctx::new(&v, no_log);
        let skills = extended_skills(&ctx, 1).unwrap();
        let stealing = extended_stealing(&ctx, 1).unwrap();
        for (writes, capped) in [(&skills, true), (&stealing, false)] {
            let w = &writes[0];
            let rel = u32::from_le_bytes(w.new_bytes()[1..5].try_into().unwrap());
            let stub = w.site().ru32().wrapping_add(5).wrapping_add(rel);
            // SAFETY: this builder's jump targets its context stub: resume, flags, registers, handler, argument.
            let arg = unsafe { common::read::<u32>(stub + 13) } as usize;
            let mut record = [0u32; 5];
            record[4] = 70;
            let mut panel = [0u32; 97];
            panel[96] = record.as_mut_ptr() as u32;
            // SAFETY: Context contains only integer register values; the handler uses these local records.
            let mut registers: Context = unsafe { core::mem::zeroed() };
            registers.eax = record.as_mut_ptr() as u32;
            registers.esi = panel.as_mut_ptr() as u32;
            skill_cap(arg, &mut registers);
            let ge = (registers.eflags & 0x80 != 0) == (registers.eflags & 0x800 != 0);
            assert_eq!(ge, capped);
        }
    }

    #[test]
    fn plain_switches_write_the_originals_bytes() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        assert_eq!(
            plain(&unlock_money(&ctx, 1).unwrap()),
            [(
                eix_sites::GUARD_PLAYERS__50,
                vec![0x33, 0xD5],
                vec![0x90, 0x90]
            )]
        );
        assert!(unlock_money(&ctx, 2).unwrap().is_empty());
        assert_eq!(
            plain(&hero_always_can_run(&ctx, 1).unwrap()),
            [(
                eix_sites::UNIT_STATS_MOVEMENT_STATE__70,
                vec![0x74],
                vec![0xEB]
            )]
        );
        assert_eq!(
            sites(&save_all(&ctx, 32).unwrap()),
            [
                (eix_sites::GAME_FLOW_SAVE_GAME__24E, 1),
                (eix_sites::GAME_FLOW_LOAD_GAME__40D, 1)
            ]
        );
        assert!(save_all(&ctx, 0).unwrap().is_empty());
    }

    #[test]
    fn mercenaries_and_the_hidden_draw_range() {
        let v = values(&[("gameplay", "maxcountofmerc", "3")]);
        let ctx = Ctx::new(&v, no_log);
        assert_eq!(
            plain(&extended_merc(&ctx, 1).unwrap()),
            [(
                eix_sites::C_WORLD_SERVER_UPDATE_MERC_BRIEFINGS__3C1,
                vec![2],
                vec![3]
            )]
        );
        // The draw range needs all three conditions.
        let v = values(&[
            ("gameplay", "maxcountofmerc", "60"),
            ("gameplay", "saveall", "32"),
        ]);
        let ctx = Ctx::new(&v, no_log);
        assert_eq!(extended_merc(&ctx, 15).unwrap().len(), 1);
        let w = plain(&extended_merc(&ctx, 16).unwrap());
        assert_eq!(w.len(), 7);
        assert_eq!(
            w[0],
            (
                eix_sites::C_WORLD_SERVER_UPDATE_MERC_BRIEFINGS__3C1,
                vec![2],
                vec![60]
            )
        );
        assert_eq!(
            w[1],
            (
                eix_sites::C_WORLD_CLIENT_DRAW_PASS__10A,
                vec![0xB0, 0xBA, 0x73, 0],
                vec![0xB9, 0xA0, 0x56, 0]
            )
        );
        assert_eq!(
            w[6],
            (
                eix_sites::C_WORLD_CLIENT_RENDER__1ED,
                vec![0, 0, 0xC8, 0x42],
                vec![0, 0, 0x80, 0x45]
            )
        );
        let v = values(&[
            ("gameplay", "maxcountofmerc", "50"),
            ("gameplay", "saveall", "32"),
        ]);
        let ctx = Ctx::new(&v, no_log);
        assert_eq!(extended_merc(&ctx, 16).unwrap().len(), 1);
    }

    #[test]
    fn damage_types_and_lights() {
        let v = values(&[
            ("gameplay", "dmgtypescount", "7"),
            ("gameplay", "light12", "0.8"),
        ]);
        let ctx = Ctx::new(&v, no_log);
        assert_eq!(
            plain(&dmg_types(&ctx, 1).unwrap()),
            [
                (
                    eix_sites::C_HERO_STATS_PANEL_DRAW_PART__14C,
                    vec![6, 0, 0, 0],
                    vec![7, 0, 0, 0]
                ),
                (
                    eix_sites::C_ITEM_INFO_PANEL_DESCRIBE_STATS__C08,
                    vec![6],
                    vec![7]
                )
            ]
        );
        let w = plain(&items_light_changes(&ctx, 1).unwrap());
        let (site, game, new) = &w[0];
        assert_eq!((*site, game.len()), (eix_sites::K_SCHOOL_GLOW, 128));
        // light12 is the first float of the fourth entry; the others keep their built-in 1.
        assert_eq!(new[48..52], 0.8f32.to_le_bytes());
        assert_eq!(new[0..4], 1f32.to_le_bytes());
        assert_eq!(new[4..8], 1f32.to_le_bytes());
    }

    #[test]
    fn shop_ranges_and_factors() {
        let v = values(&[
            ("gameplay", "maxsweapons", "3"),
            ("gameplay", "maxsprototypes", "50"),
            ("gameplay", "minsprototypes", "20"),
            ("gameplay", "jigran", "0.25"),
        ]);
        let ctx = Ctx::new(&v, no_log);
        let w = plain(&shops_new_normal_changes(&ctx, 1).unwrap());
        assert_eq!(w.len(), 17);
        assert_eq!(
            w[0],
            (
                eix_sites::CLASS_TEMPLATE_GENERATE_ITEMS__AE,
                vec![3],
                vec![3]
            )
        );
        // Armours keep the built-in 10.
        assert_eq!(
            w[2],
            (
                eix_sites::CLASS_TEMPLATE_GENERATE_ITEMS__BE,
                vec![3],
                vec![10]
            )
        );
        assert_eq!(
            w[8],
            (
                eix_sites::CLASS_TEMPLATE_GENERATE_MAGIC__7B,
                vec![3],
                vec![50]
            )
        );
        assert_eq!(
            w[9],
            (
                eix_sites::CLASS_TEMPLATE_GENERATE_MAGIC__7D,
                vec![1],
                vec![20]
            )
        );
        assert_eq!(
            w[16],
            (
                eix_sites::DATA_79A356,
                vec![0; 4],
                0.25f32.to_le_bytes().to_vec()
            )
        );
        assert!(shops_new_normal_changes(&ctx, 2).unwrap().is_empty());
        let s = sites(&shops_changes(&ctx, 1).unwrap());
        assert_eq!(
            s,
            [
                (eix_sites::CLASS_TEMPLATES_BUILD__53D, 10),
                (eix_sites::CLASS_TEMPLATES_BUILD__518, 1),
                (eix_sites::CLASS_TEMPLATES_BUILD__41F, 1),
                (eix_sites::CLASS_TEMPLATES_BUILD__448, 7),
                (eix_sites::CLASS_TEMPLATES_BUILD__7B5, 6)
            ]
        );
    }

    #[test]
    fn lift_debuff_formula() {
        // load 50 of 100, liftkey 1: 1.01 - 0.5 / 1.5.
        let d = lift_debuff(30.0, 50.0, 100.0, 1.0);
        assert!((d - 30.0 * (1.01 - 0.5 / 1.5)).abs() < 1e-4, "{d}");
        // liftkey 0 (the shipped INI): 1.01 - 1 = 0.01 of the stat at any load.
        assert!((lift_debuff(30.0, 10.0, 100.0, 0.0) - 0.3).abs() < 1e-4);
    }

    #[test]
    fn speeds_under_load() {
        // r = 1/3, r^3 = 1/27.
        let s = move_speed(1.0, 50.0, 100.0, 1.0, None);
        assert!((s - (1.0 - 1.0 / 27.0)).abs() < 1e-6, "{s}");
        // The remaster: (0.2 * 1 + s) * 1, kept above 0.5, then the overload factor 0.5.
        let r = move_speed(1.0, 50.0, 100.0, 1.0, Some((0.2, 1.0, 1.0, 0.5, 0.5)));
        assert!((r - (0.2 + s) * 0.5).abs() < 1e-6, "{r}");
        let low = move_speed(0.1, 50.0, 100.0, 1.0, Some((0.0, 1.0, 1.0, 0.5, 2.0)));
        assert_eq!(low, 1.0);
        let nan = move_speed(f32::NAN, 50.0, 100.0, 1.0, Some((0.0, 1.0, 1.0, 0.5, 1.0)));
        assert_eq!(nan, 0.5);
    }

    #[test]
    fn stealing_stays_inside_the_originals_dispatcher() {
        // The original rewrites these 50 bytes as its dispatcher; ours changes two places within them.
        const DISPATCHER: &str = concat!(
            "?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ",
            "?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ??"
        );
        assert_eq!(DISPATCHER.split(' ').count(), 50);
        assert_original(&[(
            "gameplay",
            "improvedstealing",
            1,
            &[],
            &[(eix_sites::HOOVER_COLLECT_LOOT__1E, DISPATCHER)],
        )]);
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        assert_eq!(
            sites(&improved_stealing(&ctx, 1).unwrap()),
            [
                (eix_sites::HOOVER_COLLECT_LOOT__1F, 2),
                (eix_sites::HOOVER_COLLECT_LOOT__2D, 5)
            ]
        );
        assert!(improved_stealing(&ctx, 2).unwrap().is_empty());
    }

    /// `newoptions`' table references (ours point at our table) and its names and bound; then actions 60 and 61's
    /// switch entries, which the original leaves as padding and ours ends the switch with.
    const NEW_OPTIONS: &[(Site, &str)] = &[
        (eix_sites::LOAD_KEY_BINDINGS__12B, "?? ?? ?? ??"),
        (eix_sites::LOAD_KEY_BINDINGS__14C, "?? ?? ?? ??"),
        (eix_sites::LOAD_KEY_BINDINGS__168, "?? ?? ?? ??"),
        (eix_sites::SAVE_KEY_BINDINGS__DD, "?? ?? ?? ??"),
        (eix_sites::SAVE_KEY_BINDINGS__ED, "?? ?? ?? ??"),
        (eix_sites::SAVE_KEY_BINDINGS__101, "?? ?? ?? ??"),
        (
            eix_sites::C_OPTIONS_WINDOW_BUILD_BINDINGS__5B,
            "?? ?? ?? ??",
        ),
        (
            eix_sites::C_OPTIONS_WINDOW_BUILD_BINDINGS__27F,
            "?? ?? ?? ??",
        ),
        (eix_sites::C_OPTIONS_WINDOW_SHOW_GROUP__109, "?? ?? ?? ??"),
        (eix_sites::C_OPTIONS_WINDOW_SHOW_GROUP__183, "?? ?? ?? ??"),
        (
            eix_sites::C_SCENE_RENDERER_SET_FOG_COLOUR_STATE_IMPL__33,
            "63 61 6D 70 00 6B 69 63 6B 00 00 00 00",
        ),
        (eix_sites::BATTLE_SCREEN_KEY_DOWN__C4, "3C"),
        (
            eix_sites::BATTLE_SCREEN_KEY_DOWN__1068,
            "?? ?? ?? ?? ?? ?? ?? ??",
        ),
    ];

    /// `camp` 2's controls with `newoptions`, as the original writes them.
    const CAMP_CONTROLS: &[(Site, &str)] = &[
        (
            eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__7F,
            "E9 ?? ?? ?? ??",
        ),
        (eix_sites::C_HUD_ACTION_COLUMN_SET_UNIT__B3, "26"),
        (
            eix_sites::C_HUD_ACTION_COLUMN_SET_UNIT__2CD,
            "E9 ?? ?? ?? ?? 90 90 90 90 90 90 90 90 90 5D 5B 8B 4C 24 2C 5F 5E 64 89 0D 00 00 00 00 83 C4 30 C2 08 00",
        ),
        (eix_sites::ACTION_COLUMN_MOUSE_UP_VERIFY__19, "41"),
        (eix_sites::ACTION_COLUMN_MOUSE_UP_VERIFY__3B, "1F"),
        (
            eix_sites::ACTION_COLUMN_MOUSE_UP_VERIFY__43,
            "17 83 F8 02 75 05 E8 ?? ?? ?? ?? 90 50 8B CF C6 46 08 01 E8 15 FD FF FF 5F 5E C2 0C 00",
        ),
    ];

    #[test]
    fn new_options_write_the_originals_bytes() {
        let sites: Vec<(Site, &str)> = NEW_OPTIONS.iter().chain(CAMP_CONTROLS).copied().collect();
        let both: &'static [(Site, &str)] = sites.leak();
        assert_original(&[
            ("gameplay", "newoptions", 1, &[], NEW_OPTIONS),
            (
                "gameplay",
                "camp",
                2,
                &[("gameplay", "newoptions", "1")],
                both,
            ),
        ]);
    }

    #[test]
    fn the_new_key_table_appends_three_actions() {
        let entry = |i: usize| -> [u32; 4] {
            core::array::from_fn(|j| {
                KEY_TABLE[4 * i + j].load(core::sync::atomic::Ordering::Relaxed)
            })
        };
        assert_eq!(entry(11), [11, eix_sites::DATA_7879B8.ru32(), 6, 0]);
        assert_eq!(
            entry(60),
            [
                60,
                eix_sites::C_SCENE_RENDERER_SET_FOG_COLOUR_STATE_IMPL__33.ru32(),
                5,
                9
            ]
        );
        assert_eq!(
            entry(61),
            [
                61,
                eix_sites::C_SCENE_RENDERER_SET_FOG_COLOUR_STATE_IMPL__38.ru32(),
                5,
                10
            ]
        );
        assert_eq!(
            entry(62),
            [
                62,
                eix_sites::C_SCENE_RENDERER_SET_FOG_COLOUR_STATE_IMPL__38.ru32(),
                5,
                11
            ]
        );
        for (i, row) in KEY_ACTIONS.iter().enumerate() {
            assert_eq!(row[0], i as u32);
        }
    }

    #[test]
    fn camp_key_writes_the_originals_bytes() {
        assert_original(&[(
            "gameplay",
            "camp",
            2,
            &[],
            &[
                (
                    eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__7F,
                    "E9 ?? ?? ?? ??",
                ),
                (eix_sites::BATTLE_SCREEN_KEY_DOWN__E79, "E8 ?? ?? ?? ??"),
            ],
        )]);
    }

    /// `camp` 3's quest-item hooks, as the original writes them.
    const QUEST_HOOKS: &[(Site, &str)] = &[
        (
            eix_sites::C_WORLD_SERVER_CREATE_ITEM_NAMED__7D,
            "E8 ?? ?? ?? ?? 90",
        ),
        (eix_sites::C_PLAYER_TAKE_LOOT__39, "E8 ?? ?? ?? ?? 53"),
    ];

    #[test]
    fn camp_three_writes_the_originals_bytes() {
        let key: Vec<(Site, &str)> = [
            (
                eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__7F,
                "E9 ?? ?? ?? ??",
            ),
            (eix_sites::BATTLE_SCREEN_KEY_DOWN__E79, "E8 ?? ?? ?? ??"),
        ]
        .into_iter()
        .chain(QUEST_HOOKS.iter().copied())
        .collect();
        let controls: Vec<(Site, &str)> = NEW_OPTIONS
            .iter()
            .chain(CAMP_CONTROLS)
            .chain(QUEST_HOOKS)
            .copied()
            .collect();
        assert_original(&[
            ("gameplay", "camp", 3, &[], key.leak()),
            (
                "gameplay",
                "camp",
                3,
                &[("gameplay", "newoptions", "1")],
                controls.leak(),
            ),
        ]);
    }

    #[test]
    fn camp_thirteen_writes_the_originals_bytes() {
        let key = [
            (
                eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__7F,
                "E9 ?? ?? ?? ??",
            ),
            (eix_sites::BATTLE_SCREEN_KEY_DOWN__E79, "E8 ?? ?? ?? ??"),
            (eix_sites::C_BATTLE_SCREEN_CREATE__7FD, "E9 ?? ?? ?? ?? 90"),
        ];
        let controls: Vec<(Site, &str)> = key
            .iter()
            .chain(NEW_OPTIONS)
            .chain(
                CAMP_CONTROLS
                    .iter()
                    .filter(|s| s.0 != eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__7F),
            )
            .copied()
            .collect();
        assert_original(&[
            ("gameplay", "camp", 13, &[], key.to_vec().leak()),
            ("gameplay", "camp", 20, &[], key.to_vec().leak()),
            (
                "gameplay",
                "camp",
                13,
                &[("gameplay", "newoptions", "1")],
                controls.leak(),
            ),
        ]);
    }

    #[test]
    fn camp_four_writes_the_originals_bytes() {
        assert_original(&[(
            "gameplay",
            "camp",
            4,
            &[],
            &[
                (eix_sites::CLAMP_UNIT__3B, "51 75 49 73 00"),
                (
                    eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__7F,
                    "E9 ?? ?? ?? ??",
                ),
                (eix_sites::BATTLE_SCREEN_KEY_DOWN__E79, "E8 ?? ?? ?? ??"),
                (
                    eix_sites::C_HUD_QUICK_ITEMS_SET_UNIT__415,
                    "90 E8 ?? ?? ?? ??",
                ),
            ],
        )]);
    }

    #[test]
    fn hooks_take_the_originals_sites() {
        let v = values(&[]);
        let ctx = Ctx::new(&v, no_log);
        let w = plain(&avg_armors(&ctx, 1).unwrap());
        assert_eq!(
            (w[0].0, w[0].1.len()),
            (eix_sites::C_HERO_STATS_PANEL_DRAW_PART__150, 51)
        );
        // The original's rewrite, but the call's target.
        assert_eq!(w[0].2[..43], AVG_ARMORS_NOW[..43]);
        assert_eq!(w[0].2[47..], AVG_ARMORS_NOW[47..]);
        assert!(avg_armors(&ctx, 2).unwrap().is_empty());
        assert_eq!(
            sites(&camp(&ctx, 1).unwrap()),
            [(eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__7F, 5)]
        );
        for value in [2, 5, 12] {
            assert_eq!(
                sites(&camp(&ctx, value).unwrap()),
                [
                    (eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__7F, 5),
                    (eix_sites::BATTLE_SCREEN_KEY_DOWN__E79, 5)
                ]
            );
        }
        // `newoptions` puts the camp's own action and button in the Home key's place.
        let with_options = values(&[("gameplay", "newoptions", "1")]);
        let options = Ctx::new(&with_options, no_log);
        let controls = sites(&camp(&options, 2).unwrap());
        assert!(
            !controls
                .iter()
                .any(|&(at, _)| at == eix_sites::BATTLE_SCREEN_KEY_DOWN__E79)
        );
        assert!(controls.contains(&(eix_sites::BATTLE_SCREEN_KEY_DOWN__1068, 4)));
        assert_eq!(
            sites(&camp(&ctx, 13).unwrap()),
            [
                (eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__7F, 5),
                (eix_sites::BATTLE_SCREEN_KEY_DOWN__E79, 5),
                (eix_sites::C_BATTLE_SCREEN_CREATE__7FD, 6)
            ]
        );
        // `newoptions` = 2 leaves 2 to 12 with neither the Home key nor the controls.
        let options_two = values(&[("gameplay", "newoptions", "2")]);
        let two = Ctx::new(&options_two, no_log);
        assert_eq!(
            sites(&camp(&two, 2).unwrap()),
            [(eix_sites::C_INTERFACE_3D_ON_STRING_EVENT__7F, 5)]
        );
        assert!(camp(&ctx, 0).unwrap().is_empty());
        let s = sites(&extended_skills(&ctx, 1).unwrap());
        assert_eq!(
            s,
            [
                (eix_sites::TRADE_DRAW_LISTS__21A, 5),
                (eix_sites::TEXT_626A1E, 2),
                (eix_sites::TEXT_626A20, 5),
                (eix_sites::C_TRADE_PANEL_UPDATE_HELD__63, 1),
                (eix_sites::C_TRADE_PANEL_UPDATE_HELD__64, 5)
            ]
        );
        let s = sites(&extended_stealing(&ctx, 1).unwrap());
        assert_eq!(
            s,
            [
                (eix_sites::TRADE_DRAW_LISTS__3B5, 10),
                (eix_sites::TEXT_626AAD, 2),
                (eix_sites::TEXT_626AAF, 5),
                (eix_sites::C_TRADE_PANEL_UPDATE_HELD__C2, 1),
                (eix_sites::C_TRADE_PANEL_UPDATE_HELD__C3, 5)
            ]
        );
        assert_eq!(
            sites(&copy_stats_cut(&ctx, 1).unwrap()),
            [(eix_sites::SCRIPT_RUN_EXECUTE__236B, 5)]
        );
        assert_eq!(
            sites(&lift_changes(&ctx, 1).unwrap()),
            [
                (eix_sites::UNIT_STATS_RECOMPUTE__658, 1),
                (eix_sites::UNIT_STATS_RECOMPUTE__659, 5)
            ]
        );
        assert_eq!(
            sites(&lift_debuffs(&ctx, 1).unwrap()),
            [
                (eix_sites::UNIT_STATS_PACK__146, 6),
                (eix_sites::DATA_799FFC, 4)
            ]
        );
        assert_eq!(
            sites(&encumbrance_move_speed(&ctx, 1).unwrap()),
            [
                (eix_sites::UNIT_STATS_RECOMPUTE__67A, 6),
                (MOVE_MUL, 4),
                (eix_sites::GAME_BUFFER_READ_BYTES__64, 8)
            ]
        );
        assert_eq!(
            sites(&encumbrance_attack_speed(&ctx, 1).unwrap()),
            [
                (eix_sites::C_UNIT_CLIENT_SPECIFIC_ADVANCE_TIME__143, 2),
                (eix_sites::C_UNIT_ACTION_SPEED__13, 6),
                (ATTACK_MUL, 4)
            ]
        );
        assert_eq!(
            sites(&encumbrance_attack_speed(&ctx, 2).unwrap()),
            [(ATTACK_MUL, 4), (eix_sites::UNIT_STATS_RECOMPUTE__69E, 6)]
        );
        assert!(encumbrance_attack_speed(&ctx, 3).unwrap().is_empty());
        let w = heroes_can_hide_weapons(&ctx, 1).unwrap();
        assert_eq!(
            sites(&w),
            [
                (eix_sites::WEAPON_BAR_MOUSE_UP_VERIFY__62, 1),
                (eix_sites::WEAPON_BAR_MOUSE_UP_VERIFY__BB, 5),
                (eix_sites::BATTLE_SCREEN_KEY_DOWN__70F, 4),
                (eix_sites::BATTLE_SCREEN_KEY_DOWN__789, 4),
                (eix_sites::BATTLE_SCREEN_KEY_DOWN__7F9, 4),
                (eix_sites::BATTLE_SCREEN_KEY_DOWN__869, 4)
            ]
        );
        // The four keys share one entry.
        let target = |w: &SiteWrite| {
            w.site().offset(4).ru32() + u32::from_le_bytes(w.new_bytes().try_into().unwrap())
        };
        assert_eq!(target(&w[2]), target(&w[5]));
        let v = values(&[("gameplay", "speedslowremaster", "2")]);
        let ctx = Ctx::new(&v, no_log);
        assert_eq!(
            sites(&encumbrance_move_speed(&ctx, 1).unwrap()),
            [
                (eix_sites::UNIT_STATS_RECOMPUTE__67A, 6),
                (SLOW_MOVE_MUL, 4),
                (MOVE_MUL, 4),
                (eix_sites::GAME_BUFFER_READ_BYTES__64, 8)
            ]
        );
    }
}
