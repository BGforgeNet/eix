#![warn(missing_docs, unreachable_pub, clippy::missing_panics_doc)]
#![no_std]
#![forbid(unsafe_code)]
//! The game.exe addresses the extender's code uses, one named constant each; code names an address only through this
//! crate. Addresses encoded inside instruction bytes (the operand of an expected `fld dword [0x73F13C]`) stay in those
//! bytes.
//!
//! The values are those of RU 1.06 (`ru106.rs`); another build gets its own file with the same names. A name in
//! `.text` is the nearest function at or below the address, then `__` and the offset into it in hex
//! (`C_UNIT_SERVER_ON_DAMAGED__47`): the function's name as the code's comments cite it, else as public research
//! (ei-rebuild and others) names it. Function sizes are unknown, so padding after a function takes its name too. A
//! global or vtable takes its symbol's name at its own address only. An address without such a name is its section
//! and value (`TEXT_4CF106`, `RDATA_73FF16`, `DATA_7AF760`); two addresses that would share a name both add `_AT_` and
//! their value. A 32-bit game word takes `NAME as u32`, which loses nothing: game.exe is a 32-bit image.

mod ru106;

pub use ru106::*;
