//! game.exe EN/DE 1.06 (GOG's English and German exe, and the official EN and DE 1.06 patches' exe) as offsets from
//! RU 1.06: an RU address from `start` up to the next row's `start` lies at that address plus `delta`.
//!
//! Derived by aligning the two exes' instruction streams: each code row is a run of aligned instructions, each data
//! row a run of aligned instructions' address operands, all of one delta.

/// `(RU start, delta)`, ascending; the last row reaches the end of the image.
pub(crate) const RUNS: &[(usize, isize)] = &[
    (0x40_1000, 0),
    (0x60_519D, -4),
    (0x60_5217, -58),
    (0x60_5DE0, -64),
    (0x73_B000, 0),
    (0x79_3380, -8),
    (0x79_3484, -12),
    (0x79_69D0, -16),
    (0x7C_AF20, -32),
];

/// RU ranges `[start, end)` this build has no counterpart of: RU's briefing picks one of two speech folders through a
/// table of their names, where this build's briefing has one folder and no table.
pub(crate) const MISSING: &[(usize, usize)] = &[(0x79_3378, 0x79_3380)];
