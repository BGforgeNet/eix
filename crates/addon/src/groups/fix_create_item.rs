//! FixCreateItem: `CItemManager::CreateItem` 0x6AC970 reports an unknown prototype, or an invalid prototype or
//! material, and returns no item, where the game called `FatalError`.
#![expect(unsafe_code, reason = "reading the item name the game passes")]

use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use super::arg_path::c_str;
use super::{BuildError, Labelled, Setup};
use crate::report;

/// The failure exit after the first `FatalError` call, which destroys the locals and returns null.
const FAILURE_EXIT: Site = eix_sites::C_ITEM_MANAGER_CREATE_ITEM__261;
/// Stack word 0x18 (ESP + 0x60) at both sites: the item's name, which the second site's `FatalError` prints.
const NAME_WORD: usize = 0x60 / 4;

/// The two `FatalError` calls' first instructions and what each reports.
const SITES: [(&str, Site, &[u8], &str); 2] = [
    (
        "FixCreateItem #0",
        eix_sites::C_ITEM_MANAGER_CREATE_ITEM__24D,
        &[0x8B, 0x54, 0x24, 0x40, 0x8B, 0x02],
        "can't find prototype",
    ),
    (
        "FixCreateItem #1",
        eix_sites::C_ITEM_MANAGER_CREATE_ITEM__360,
        &[0x8B, 0x4C, 0x24, 0x60, 0x51],
        "invalid prototype or material",
    ),
];

extern "C" fn handler(reason: usize, ctx: &mut Context) {
    let reason = SITES.get(reason).map_or("", |s| s.3);
    // SAFETY: at both sites the word is the name the function was given, a NUL-terminated string.
    let name = ctx.stack_word(NAME_WORD).and_then(|w| unsafe { c_str(w) });
    let name = name.map_or(&b""[..], |n| n.to_bytes());
    report::error(&[b"GiveItem: ", reason.as_bytes(), b" (\"", name, b"\")"].concat());
    ctx.resume = FAILURE_EXIT;
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    report::init(&setup.options);
    SITES
        .iter()
        .enumerate()
        .map(|(i, (label, site, original, _))| {
            Ok(Labelled {
                label: (*label).to_owned(),
                write: hook::context_hook(*site, original, Mode::Drop, handler, i)?,
            })
        })
        .collect()
}
