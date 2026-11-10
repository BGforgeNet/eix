#![warn(missing_docs, unreachable_pub, clippy::missing_panics_doc)]
#![expect(
    unsafe_code,
    reason = "the DLL's exports (Handler, Apply) and DllMain are its ABI"
)]
//! `addon.dll`: mod loading, plugins, patch groups, console, Lua.
//!
//! The loader calls [`Apply`] from game.exe's entry point. It checks the loader contract, then points the CRT's
//! call of WinMain at the addon, whose work (options, plugins, patch tables) runs before the game's WinMain.

use std::ffi::c_void;

use eix_engine::win::{error_box, exit_process};

mod auto_pause;
mod auto_recover;
mod cli;
mod crash;
mod encumbrance;
mod environment;
mod fixes;
mod flac_music;
mod game;
mod groups;
mod ini;
mod ini_variables;
mod log;
mod lua;
mod plugins;
mod report;
mod setup;
mod spell_addon;

const DLL_PROCESS_ATTACH: u32 = 1;
const DLL_PROCESS_DETACH: u32 = 0;

/// Writes a panic's message and place to addon.log; `panic = "abort"` then ends the process.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let at = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic without a message".to_owned());
        log::error("panic", &message, &at);
    }));
}

/// The patch work, from the WinMain hook: options, plugins before, the tables, plugins after.
fn addon_main(token: &game::BeforeWinMain) {
    log::info("main", "WinMain hook: applying the mod", "");
    let exe_enables_t2 = game::exe_enables_t2();
    let mut setup = setup::read(exe_enables_t2);
    log::set_threshold(setup.log_level);
    report::set_console_threshold(setup.console_log_level);
    // The original runs main.lua from its config read, before plugins and patch groups.
    lua::run_main_script(&setup);
    let command = cli::early(&mut setup, exe_enables_t2);
    // The original reads the setting with the rest of its options, and a crash before that backs nothing up.
    if setup.options.settings.auto_recover != 0 {
        auto_recover::enable(&setup.mod_dir);
    }
    let plugin = &setup.options.config.dev.plugin;
    // The original joins the value to the mod folder and loads when that is not empty: an empty value names the mod
    // folder itself, whose `*.plg` files load.
    let plugins = if setup.mod_dir.is_empty() {
        plugins::Plugins::default()
    } else {
        plugins::Plugins::load(&setup.mod_dir, plugin)
    };
    plugins.call(c"EntryBeforePatch", &setup.mod_dir);
    let tally = groups::run(token, &setup);
    log::info(
        "groups",
        &format!(
            "{} applied, {} disabled, {} failed",
            tally.applied, tally.disabled, tally.failed
        ),
        "",
    );
    fixes::run(token, &setup);
    auto_pause::install(token, setup.options.eix.auto_pause);
    flac_music::install(token, setup.options.eix.music, &setup.mod_dir);
    encumbrance::install(token, setup.options.eix.encumbrance);
    ini_variables::install(token, &setup.mod_dir);
    // SpellAddonXa.asi loads with the game's sound start-up, which a command with work after the patch groups ends
    // the process before.
    if !command.late {
        spell_addon::install(token, &setup.mod_dir);
    }
    plugins.call(c"EntryAfterPatch", &setup.mod_dir);
    if command.late {
        cli::late(&setup);
    }
    // After the plugins and the patches: the modules the game starts with.
    environment::log_modules("modules");
    log::info("main", "WinMain hook done, starting the game", "");
}

/// Ordinal 2, called by the loader with no arguments before game.exe's WinMain runs.
///
/// # Panics
/// Only in a build with the `test-panic` feature, on purpose.
///
/// # Safety
/// Called by the addon loader only, from game.exe's entry point, on its only thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Apply() {
    install_panic_hook();
    let path = setup::log_path();
    // The original goes on silently without an addon.log it cannot open; the box is ours, kept so that the cause
    // shows.
    if let Err(e) = log::open(&path) {
        error_box("addon.dll", format!("cannot open {}: {e}", path.display()));
    }
    report::open_errors_log(&path);
    crash::install(&path, setup::full_dump());
    log::info(
        "main",
        &format!("{} loaded by the new loader", environment::build()),
        "",
    );
    environment::log_system();
    if cfg!(feature = "test-panic") {
        panic!("test-panic build: forced panic in Apply");
    }
    if let Err(e) = game::known_exe() {
        log::error("main", &e, "");
        error_box("addon.dll", &e);
        exit_process(1);
    }
    let build = match eix_sites::selected() {
        eix_sites::Build::Ru106 => "game.exe: Russian 1.06",
        eix_sites::Build::En106 => "game.exe: English/German 1.06",
    };
    log::info("main", build, "");
    // The original creates the state while addon.dll loads, whatever the config says; this is our load-time entry.
    lua::create();
    if !game::loader_contract_holds() {
        let text = "Apply cannot be called from old loader!";
        log::error("main", text, "0x0073A670");
        error_box("addon.dll", text);
        exit_process(1);
    }
    if let Err(e) = game::hook_win_main() {
        let text = format!("Failed to hook WinMain: {e}");
        log::error("main", &text, "0x006EC301");
        error_box("addon.dll", &text);
        exit_process(1);
    }
    log::info("main", "WinMain hooked", "0x006EC301");
}

/// Ordinal 1, the entry an old loader jumps to when the addon has no `Apply`; this addon needs the new loader.
///
/// # Safety
/// Never called by a current loader.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Handler() {
    let text = "Unhandled call detected: this addon.dll needs the new addon loader.";
    log::error("main", text, "");
    error_box("addon.dll", text);
    exit_process(1);
}

/// The DLL entry point.
///
/// # Safety
/// Called by the Windows loader only.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(
    module: *mut c_void,
    reason: u32,
    _reserved: *mut c_void,
) -> i32 {
    match reason {
        // SAFETY: `module` is our own handle, as the Windows loader passes it.
        DLL_PROCESS_ATTACH => unsafe { eix_engine::win::disable_thread_library_calls(module) },
        DLL_PROCESS_DETACH => log::info("main", "cleanup: the process is ending", ""),
        _ => {}
    }
    1
}
