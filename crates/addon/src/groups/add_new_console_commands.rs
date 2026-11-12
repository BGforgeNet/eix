//! AddNewConsoleCommands: the console's `help` names the addon's commands, and `addon`, `execs`, `listgvar`,
//! `drawall` (after the cheat word) and `nogui` (always) are added.
//!
//! `Execute` 0x5DC620 registers the game's commands on every call; the hook at 0x5DD565, after the line is split,
//! registers these the same way, keeping the game's `help` to call it first. `nogui` hides the battle screen's
//! panels through its draw (the call of `CUiContainer::DrawContainerImpl` at 0x6360DF) and the cursor.
//!
//! The commands' texts are the original's; `addon debug` sets addon.log's level and the console logger's.
#![expect(
    unsafe_code,
    reason = "registering console commands and reading the game's script variables"
)]

use std::ffi::{CStr, CString};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use eix_core::ei_plugin::addon_version_text;
use eix_core::num::atoi;
use eix_engine::game::console::{self, CHEAT_COMMANDS, CommandHandler, PUBLIC_COMMANDS};
use eix_engine::game::flow::DRAW_ALL;
use eix_engine::game::mfc::{self, FormatArg, GameArray, GameString};
use eix_engine::game::object::EngineObject;
use eix_engine::game::script::{MISSION_SCRIPT, Script, ScriptGroup, ScriptObjectRef};
use eix_engine::game::ui::{self, UiContainer};
use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use super::{BuildError, Labelled, Setup};
use crate::{log, report};

/// `sete cl; cmp al, 0x40` in `Execute`, after the line is split into words.
const REGISTER: Site = eix_sites::TEXT_5DD565;
const REGISTER_BYTES: [u8; 5] = [0x0F, 0x94, 0xC1, 0x3C, 0x40];
/// The battle screen's `call 0x600EF0` (`CUiContainer::DrawContainerImpl`) in `CBattleScreen::Draw` 0x636020.
const DRAW: Site = eix_sites::C_BATTLE_SCREEN_DRAW__BF;
const DRAW_BYTES: [u8; 5] = [0xE8, 0x0C, 0xAE, 0xFC, 0xFF];

/// The game's `help`, saved at each registration.
static GAME_HELP: AtomicUsize = AtomicUsize::new(0);
/// The draw call's old target.
static DRAW_CONTAINER: AtomicUsize = AtomicUsize::new(0);
static GUI_SHOWN: AtomicBool = AtomicBool::new(true);
static CURSOR_SHOWN: AtomicBool = AtomicBool::new(true);
/// The battle screen applies [`GUI_SHOWN`] to its panels while set: after a `nogui`, once to show them, every frame
/// while hidden.
static APPLY_GUI: AtomicBool = AtomicBool::new(false);
/// The mod folder for `addon info`.
static MOD_DIR: OnceLock<Vec<u8>> = OnceLock::new();

/// Prints `text` and a line break on the console.
fn say(text: &[u8]) {
    let line = crate::setup::c_string_until_nul(text);
    // SAFETY: console commands run on the game's thread inside `Execute`, with the console built.
    unsafe { console::print_line(&line) };
}

/// The command's arguments (lower case).
///
/// # Safety
/// `args` must be the argument array `Execute` passes a command, alive for the call.
unsafe fn arguments(args: *mut GameArray<GameString>) -> Vec<Vec<u8>> {
    // SAFETY: the caller guarantees a constructed string array.
    let array = unsafe { &*args };
    // SAFETY: as above.
    let strings = unsafe { array.as_slice() };
    // SAFETY: each element is a constructed game string.
    strings
        .iter()
        .map(|s| unsafe { s.as_bytes() }.to_vec())
        .collect()
}

/// `help`: the game's help, then the addon's line.
extern "fastcall" fn help(args: *mut GameArray<GameString>) {
    let game = GAME_HELP.load(Ordering::Relaxed);
    if game != 0 {
        // SAFETY: the slot held the game's own `help` handler when it was saved, a CommandHandler.
        let f: CommandHandler = unsafe { hook::as_fn_at(game) };
        // SAFETY: the game's handler with the arguments `Execute` passed this one.
        unsafe { f(args) };
    }
    say(b"ADDON [command]");
    say(b"addon-specific commands");
}

