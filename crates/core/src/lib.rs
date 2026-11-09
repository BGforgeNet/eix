#![warn(missing_docs, unreachable_pub, clippy::missing_panics_doc)]
#![forbid(unsafe_code)]
//! Platform-free logic: parsers, the config model, formulas. Builds and tests on any host.

pub mod auto_pause;
pub mod auto_recover;
pub mod cli;
pub mod console_edit;
pub mod crash_record;
pub mod ei_plugin;
pub mod eix_ini;
pub mod engine_fixes;
pub mod fixes;
pub mod groups;
pub mod heap_padding;
pub mod mod_files;
pub mod msvc_map;
pub mod num;
pub mod options;
pub mod reg;
pub mod rules;
pub mod script;
pub mod spelladdon;
pub mod stats;
pub mod vertex_lighting;
pub mod widescreen;
pub mod x86_flags;
