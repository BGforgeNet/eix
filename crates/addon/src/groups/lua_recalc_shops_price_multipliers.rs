//! LuaRecalcShopsPriceMultipliers: when the class templates are built, the shops' nine
//! price multipliers pass through the script's `RecalcShopsPriceMultipliers`.
#![expect(
    unsafe_code,
    reason = "reading and writing the shops' records the game built"
)]

use eix_core::lua_callbacks::Callback::RecalcShopsPriceMultipliers;
use eix_engine::game::database::VectorBounds;
use eix_engine::hook::{Context, Mode};

use super::lua_common::{context, require_host, wanted, with_host};
use super::{BuildError, Labelled, Setup};
use crate::lua::{addr, ptr};

/// The multipliers of one shop record: nine floats from +0x10 (spells' buy, sell, assemble, disassemble, the same
/// for items, then repair).
const FIRST: u32 = 0x10;
const COUNT: usize = 9;

/// The shop record pointers of the vector at `vector`.
///
/// # Safety
/// `vector` must be the game's live vector of shop record pointers.
unsafe fn records(vector: u32) -> Vec<u32> {
    // SAFETY: the caller guarantees the vector.
    let v = unsafe { ptr::<VectorBounds>(vector).read() };
    let count = (v.end.wrapping_sub(v.begin) >> 2) as usize;
    // SAFETY: the vector holds `count` pointers from `begin`.
    (0..count)
        // SAFETY: entry `i` of the vector.
        .map(|i| unsafe { ptr::<u32>(v.begin).wrapping_add(i).read() })
        .collect()
}

/// At 0x4A7E7E, after the epilogue's first two instructions ran, EBX is the vector of shop record pointers.
extern "C" fn handler(_: usize, ctx: &mut Context) {
    // SAFETY: at this site EBX is the class templates' shop vector, on the game's thread.
    let shops = unsafe { records(ctx.ebx) };
    let is_lmp = shops.len() == 4;
    let mut all = Vec::with_capacity(shops.len() * COUNT);
    for &shop in &shops {
        for i in 0..COUNT as u32 {
            // SAFETY: each shop record holds the nine floats from +0x10.
            all.push(unsafe { ptr::<f32>(shop.wrapping_add(FIRST + 4 * i)).read() });
        }
    }
    // The original passes its vector's data pointer, null when there are no shops. The script writes through it.
    let array = if all.is_empty() {
        0
    } else {
        addr(all.as_mut_ptr())
    };
    with_host((), |h| h.recalc_shops_price_multipliers(array, is_lmp));
    for (j, &shop) in shops.iter().enumerate() {
        for i in 0..COUNT {
            // SAFETY: as above.
            unsafe {
                ptr::<f32>(shop.wrapping_add(FIRST + 4 * i as u32)).write(all[j * COUNT + i])
            };
        }
    }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    require_host()?;
    if !wanted(&[RecalcShopsPriceMultipliers]) {
        return Ok(Vec::new());
    }
    Ok(vec![context(
        "LuaRecalcShopsPriceMultipliers #0",
        eix_sites::CLASS_TEMPLATES_BUILD__84E,
        &[0x8B, 0x4C, 0x24, 0x18, 0x5F],
        Mode::Before,
        handler,
    )?])
}