/// The `addon debug` usage text.
fn debug_usage(log_level: i32, console_level: i32) -> String {
    format!(
        "Usage:\n\taddon debug {{type}} [level]\n\nSet debug level for type of addon's debugging settings\n\
         or show current value if level is not specified\n\nTypes:\n* log      - addon log file (current: \
         {log_level})\n* console  - addon's messages to console (current: {console_level})\n\nLevels:\n* 4        - \
         disable\n* 3        - error\n* 2        - warning\n* 1        - information\n* 0        - debug"
    )
}

/// What `addon debug <type> [level]` answers; `set` takes a new level for a type (`true` for the log).
/// The console's text is ANSI, so an argument echoed back keeps its own bytes.
fn debug(args: &[Vec<u8>], mut set: impl FnMut(bool, i32), levels: (i32, i32)) -> Option<Vec<u8>> {
    let kind: &[u8] = args.first().map_or(b"", Vec::as_slice);
    let level = args.get(1).map_or(-1, |a| atoi(a));
    match kind {
        b"" | b"help" => Some(debug_usage(levels.0, levels.1).into_bytes()),
        b"console" | b"log" => {
            let log = kind == b"log";
            if level >= 0 {
                set(log, level.min(4));
                None
            } else if log {
                Some(format!("Current addon log level: {}", levels.0).into_bytes())
            } else {
                Some(format!("Current console log level: {}", levels.1).into_bytes())
            }
        }
        other => Some([&b"Unknown debug type: "[..], other].concat()),
    }
}

fn welcome() -> String {
    format!(
        "Welcome to Evil Islands Addon Engine v{}\n\nAvailable commands:\n\
         * addon info          - show detailed information about addon\n\
         * addon debug [args]  - set addon's debug settings",
        addon_version_text()
    )
}

/// What `addon ...` answers.
fn addon_text(
    args: &[Vec<u8>],
    mod_dir: &[u8],
    set: impl FnMut(bool, i32),
    levels: (i32, i32),
) -> Option<Vec<u8>> {
    match args.first().map(Vec::as_slice) {
        None | Some(b"help") => Some(welcome().into_bytes()),
        Some(b"info") => {
            let current: &[u8] = if mod_dir.is_empty() {
                b"No mod is used"
            } else {
                mod_dir
            };
            let head = format!(
                "Addon core version: {}\nCurrent mod: ",
                addon_version_text()
            );
            Some([head.as_bytes(), current].concat())
        }
        Some(b"debug") => debug(&args[1..], set, levels),
        Some(other) => Some([&b"Unknown command: "[..], other].concat()),
    }
}

extern "fastcall" fn addon(args: *mut GameArray<GameString>) {
    // SAFETY: `Execute` passes its argument array.
    let args = unsafe { arguments(args) };
    let mod_dir = MOD_DIR.get().map(Vec::as_slice).unwrap_or_default();
    let set = |log_file: bool, level: i32| {
        if log_file {
            log::set_threshold(level);
        } else {
            report::set_console_threshold(level);
        }
    };
    if let Some(text) = addon_text(
        &args,
        mod_dir,
        set,
        (log::threshold(), report::console_threshold()),
    ) {
        say(&text);
    }
}

/// `execs <file>`: loads a script file into the zone's script and runs it, or prints the script error.
extern "fastcall" fn execs(args: *mut GameArray<GameString>) {
    // SAFETY: `Execute` passes its argument array.
    let args = unsafe { arguments(args) };
    // The original reads the first argument without checking that there is one; without it nothing happens.
    let Some(file) = args.first() else {
        return;
    };
    let Ok(file) = CString::new(file.clone()) else {
        return;
    };
    let script = MISSION_SCRIPT.as_ptr();
    // SAFETY: the game's thread; the zone's script is a constructed static of the server world.
    if unsafe { Script::load_file(script, &file) } {
        // SAFETY: as above.
        unsafe { Script::tick(script) };
        return;
    }
    // SAFETY: as above.
    let s = unsafe { &*script };
    // SAFETY: the error text is a constructed string.
    let text = unsafe { s.error_text.as_bytes() }.to_vec();
    let line = s.error_line;
    say(&[format!("Script error: line {line}: ").as_bytes(), &text].concat());
}

