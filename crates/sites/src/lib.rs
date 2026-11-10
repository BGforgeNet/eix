#![warn(missing_docs, unreachable_pub, clippy::missing_panics_doc)]
#![no_std]
#![forbid(unsafe_code)]
//! The game.exe addresses the extender's code uses, one named constant each; code names an address only through this
//! crate. Addresses encoded inside instruction bytes (the operand of an expected `fld dword [0x73F13C]`) stay in those
//! bytes.
//!
//! The values are those of RU 1.06 (`ru106.rs`), and every address in the code, in a constant or inside expected
//! bytes, is an RU 1.06 address. Another build is a table of offsets from RU 1.06 (`en106.rs`): [`select`] picks the
//! build once at start-up, and [`here`] gives an RU address's place in the running build. A name in
//! `.text` is the nearest function at or below the address, then `__` and the offset into it in hex
//! (`C_UNIT_SERVER_ON_DAMAGED__47`): the function's name as the code's comments cite it, else as public research
//! (ei-rebuild and others) names it. Function sizes are unknown, so padding after a function takes its name too. A
//! global or vtable takes its symbol's name at its own address only. An address without such a name is its section
//! and value (`TEXT_4CF106`, `RDATA_73FF16`, `DATA_7AF760`); two addresses that would share a name both add `_AT_` and
//! their value. A 32-bit game word takes `NAME as u32`, which loses nothing: game.exe is a 32-bit image.

use core::sync::atomic::{AtomicU8, Ordering};

mod en106;
mod ru106;

pub use ru106::*;

/// The game.exe builds the tables cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    /// Russian 1.06: the starter's `Engine\game.exe` and GOG's Russian exe.
    Ru106,
    /// English and German 1.06: GOG's English and German exe (one file), and the official EN and DE 1.06 patches.
    En106,
}

static BUILD: AtomicU8 = AtomicU8::new(0);

/// Makes [`here`] and [`ru`] translate for `build`; called once, before the first game address is used.
pub fn select(build: Build) {
    BUILD.store(build as u8, Ordering::Relaxed);
}

/// The build [`select`] chose; RU 1.06 before any choice.
pub fn selected() -> Build {
    match BUILD.load(Ordering::Relaxed) {
        1 => Build::En106,
        _ => Build::Ru106,
    }
}

/// Whether addresses need translating: a build other than RU 1.06 runs.
pub fn translating() -> bool {
    selected() != Build::Ru106
}

/// Whether `address` lies in game.exe's image, code or data, in any build (the sections start at one address).
pub fn in_image(address: usize) -> bool {
    (TEXT_SECTION..RSRC_SECTION).contains(&address)
}

/// The RU 1.06 address `ru` in the running build; an address outside the image is returned as it is.
pub fn here(ru: usize) -> usize {
    match selected() {
        Build::Ru106 => ru,
        Build::En106 => shift(en106::RUNS, ru, false),
    }
}

/// The RU 1.06 address of `address` in the running build: the inverse of [`here`].
pub fn ru(address: usize) -> usize {
    match selected() {
        Build::Ru106 => address,
        Build::En106 => shift(en106::RUNS, address, true),
    }
}

fn shift(runs: &[(usize, isize)], address: usize, inverse: bool) -> usize {
    if !in_image(address) {
        return address;
    }
    // The row whose start (in the space `address` is in) is the last at or below it.
    let start = |&(s, d): &(usize, isize)| if inverse { s.wrapping_add_signed(d) } else { s };
    let row = runs.partition_point(|r| start(r) <= address);
    match row.checked_sub(1).and_then(|i| runs.get(i)) {
        Some(&(_, d)) if inverse => address.wrapping_add_signed(-d),
        Some(&(_, d)) => address.wrapping_add_signed(d),
        None => address,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn en106_maps_both_ways() {
        let runs = en106::RUNS;
        for &(start, _) in runs {
            for a in [start, start + 1, start + 7] {
                assert_eq!(shift(runs, shift(runs, a, false), true), a, "{a:#x}");
            }
        }
        // Low code is unchanged, high code moves by 0x40, data by its run.
        assert_eq!(shift(runs, ENTRY, false), 0x6E_C1E6);
        assert_eq!(shift(runs, 0x45_6440, false), 0x45_6440);
        assert_eq!(shift(runs, 0x7C_D000, false), 0x7C_D000);
        assert_eq!(shift(runs, 0x10_0000, false), 0x10_0000);
    }
}
