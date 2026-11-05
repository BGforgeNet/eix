//! Who made which code: the game.exe sites written and the stubs placed, each with the patch group, engine fix or
//! SpellAddon feature it belongs to, so that a crash record can name the owner of an address.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::lock::SpinLock;

/// What a recorded range is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Bytes written over game.exe.
    Site,
    /// Code placed in executable memory of ours: a hook's stub or a trampoline.
    Stub,
}

#[derive(Debug)]
struct Range {
    start: usize,
    len: usize,
    kind: Kind,
    owner: String,
}

/// Every recorded range, oldest first; a later write over the same bytes is the one that holds them now.
static RANGES: SpinLock<Vec<Range>> = SpinLock::new(Vec::new());

/// The owner whose writes are being prepared, for the stubs they place. One for the process: the groups, fixes and
/// SpellAddon features are built one at a time on the game's main thread, and no other thread places stubs then.
static BUILDING: SpinLock<Option<String>> = SpinLock::new(None);

/// Runs `build` with `owner` named as the owner of every stub it places.
pub fn building<T>(owner: &str, build: impl FnOnce() -> T) -> T {
    let before = BUILDING.with(|b| b.replace(owner.to_string()));
    let result = build();
    BUILDING.with(|b| *b = before);
    result
}

/// Records `len` bytes at `start` as written over game.exe for `owner`.
pub fn site(start: usize, len: usize, owner: &str) {
    record(start, len, Kind::Site, owner.to_string());
}

/// Records a stub placed at `start`, for the owner being built (see [`building`]).
pub(crate) fn stub(start: usize, len: usize) {
    let owner = BUILDING
        .with(|b| b.clone())
        .unwrap_or_else(|| "eix start-up".to_string());
    record(start, len, Kind::Stub, owner);
}

fn record(start: usize, len: usize, kind: Kind, owner: String) {
    RANGES.with(|r| {
        r.push(Range {
            start,
            len,
            kind,
            owner,
        });
    });
}

/// The owner of `address`, as the crash record names it: `WideScreen's site 0x4A7DE5+3` or `a stub of WideScreen,
/// +0x12`. None for an address nothing recorded holds, and while another caller holds the record (a crash inside it).
pub fn of(address: usize) -> Option<String> {
    RANGES.try_with(|ranges| {
        let r = ranges
            .iter()
            .rev()
            .find(|r| (r.start..r.start + r.len).contains(&address))?;
        let offset = address - r.start;
        Some(match r.kind {
            Kind::Site => format!("{}'s site {:#X}+{offset}", r.owner, r.start),
            Kind::Stub => format!("a stub of {}, +{offset:#X}", r.owner),
        })
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latest_range_over_an_address_names_its_owner() {
        // Above 0xC0000000, where a 32-bit process maps nothing, so other tests' real sites and stubs cannot hold
        // these. The stub goes through `record`: the owner being built is global, and other tests build in parallel.
        site(0xF000_0000, 8, "Early");
        site(0xF000_0004, 2, "Later");
        record(0xF010_0000, 16, Kind::Stub, "Hooking".to_string());
        assert_eq!(
            of(0xF000_0001).as_deref(),
            Some("Early's site 0xF0000000+1")
        );
        assert_eq!(
            of(0xF000_0005).as_deref(),
            Some("Later's site 0xF0000004+1")
        );
        assert_eq!(of(0xF010_000F).as_deref(), Some("a stub of Hooking, +0xF"));
        assert_eq!(of(0xF010_0010), None);
    }
}