/// listgvar's object text: `null`, or `<class:id>`.
///
/// # Safety
/// `object` must be null or a live engine object, on the game's thread.
unsafe fn object_text(object: *mut EngineObject) -> Vec<u8> {
    if object.is_null() {
        return b"null".to_vec();
    }
    // SAFETY: the caller guarantees a live object.
    let name = unsafe { EngineObject::class_name(object) };
    // SAFETY: the class name is a static C string.
    let name = unsafe { CStr::from_ptr(name.cast()) };
    // SAFETY: as above.
    let id = unsafe { (*object).id };
    [b"<", name.to_bytes(), format!(":{id}>").as_bytes()].concat()
}

/// listgvar's group text: `empty`, or the objects joined by `, `.
///
/// # Safety
/// `group` must be a live group of live references, on the game's thread.
unsafe fn group_text(group: &ScriptGroup) -> Vec<u8> {
    // SAFETY: the caller guarantees the group.
    let members = unsafe { group.as_slice() };
    if members.is_empty() {
        return b"empty".to_vec();
    }
    let texts: Vec<Vec<u8>> = members
        .iter()
        // SAFETY: each reference holds a live object or null.
        .map(|r: &ScriptObjectRef| unsafe { object_text(r.object) })
        .collect();
    texts.join(&b", "[..])
}

/// The characters of name `i` of `names` (a C string), or an empty one past the end.
///
/// # Safety
/// `names` must be a constructed string array of the game, unchanged while the pointer is used.
unsafe fn name_at(names: &GameArray<GameString>, i: usize) -> *const u8 {
    // SAFETY: the caller guarantees the array.
    let names = unsafe { names.as_slice() };
    names
        .get(i)
        .map_or(c"".as_ptr().cast(), |s| s.chars.cast_const())
}

/// A listgvar line, formatted by the game's printf as the original formats it.
///
/// # Safety
/// On the game's thread; each `Str` argument a live C string.
unsafe fn line(format: &CStr, args: &[FormatArg]) {
    // SAFETY: passed on.
    say(&unsafe { mfc::format_bytes(format, args) });
}

/// `listgvar`: the zone script's global variables, floats, strings, objects and groups.
extern "fastcall" fn listgvar(_: *mut GameArray<GameString>) {
    // SAFETY: the game's thread; the zone's script is a constructed static.
    let globals = unsafe { &(*MISSION_SCRIPT.as_ptr()).globals };
    // SAFETY: constructed arrays of the script, unchanged while listed.
    for (i, v) in unsafe { globals.floats.as_slice() }.iter().enumerate() {
        // SAFETY: the name is a constructed string of the script.
        let name = unsafe { name_at(&globals.float_names, i) };
        let args = [FormatArg::Str(name), FormatArg::Double(f64::from(*v))];
        // SAFETY: as above.
        unsafe { line(c"%s : float = %f", &args) };
    }
    // SAFETY: as above.
    for (i, v) in unsafe { globals.strings.as_slice() }.iter().enumerate() {
        // SAFETY: as above.
        let name = unsafe { name_at(&globals.string_names, i) };
        let args = [FormatArg::Str(name), FormatArg::Str(v.chars.cast_const())];
        // SAFETY: as above; the value is a constructed string.
        unsafe { line(c"%s : string = \"%s\"", &args) };
    }
    // SAFETY: as above.
    for (i, v) in unsafe { globals.objects.as_slice() }.iter().enumerate() {
        // SAFETY: a variable's reference holds a live object or null.
        let text = CString::new(unsafe { object_text(v.object) }).unwrap_or_default();
        // SAFETY: as above.
        let name = unsafe { name_at(&globals.object_names, i) };
        let args = [FormatArg::Str(name), FormatArg::Str(text.as_ptr().cast())];
        // SAFETY: as above; `text` lives across the call.
        unsafe { line(c"%s : object = %s", &args) };
    }
    // SAFETY: as above.
    for (i, v) in unsafe { globals.groups.as_slice() }.iter().enumerate() {
        // SAFETY: a variable's group of live references.
        let text = CString::new(unsafe { group_text(v) }).unwrap_or_default();
        // SAFETY: as above.
        let name = unsafe { name_at(&globals.group_names, i) };
        let args = [FormatArg::Str(name), FormatArg::Str(text.as_ptr().cast())];
        // SAFETY: as above; `text` lives across the call.
        unsafe { line(c"%s : group = %s", &args) };
    }
}

