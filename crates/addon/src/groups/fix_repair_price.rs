//! FixRepairPrice: `CCampPages::Price` 0x60FAB0 holds an item's wear to 0..=1 (core's [`repair_wear`]), and for deal
//! kind 7 (taking an item apart in the item constructor) adds the repair cost of the wear instead of subtracting it.
#![expect(unsafe_code, reason = "reading the item the game prices")]

use std::sync::atomic::{AtomicUsize, Ordering};

use eix_core::fixes::repair_wear;
use eix_engine::game::item::Item;
use eix_engine::hook::{self, Context, Mode};

use super::{BuildError, Labelled, Setup};

/// `fld dword ptr [esi + 0x20]; fdiv dword ptr [esi + 0x24]`, the start of the wear `1 - condition / maximum` that
/// 0x60FAEC stores at ESP + 0x14; ESI is the item.
const WEAR: usize = eix_sites::C_CAMP_PAGES_PRICE__2C;
const WEAR_ORIGINAL: [u8; 6] = [0xD9, 0x46, 0x20, 0xD8, 0x76, 0x24];
/// After the wear: `call [eax + 0x40]` (the item's price) with EAX the vtable and ECX the item.
const WEAR_RESUME: usize = eix_sites::C_CAMP_PAGES_PRICE__40;
/// Stack word 5, ESP + 0x14: the wear.
const WEAR_WORD: usize = 5;

/// `fmul dword ptr [edi + ebx*4 + 0x21C]` (the price times the deal's rate), then `fsub dword ptr [esp + 0xC]` at
/// 0x60FB8E takes the repair cost off; EBX is the deal kind. The hook covers the multiply's first five bytes, as the
/// original's jump does.
const COST: usize = eix_sites::C_CAMP_PAGES_PRICE__D7;
const COST_MULTIPLY: [u8; 7] = [0xD8, 0x8C, 0x9F, 0x1C, 0x02, 0x00, 0x00];
/// Stack word 3, ESP + 0xC: the repair cost.
const COST_WORD: usize = 3;
/// The deal that takes an item apart.
const TAKE_APART: u32 = 7;

/// The copy of the multiply, which goes on at the subtraction 0x60FB8E.
static MULTIPLY: AtomicUsize = AtomicUsize::new(0);

extern "C" fn wear(_: usize, ctx: &mut Context) {
    let item = core::ptr::with_exposed_provenance::<Item>(ctx.esi as usize);
    // SAFETY: at 0x60FADC ESI is the live item the camp prices, on the game's thread.
    let item = unsafe { &*item };
    ctx.set_stack_word(
        WEAR_WORD,
        repair_wear(item.condition, item.max_condition).to_bits(),
    );
    // An item's first word is its vtable.
    ctx.eax = item.base.vtable.expose_provenance() as u32;
    ctx.ecx = ctx.esi;
    ctx.resume = WEAR_RESUME;
}

extern "C" fn cost(_: usize, ctx: &mut Context) {
    // Subtracting the negated cost is the addition, and the cost is not read again; this keeps the FPU stack, which
    // holds the price, out of the handler.
    if ctx.ebx == TAKE_APART
        && let Some(cost) = ctx.stack_word(COST_WORD)
    {
        ctx.set_stack_word(COST_WORD, cost ^ 0x8000_0000);
    }
    ctx.resume = MULTIPLY.load(Ordering::SeqCst);
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    MULTIPLY.store(hook::trampoline(COST, &COST_MULTIPLY)?, Ordering::SeqCst);
    Ok(vec![
        Labelled {
            label: "FixRepairPrice #0".to_owned(),
            write: hook::context_hook(WEAR, &WEAR_ORIGINAL, Mode::Drop, wear, 0)?,
        },
        Labelled {
            label: "FixRepairPrice #1".to_owned(),
            write: hook::context_hook(COST, &COST_MULTIPLY[..5], Mode::Drop, cost, 0)?,
        },
    ])
}
