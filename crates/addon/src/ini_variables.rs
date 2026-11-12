//! INI variables: a script's `GSGetVar(player, "ini.<file>.<section>.<key>")` is the value of `[section] key` in
//! `<file>.ini`, the mod folder's first, else the game folder's; 0 when the file, section or key is missing or the
//! value is not a number. Each file is read once, at the first variable that names it, so a change applies at the
//! next game start. Nothing goes into the player's variables: no save holds the values and no client is sent them.
//! Other names keep the game's lookup, and without eix the game reads 0 for such a name, as for any name never set.
//!
//! The hook is at the start of `GSGetVar`'s case in `SSCCmd::CalcFloat` 0x4B41E0 (0x4B505A, `mov ecx, 0x7AFE20`
//! before the multiplayer client's test): ESI is the call, its first string argument the name, and `[ebp+0x10]` the
//! result's address. For an INI variable the handler writes the result and resumes at the case's return (0x4B5141),
//! where the stack is as at the case's start.
#![expect(
    unsafe_code,
    reason = "reads the script call's name and writes its result"
)]

use std::sync::{Mutex, OnceLock};

use eix_core::ini::{self, Document};
use eix_engine::game::script::ScriptCommand;
use eix_engine::hook::{self, Context, Mode};

use crate::game::BeforeWinMain;
use crate::groups::{Labelled, Tally, apply_built};
use crate::log;
use crate::lua::ptr;
use crate::setup::{ansi, path};

/// The mod folder (ending in `\`, empty without a mod) and the game folder (the working folder, without a `\`).
static FOLDERS: OnceLock<(Vec<u8>, Vec<u8>)> = OnceLock::new();

/// The files read, by lowercase name: `None` for one in neither folder.
static FILES: Mutex<Vec<(Vec<u8>, Option<Document>)>> = Mutex::new(Vec::new());

/// The variables already reported as reading 0, by name.
static REPORTED: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());

fn shown(bytes: &[u8]) -> String {
    eix_engine::win::ansi_text(bytes)
}

/// `<file>.ini` from the mod folder, else the game folder; `None` when neither has it.
fn load(file: &[u8]) -> Option<Document> {
    let name = [file, b".ini"].concat();
    let (mod_dir, game_dir) = FOLDERS.get()?;
    let candidates = [
        (!mod_dir.is_empty()).then(|| [&mod_dir[..], &name].concat()),
        (!game_dir.is_empty()).then(|| [&game_dir[..], b"\\", &name].concat()),
    ];
    for candidate in candidates.into_iter().flatten() {
        match std::fs::read(path(&candidate)) {
            Ok(text) => {
                log::info("ini", "variables read from", &shown(&candidate));
                return Some(Document::parse(&text));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn("ini", &format!("cannot read: {e}"), &shown(&candidate)),
        }
    }
    log::info(
        "ini",
        "in neither the mod folder nor the game folder: its variables read 0",
        &shown(&name),
    );
    None
}

/// Logs once per name why it reads 0: a script may read it every tick.
fn report_once(what: &str, name: &[u8]) {
    let first = REPORTED.lock().is_ok_and(|mut seen| {
        let new = !seen.iter().any(|n| n.eq_ignore_ascii_case(name));
        if new {
            seen.push(name.to_vec());
        }
        new
    });
    if first {
        log::info("ini", what, &shown(name));
    }
}

/// The value of the INI variable `name`; `None` for a name that is not one.
fn value(name: &[u8]) -> Option<f32> {
    let variable = ini::variable(name)?;
    let Ok(mut files) = FILES.lock() else {
        log::error("ini", "the INI files' table is unusable", &shown(name));
        return Some(0.0);
    };
    let file = variable.file.to_ascii_lowercase();
    let at = match files.iter().position(|(f, _)| *f == file) {
        Some(at) => at,
        None => {
            let document = load(variable.file);
            files.push((file, document));
            files.len() - 1
        }
    };
    let Some(document) = &files[at].1 else {
        return Some(0.0);
    };
    let Some(text) = document.get(variable.section, variable.key) else {
        report_once("no such section or key: reads 0", name);
        return Some(0.0);
    };
    Some(match ini::parse_number(text) {
        Some(n) => n as f32,
        None => {
            report_once("the value is not a number: reads 0", name);
            0.0
        }
    })
}

/// At the start of `GSGetVar`'s case: an INI variable's value as the result, past the game's lookup.
extern "C" fn get_var(_: usize, ctx: &mut Context) {
    let call = ptr::<ScriptCommand>(ctx.esi);
    // SAFETY: ESI is the call being evaluated, on the game's thread.
    let strings = unsafe { &(*call).string_args };
    // SAFETY: its arguments are bound for this evaluation and do not change during it.
    let Some(first) = (unsafe { strings.as_slice() }).first() else {
        return;
    };
    // SAFETY: as above.
    let name = unsafe { first.as_bytes() };
    let Some(result) = value(name) else {
        return;
    };
    let at = (ctx.ebp as usize).wrapping_add(0x10);
    let Some(out) = ctx.stack_at(at) else {
        log::error(
            "ini",
            "the result's address is off the stack",
            &format!("{at:#x}"),
        );
        return;
    };
    // SAFETY: CalcFloat's third argument is the address it writes its float result to.
    unsafe { ptr::<f32>(out).write(result) };
    ctx.resume = eix_sites::SSC_CMD_CALC_FLOAT__F61;
}

/// Installs the hook: the mod folder from the setup, the game folder as the working folder at start.
pub(crate) fn install(token: &BeforeWinMain, mod_dir: &[u8]) {
    let game_dir = match std::env::current_dir() {
        Ok(dir) => ansi(dir.as_os_str()),
        Err(e) => {
            log::warn(
                "ini",
                &format!("no working folder: {e}"),
                "the game folder is not looked in",
            );
            Vec::new()
        }
    };
    let _ = FOLDERS.set((mod_dir.to_vec(), game_dir));
    let built = hook::context_hook(
        eix_sites::SSC_CMD_CALC_FLOAT__E7A,
        &[0xB9, 0x20, 0xFE, 0x7A, 0x00],
        Mode::After,
        get_var,
        0,
    )
    .map(|write| {
        vec![Labelled {
            label: "IniVariables #0".to_owned(),
            write,
        }]
    })
    .map_err(Into::into);
    let mut tally = Tally::default();
    apply_built(token, "ini", "IniVariables", built, &mut tally);
}