/// `drawall <n>`: the draw-all flag becomes 1 for 0, else 0.
extern "fastcall" fn drawall(args: *mut GameArray<GameString>) {
    // SAFETY: `Execute` passes its argument array.
    let args = unsafe { arguments(args) };
    if let Some(first) = args.first() {
        // SAFETY: the game's thread; the flag is a plain word the client world reads.
        unsafe { DRAW_ALL.write(u32::from(atoi(first) == 0)) };
    }
}

const NOGUI_USAGE: &str = "Hide or show GUI in game missions\n\nnogui 0 - show all (default)\nnogui 1 - hide GUI but \
                           show cursor\nnogui 2 - hide GUI and cursor";

/// What `nogui` does: the mode 0..=2 for one argument, else the usage.
fn nogui_mode(args: &[Vec<u8>]) -> Option<u32> {
    let mode = if args.len() == 1 { atoi(&args[0]) } else { -1 };
    u32::try_from(mode).ok().filter(|&m| m <= 2)
}

extern "fastcall" fn nogui(args: *mut GameArray<GameString>) {
    // SAFETY: `Execute` passes its argument array.
    let args = unsafe { arguments(args) };
    let Some(mode) = nogui_mode(&args) else {
        say(NOGUI_USAGE.as_bytes());
        return;
    };
    let cursor = mode < 2;
    let gui = mode == 0;
    CURSOR_SHOWN.store(cursor, Ordering::Relaxed);
    GUI_SHOWN.store(gui, Ordering::Relaxed);
    APPLY_GUI.store(true, Ordering::Relaxed);
    let word = |shown: bool| if shown { "show" } else { "hide" };
    say(format!("GUI: {}, cursor: {}", word(gui), word(cursor)).as_bytes());
    // SAFETY: the game's thread; the cursor is a constructed static.
    unsafe { ui::show_cursor(cursor) };
}

/// Registers the addon's commands, as the game registers its own on every call.
extern "C" fn register(_: usize, _: &mut Context) {
    let cheat = CHEAT_COMMANDS.as_ptr();
    let public = PUBLIC_COMMANDS.as_ptr();
    let mut key = std::mem::MaybeUninit::<GameString>::uninit();
    // SAFETY: the game's thread inside `Execute`: the maps and the string code are live; `key` holds no string yet.
    unsafe { mfc::construct(key.as_mut_ptr(), c"help") };
    // SAFETY: `key` is constructed; the cheat map is live.
    let slot = unsafe { console::command_slot(cheat, key.as_ptr()) };
    // SAFETY: the map's live value slot.
    let game = unsafe { slot.read() }.map_or(0, |h| h as usize);
    GAME_HELP.store(game, Ordering::Relaxed);
    // SAFETY: `key` is a constructed string, not used again.
    unsafe { mfc::destroy(key.as_mut_ptr()) };
    let commands: [(*mut console::CommandMap, &CStr, CommandHandler); 6] = [
        (cheat, c"help", help),
        (cheat, c"addon", addon),
        (cheat, c"execs", execs),
        (cheat, c"listgvar", listgvar),
        (cheat, c"drawall", drawall),
        (public, c"nogui", nogui),
    ];
    for (map, name, handler) in commands {
        // SAFETY: as above; the handlers live as long as the DLL, which the game never unloads.
        unsafe { console::register_command(map, name, handler) };
    }
}

type DrawContainer = unsafe extern "thiscall" fn(*mut UiContainer, u32) -> u32;

