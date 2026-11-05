#![warn(missing_docs, unreachable_pub, clippy::missing_panics_doc)]
#![cfg_attr(not(test), no_std)]
//! The only crate that touches game.exe: memory, hooks, engine structures, Win32.
//!
//! Builds only for 32-bit x86 Windows: its calling conventions and trampolines exist nowhere else. It uses `core` and
//! `alloc` only, so the loader can be built without std; a DLL without std installs [`heap::ProcessHeap`] as its
//! global allocator.

#[cfg(not(all(windows, target_arch = "x86")))]
compile_error!("eix-engine targets i686-pc-windows-gnu only");

extern crate alloc;

pub mod ansi;
pub mod crash;
pub mod ddraw;
pub mod dll;
pub mod env;
pub mod exe;
pub mod exec;
pub mod float;
pub mod game;
pub mod gdi;
pub mod group;
pub mod heap;
pub mod hook;
pub mod imports;
mod lock;
pub mod owners;
pub mod patch;
pub mod registry;
pub mod sse;
pub mod win;
pub mod x86;
pub mod x87;

#[cfg(test)]
mod tests {
    // Every address table, struct layout and trampoline assumes game.exe's 32-bit process.
    #[test]
    fn pointers_are_32_bit() {
        assert_eq!(size_of::<usize>(), 4);
    }
}
