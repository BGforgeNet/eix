#![warn(missing_docs, unreachable_pub, clippy::missing_panics_doc)]
#![no_std]
#![forbid(unsafe_code)]
//! The game.exe addresses the extender's code uses, one named constant each; code names an address only through this
//! crate. Addresses encoded inside instruction bytes (the operand of an expected `fld dword [0x73F13C]`) stay in those
//! bytes.
//!
//! The values are those of RU 1.06 (`ru106.rs`), and every address in the code, in a [`Site`] or inside expected
//! bytes, is an RU 1.06 address. Another build is a table of offsets from RU 1.06 (`en106.rs`): [`select`] picks the
//! build once at start-up, and [`Site::here`] gives a site's place in the running build. A site is no integer, so
//! code cannot read, call or compare it as an address without saying in which build. A name in
//! `.text` is the nearest function at or below the address, then `__` and the offset into it in hex
//! (`C_UNIT_SERVER_ON_DAMAGED__47`): the function's name as the code's comments cite it, else as public research
//! (ei-rebuild and others) names it. Function sizes are unknown, so padding after a function takes its name too. A
//! global or vtable takes its symbol's name at its own address only. An address without such a name is its section
//! and value (`TEXT_4CF106`, `RDATA_73FF16`, `DATA_7AF760`); two addresses that would share a name both add `_AT_` and
//! their value.

use core::fmt;
use core::sync::atomic::{AtomicU8, Ordering};

mod en106;
mod ru106;

pub use ru106::*;

/// A place in game.exe, named by its RU 1.06 address. Bytes for the engine to compare and write hold the RU address
/// ([`ru32`](Self::ru32)), which the engine translates with the bytes; a read, a call, a register or a comparison with
/// a live value takes the running build's ([`here`](Self::here), [`of`](Self::of)).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Site(usize);

impl Site {
    /// The site at RU 1.06 address `ru`.
    pub const fn new(ru: usize) -> Self {
        Self(ru)
    }

    /// The site `n` bytes on: an instruction inside a function, a field inside a global.
    pub const fn offset(self, n: usize) -> Self {
        Self(self.0 + n)
    }

    /// The site `n` bytes back.
    pub const fn back(self, n: usize) -> Self {
        Self(self.0 - n)
    }

    /// How many bytes `self` lies past `from`.
    pub const fn since(self, from: Self) -> usize {
        self.0 - from.0
    }

    /// The RU 1.06 address. Only for bytes the engine translates, tables keyed by site and messages: in another build
    /// nothing lies there.
    pub const fn ru(self) -> usize {
        self.0
    }

    /// [`ru`](Self::ru) as a 32-bit game word, which loses nothing: game.exe is a 32-bit image.
    pub const fn ru32(self) -> u32 {
        self.0 as u32
    }

    /// The address in the running build.
    pub fn here(self) -> usize {
        here(self.0)
    }

    /// [`here`](Self::here) as a 32-bit game word, for a register or a game field.
    pub fn here32(self) -> u32 {
        here(self.0) as u32
    }

    /// The address in `build`, whichever build runs: for checking bytes meant for another build.
    pub fn in_build(self, build: Build) -> usize {
        in_build(build, self.0)
    }

    /// The running build's `address` as a pointer to `T`.
    pub fn ptr<T>(self) -> *mut T {
        core::ptr::with_exposed_provenance_mut(self.here())
    }

    /// The site at `address` of the running build (a return address, a live pointer); an address outside game.exe's
    /// image stays as it is.
    pub fn of(address: usize) -> Self {
        Self(ru(address))
    }
}

impl fmt::Debug for Site {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

impl fmt::LowerHex for Site {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.0, f)
    }
}

impl fmt::UpperHex for Site {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperHex::fmt(&self.0, f)
    }
}

/// The game.exe builds the tables cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    /// Russian 1.06: the starter's `Engine\game.exe` and GOG's Russian exe.
    Ru106,
    /// English and German 1.06: GOG's English and German exe (one file).
    En106,
}

static BUILD: AtomicU8 = AtomicU8::new(0);

/// Makes [`Site::here`] and [`Site::of`] translate for `build`; called once, before the first game address is used.
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
    (TEXT_SECTION.0..RSRC_SECTION.0).contains(&address)
}

/// The RU 1.06 address `ru` in the running build; an address outside the image is returned as it is.
fn here(ru: usize) -> usize {
    in_build(selected(), ru)
}

fn in_build(build: Build, ru: usize) -> usize {
    match build {
        Build::Ru106 => ru,
        Build::En106 => shift(en106::RUNS, ru, false),
    }
}

/// The RU 1.06 address of `address` in the running build: the inverse of [`here`].
fn ru(address: usize) -> usize {
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
        assert_eq!(shift(runs, ENTRY.0, false), 0x6E_C1E6);
        assert_eq!(shift(runs, 0x45_6440, false), 0x45_6440);
        assert_eq!(shift(runs, 0x7C_D000, false), 0x7C_D000);
        assert_eq!(shift(runs, 0x10_0000, false), 0x10_0000);
    }
}
