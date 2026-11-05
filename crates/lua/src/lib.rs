#![warn(missing_docs, unreachable_pub, clippy::missing_panics_doc)]
//! The Lua host: LuaJIT 2.1 with FFI, as addon.dll 0.12.4.2 embeds it; ours is a newer 2.1 than that addon's beta3.
//! [`Host`] owns the one state, runs the mod's main script, and calls the 19 callbacks; the 56
//! functions registered for scripts reach the game through [`Game`].
//!
//! Every Lua error passes through mlua's safe API or a raw C function that holds nothing to drop, because LuaJIT
//! raises errors as SEH on 32-bit Windows and would skip Rust destructors.

mod bind;
mod callbacks;
mod convert;
mod functions;
mod game;
mod handler;
mod host;
mod memory;
mod state;

pub use functions::record;
pub use game::{Game, SlotKind, Table, TableBounds};
pub use host::{FatalReport, Host, error_box_and_exit};
pub use state::new_state;