/// Stands in for the battle screen's call of `CUiContainer::DrawContainerImpl`: while `nogui` asks for it, every
/// panel gets the GUI's visibility first.
extern "thiscall" fn draw_battle_screen(screen: *mut UiContainer, arg: u32) -> u32 {
    if APPLY_GUI.load(Ordering::Relaxed) {
        let shown = GUI_SHOWN.load(Ordering::Relaxed);
        // SAFETY: the game draws its live battle screen on its thread.
        let children = unsafe { &(*screen).children };
        // SAFETY: the screen's constructed child array.
        for &panel in unsafe { children.as_slice() } {
            // SAFETY: each child is a live window of the screen.
            unsafe { ui::set_window_enabled(panel, shown) };
        }
        APPLY_GUI.store(!shown, Ordering::Relaxed);
    }
    // SAFETY: the old target is 0x600EF0, of this type, stored before the call was redirected.
    let original: DrawContainer = unsafe { hook::as_fn_at(DRAW_CONTAINER.load(Ordering::SeqCst)) };
    // SAFETY: the game's own call, with its arguments.
    unsafe { original(screen, arg) }
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let _ = MOD_DIR.set(setup.mod_dir.clone());
    let (draw, old) =
        hook::redirect_call(DRAW, DRAW_BYTES, draw_battle_screen as *const () as usize)?;
    DRAW_CONTAINER.store(old.here(), Ordering::SeqCst);
    Ok(vec![
        Labelled {
            label: "AddNewConsoleCommands #0".to_owned(),
            write: draw,
        },
        Labelled {
            label: "AddNewConsoleCommands #1".to_owned(),
            write: hook::context_hook(REGISTER, &REGISTER_BYTES, Mode::Before, register, 0)?,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<Vec<u8>> {
        words.iter().map(|w| w.as_bytes().to_vec()).collect()
    }

    fn text(words: &[&str], mod_dir: &[u8]) -> (Option<String>, Vec<(bool, i32)>) {
        let mut set = Vec::new();
        let out = addon_text(&args(words), mod_dir, |l, v| set.push((l, v)), (1, 3));
        (out.map(|t| String::from_utf8_lossy(&t).into_owned()), set)
    }

    #[test]
    fn addon_answers_as_the_original() {
        assert_eq!(
            text(&[], b"").0.as_deref(),
            Some(
                "Welcome to Evil Islands Addon Engine v0.12.4.2\n\nAvailable commands:\n\
                 * addon info          - show detailed information about addon\n\
                 * addon debug [args]  - set addon's debug settings"
            )
        );
        assert_eq!(
            text(&["info"], b"").0.as_deref(),
            Some("Addon core version: 0.12.4.2\nCurrent mod: No mod is used")
        );
        assert_eq!(
            text(&["info"], b"C:\\Mods\\x\\").0.as_deref(),
            Some("Addon core version: 0.12.4.2\nCurrent mod: C:\\Mods\\x\\")
        );
        assert_eq!(
            text(&["what"], b"").0.as_deref(),
            Some("Unknown command: what")
        );
        assert_eq!(
            text(&["debug", "log"], b"").0.as_deref(),
            Some("Current addon log level: 1")
        );
        assert_eq!(
            text(&["debug", "console", "9"], b""),
            (None, vec![(false, 4)])
        );
        assert_eq!(text(&["debug", "log", "0"], b""), (None, vec![(true, 0)]));
        assert_eq!(
            text(&["debug", "x"], b"").0.as_deref(),
            Some("Unknown debug type: x")
        );
        // A non-ASCII argument goes back to the ANSI console as the bytes typed.
        let out = addon_text(
            &[b"debug".to_vec(), vec![0xC0, 0xE1]],
            b"",
            |_, _| {},
            (1, 3),
        );
        assert_eq!(out.as_deref(), Some(&b"Unknown debug type: \xC0\xE1"[..]));
        let usage = text(&["debug"], b"").0.unwrap_or_default();
        assert!(usage.contains("addon log file (current: 1)"));
        assert!(usage.contains("messages to console (current: 3)"));
    }

    #[test]
    fn nogui_takes_one_mode() {
        assert_eq!(nogui_mode(&args(&["2"])), Some(2));
        assert_eq!(nogui_mode(&args(&["3"])), None);
        assert_eq!(nogui_mode(&args(&["-1"])), None);
        assert_eq!(nogui_mode(&args(&[])), None);
        assert_eq!(nogui_mode(&args(&["0", "1"])), None);
        assert_eq!(nogui_mode(&args(&["x"])), Some(0));
    }
}
