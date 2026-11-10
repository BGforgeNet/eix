//! Typed access to game.exe RU 1.06: its structures, functions and globals.
//!
//! Every address and offset here belongs to [`crate::exe::Build::Ru106`]; a caller checks the build before it touches
//! any of them. Each structure is `#[repr(C)]` with its used offsets pinned by `offset_of!` asserts, and each field,
//! function and global names in its doc comment the game.exe code that shows its meaning. What is covered and what was
//! left out: `README.md` beside this file.
//!
//! The game is single-threaded: everything here is for its thread (a hook handler, a patch-group callback), or for a
//! moment when that thread is known to be stopped.
#![expect(
    unsafe_code,
    reason = "typed views of game.exe's memory and calls into its code"
)]

use core::fmt;
use core::marker::PhantomData;

pub mod ai;
pub mod camera;
pub mod combat;
pub mod console;
pub mod crt;
pub mod database;
pub mod flow;
pub mod item;
pub mod media;
pub mod mfc;
pub mod mob;
pub mod net;
pub mod object;
pub mod player;
pub mod res;
pub mod script;
pub mod spell;
pub mod start;
pub mod stl;
pub mod stream;
pub mod surface;
pub mod text;
pub mod ui;
pub mod unit;
pub mod world;

/// A global variable or static object of game.exe at a fixed address.
pub struct Global<T> {
    address: usize,
    _type: PhantomData<fn() -> T>,
}

impl<T> Global<T> {
    /// The global at `address`.
    pub(crate) const fn at(address: usize) -> Self {
        Self {
            address,
            _type: PhantomData,
        }
    }

    /// Its address in game.exe.
    pub const fn address(&self) -> usize {
        self.address
    }

    /// A pointer to it in the running build; making the pointer reads nothing.
    pub fn as_ptr(&self) -> *mut T {
        core::ptr::with_exposed_provenance_mut(eix_sites::here(self.address))
    }

    /// Reads its value.
    ///
    /// # Safety
    /// The process must be game.exe RU 1.06, on the game's thread, and the global initialised.
    pub unsafe fn read(&self) -> T
    where
        T: Copy,
    {
        // SAFETY: the caller guarantees the build, so the address holds a `T`, and the thread.
        unsafe { self.as_ptr().read() }
    }

    /// Writes `value` into it.
    ///
    /// # Safety
    /// As for [`read`](Self::read); the value must also be one the game handles at this point.
    pub unsafe fn write(&self, value: T)
    where
        T: Copy,
    {
        // SAFETY: as for `read`; the caller vouches for the value.
        unsafe { self.as_ptr().write(value) }
    }
}

impl<T> fmt::Debug for Global<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Global<{}>({:#x})",
            core::any::type_name::<T>(),
            self.address
        )
    }
}

impl<T> Clone for Global<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Global<T> {}

/// `address` in game.exe as a function pointer of type `F`.
///
/// # Safety
/// As [`crate::hook::as_fn`]: the code at `address` must have exactly type `F`.
pub(crate) unsafe fn code<F: Copy>(address: usize) -> F {
    // SAFETY: passed on to the caller.
    unsafe { crate::hook::as_fn(address) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_global_is_its_address() {
        let g: Global<u32> = Global::at(eix_sites::DATA_7AF61C);
        assert_eq!(g.address(), eix_sites::DATA_7AF61C);
        assert_eq!(g.as_ptr() as usize, eix_sites::DATA_7AF61C);
    }

    #[test]
    fn a_global_reads_and_writes_what_is_there() {
        let mut cell = 5u32;
        let g: Global<u32> = Global::at(core::ptr::from_mut(&mut cell).expose_provenance());
        // SAFETY: the address is a live local u32 on this thread.
        unsafe { g.write(9) };
        // SAFETY: as above.
        assert_eq!(unsafe { g.read() }, 9);
        assert_eq!(cell, 9);
    }
}
