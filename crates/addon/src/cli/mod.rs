//! The addon's command line (`game.exe addon ...`, core's [`eix_core::cli`]): the parse before the patch groups,
//! the commands' work after them, and their output on standard output.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use eix_core::cli::{
    self as core_cli, DEFAULT_RUNNER, Phase, RUNNER_VARIABLE, Step, WORKDIR_VARIABLE,
};
use eix_core::options::CommandLine;
use eix_engine::win::{error_box, exit_process, write_stdout};

use crate::setup::{self, Setup, ansi, path};
use crate::{log, lua};

mod aigraph;
mod check;

/// What the parse before the patch groups decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Early {
    /// The command has work after the patch groups ([`late`]).
    pub(crate) late: bool,
}

/// The process's arguments in the ANSI code page, the program first (`CommandLineToArgvW` of
/// `GetCommandLineW`, each converted).
fn args() -> Vec<Vec<u8>> {
    std::env::args_os().map(|a| ansi(&a)).collect()
}

/// The program name for the usage texts.
fn runner() -> Vec<u8> {
    std::env::var_os(RUNNER_VARIABLE).map_or_else(|| DEFAULT_RUNNER.to_vec(), |v| ansi(&v))
}

/// Writes `text` to standard output as the original's `printf` does (text mode).
pub(crate) fn print(text: &[u8]) {
    if !write_stdout(&core_cli::crt_text(text)) {
        log::debug(
            "cli",
            "no standard output for",
            &String::from_utf8_lossy(text),
        );
    }
}

/// Ends the process with `code`, as the original's `exit(code)`, without its runtime's teardown: that
/// teardown stops the game on the "R6025 pure virtual function call" box instead of ending it.
fn exit(code: i32) -> ! {
    log::info(
        "cli",
        &format!("the command ends the process with code {code}"),
        "",
    );
    exit_process(code as u32)
}

/// Stops the game with `text` in a box and exit code 1, as the original's command-line errors do.
fn fatal(text: &str) -> ! {
    log::error("cli", text, "");
    error_box("addon.dll", text);
    exit_process(1)
}

/// The folder a directory variable names, made absolute; `None` when it is not a folder.
fn folder(value: &OsString) -> Option<PathBuf> {
    let full = std::path::absolute(value).ok()?;
    full.is_dir().then_some(full)
}

/// The two folders `check` resolves its paths with: `ADDON_WORKDIR` (default `.`) and the working folder.
#[derive(Debug)]
pub(crate) struct Folders {
    work: PathBuf,
    current: PathBuf,
}

impl Folders {
    /// Both folders, or the original's fatal box when one is not a folder.
    fn read() -> Self {
        let work = std::env::var_os(WORKDIR_VARIABLE).unwrap_or_else(|| ".".into());
        match (folder(&work), folder(&".".into())) {
            (Some(work), Some(current)) => Self { work, current },
            _ => fatal("Fatal error: cannot initialize addon cmdline parser"),
        }
    }

    /// `arg` made absolute against the work folder: the working folder changes to it for the call and
    /// back. A path that cannot be resolved stops the game with the original's box.
    fn resolve(&self, arg: &[u8]) -> Vec<u8> {
        let fail = || -> ! {
            fatal(&format!(
                "Cannot resolve full path of {}",
                String::from_utf8_lossy(arg)
            ))
        };
        if std::env::set_current_dir(&self.work).is_err() {
            fail();
        }
        let full = std::path::absolute(path(arg));
        if std::env::set_current_dir(&self.current).is_err() {
            fail();
        }
        match full {
            Ok(full) => ansi(full.as_os_str()),
            Err(_) => fail(),
        }
    }
}

/// Whether `file` names an existing file that is not a folder.
fn is_file(file: &[u8]) -> bool {
    Path::is_file(&path(file))
}

/// The parse before the patch groups: runs `--moddir` (the config of that mod, then its main.lua
/// again) and ends the process for `help`, `version` and every parse error. Sets `setup`'s command line.
pub(crate) fn early(setup: &mut Setup, exe_enables_t2: bool) -> Early {
    let args = args();
    let runner = runner();
    // The original makes its folders when it starts, with or without arguments, and stops when they fail.
    Folders::read();
    let parse = core_cli::parse(&args, Phase::Early, &runner);
    let mut graph = false;
    for step in parse.steps {
        match step {
            Step::ModDir(name) => {
                let dir = setup::named_mod_dir(&name);
                log::info(
                    "cli",
                    "--moddir: the options of",
                    &String::from_utf8_lossy(&dir),
                );
                *setup = setup::read_for(exe_enables_t2, dir);
                log::set_threshold(setup.log_level);
                crate::report::set_console_threshold(setup.console_log_level);
                lua::run_main_script(setup);
            }
            Step::GraphGen => graph = true,
            Step::Exit { text, code } => {
                print(&text);
                exit(code);
            }
            Step::Check(_) | Step::AiGraph(_) => {
                log::error("cli", "a late command in the early parse", "");
            }
        }
    }
    let command_line = match (parse.addon, graph) {
        (false, _) => CommandLine::None,
        (true, false) => CommandLine::Addon,
        (true, true) => CommandLine::AiGraph,
    };
    setup.options.env.command_line = command_line;
    if parse.addon {
        log::info("cli", "game.exe addon", &format!("{command_line:?}"));
    }
    Early { late: parse.late }
}

/// The parse after the patch groups, when [`early`] found a command with work for then: runs `check`
/// or `aigraph` and ends the process with its code.
pub(crate) fn late(setup: &Setup) {
    let runner = runner();
    let parse = core_cli::parse(&args(), Phase::Late, &runner);
    for step in parse.steps {
        let code = match step {
            Step::Check(paths) => check::run(&paths, &runner, &Folders::read()),
            Step::AiGraph(args) => aigraph::run(&args, &runner, &setup.mod_dir),
            Step::Exit { text, code } => {
                print(&text);
                code
            }
            Step::ModDir(_) | Step::GraphGen => continue,
        };
        exit(code);
    }
}
