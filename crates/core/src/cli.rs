//! The addon's own command line, `game.exe addon [--<option> ...] <command> [<argument> ...]`, and the texts it
//! prints.
//!
//! The parser runs twice: before the patch groups ([`Phase::Early`]) and, when the command has work for then, after
//! them ([`Phase::Late`]). Each table entry has a handler per phase; a phase without one goes on. The handlers' work
//! that needs the game is the caller's: this module says what to do ([`Step`]) and with which arguments.

/// The program name the usage texts print when the environment names none (`ADDON_CLI_RUNNER`).
pub const DEFAULT_RUNNER: &[u8] = b"game.exe addon";

/// The environment variable naming the program in the usage texts: a runner script that starts the game with the
/// addon's arguments sets it.
pub const RUNNER_VARIABLE: &str = "ADDON_CLI_RUNNER";

/// The environment variable naming the folder `check` resolves its paths against (default `.`): a runner sets it to
/// the folder the user called it from, since the game runs in its own.
pub const WORKDIR_VARIABLE: &str = "ADDON_WORKDIR";

/// When the parser runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Before the patch groups, from the start of the addon's work.
    Early,
    /// After the patch groups and the plugins' after-patch entries.
    Late,
}

/// What a parse asks the caller to do, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// `--moddir <dir>`: the options go back to their defaults and the config of mod `<dir>` (under the starter's
    /// `Mods\`) is read, which runs its main.lua again. The parse goes on.
    ModDir(Vec<u8>),
    /// `aigraph` before the patch groups: GraphGenPatches is enabled.
    GraphGen,
    /// `check <path>...` after the patch groups, with its arguments.
    Check(Vec<Vec<u8>>),
    /// `aigraph <mode> <mob> [<mpr>]` after the patch groups, with its arguments.
    AiGraph(Vec<Vec<u8>>),
    /// Print `text` and end the process with `code`.
    Exit {
        /// What goes to standard output, as the original's `printf` gets it (line ends `\n`).
        text: Vec<u8>,
        /// The process exit code.
        code: i32,
    },
}

/// The result of one parse.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parse {
    /// The first argument is `addon` (the CommandLine group's switch).
    pub addon: bool,
    /// An entry with a late handler was reached: the parser runs again after the patch groups.
    pub late: bool,
    /// What to do, in order; without an [`Step::Exit`] the game goes on after the last step.
    pub steps: Vec<Step>,
}

/// A handler of the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handler {
    /// The usage, exit code 0.
    Help,
    /// Read another mod's config, go on.
    ModDir,
    /// Check scripts.
    Check,
    /// Switch GraphGenPatches on, go on.
    GraphGen,
    /// Generate the AI graph.
    AiGraph,
    /// Go on, the game runs.
    Run,
    /// The version, exit code 0.
    Version,
}

/// One entry: its name, its early and late handler, and how many arguments it takes.
struct Entry {
    name: &'static [u8],
    early: Option<Handler>,
    late: Option<Handler>,
    params: usize,
}

/// The options, given as `--<name>`.
const OPTIONS: [Entry; 2] = [
    Entry {
        name: b"help",
        early: Some(Handler::Help),
        late: None,
        params: 0,
    },
    Entry {
        name: b"moddir",
        early: Some(Handler::ModDir),
        late: None,
        params: 1,
    },
];

/// The commands.
const COMMANDS: [Entry; 5] = [
    Entry {
        name: b"help",
        early: Some(Handler::Help),
        late: None,
        params: 0,
    },
    Entry {
        name: b"check",
        early: None,
        late: Some(Handler::Check),
        params: 0,
    },
    Entry {
        name: b"aigraph",
        early: Some(Handler::GraphGen),
        late: Some(Handler::AiGraph),
        params: 0,
    },
    Entry {
        name: b"run",
        early: Some(Handler::Run),
        late: None,
        params: 0,
    },
    Entry {
        name: b"version",
        early: Some(Handler::Version),
        late: None,
        params: 0,
    },
];

/// `printf(format, arg)` for a format with one `%s`.
fn format1(format: &[u8], arg: &[u8]) -> Vec<u8> {
    match format.windows(2).position(|w| w == b"%s") {
        Some(at) => [&format[..at], arg, &format[at + 2..]].concat(),
        None => format.to_vec(),
    }
}

/// The general usage.
pub fn usage(runner: &[u8]) -> Vec<u8> {
    format1(
        b"Usage:\n    %s {command} [arguments...]\n\nCommands:\n    check        - Check script for errors\n    run          - Run the game\n    version      - Print addon's version\n    help         - Show this help and exit\n",
        runner,
    )
}

/// The `check` usage.
pub fn check_usage(runner: &[u8]) -> Vec<u8> {
    format1(
        b"Usage:\n    %s check {PATH}...\n\nCheck script(s) for errors.\nPATH can be either a path to a directory or a path to a file.\n1) If it specifies a directory, then the command will check *.mob files there.\n2) Otherwise, the file could be *.mob file or text file with script.\n",
        runner,
    )
}

/// The `aigraph` usage.
pub fn aigraph_usage(runner: &[u8]) -> Vec<u8> {
    format1(
        b"Usage:\n    %s aigraph {single|multi} mob_name [mpr_name]\n\nGenerate AI Graph.\nmpr_name is an optional argument, if it's not specified it will be\ngenerated from mob name. For example: 'zone10-lmp.mob' -> 'zone10.mpr'.\n",
        runner,
    )
}

/// The version: the name, the version this addon stands in for, the copyright line.
pub fn version_text(version: [i32; 4]) -> Vec<u8> {
    format!(
        "Evil Islands Addon Engine {}.{}.{}.{}\nCopyright (C) 2001-2024 Gipat.Ru\n",
        version[0], version[1], version[2], version[3]
    )
    .into_bytes()
}

/// A parse error: the message, then the pointer to `help`; exit code 1.
fn error(format: &[u8], arg: &[u8], runner: &[u8]) -> Step {
    Step::Exit {
        text: [
            format1(format, arg),
            format1(b"Try '%s help' for more information\n", runner),
        ]
        .concat(),
        code: 1,
    }
}

fn find<'a>(table: &'a [Entry], name: &[u8]) -> Option<&'a Entry> {
    // `_stricmp` in the C locale.
    table.iter().find(|e| e.name.eq_ignore_ascii_case(name))
}

/// What one handler does with its arguments; `None` goes on with the parse.
fn run_handler(
    handler: Handler,
    args: &[Vec<u8>],
    runner: &[u8],
    steps: &mut Vec<Step>,
) -> Option<()> {
    match handler {
        Handler::Help => steps.push(Step::Exit {
            text: usage(runner),
            code: 0,
        }),
        Handler::Version => steps.push(Step::Exit {
            text: version_text(crate::ei_plugin::ADDON_VERSION),
            code: 0,
        }),
        Handler::ModDir => {
            steps.push(Step::ModDir(args.first().cloned().unwrap_or_default()));
            return None;
        }
        Handler::GraphGen => {
            steps.push(Step::GraphGen);
            return None;
        }
        Handler::Run => return None,
        Handler::Check => steps.push(Step::Check(args.to_vec())),
        Handler::AiGraph => steps.push(Step::AiGraph(args.to_vec())),
    }
    Some(())
}

/// Parses the process arguments `args` (the program first) for `phase`; `runner` names the program in the texts.
pub fn parse(args: &[Vec<u8>], phase: Phase, runner: &[u8]) -> Parse {
    let mut out = Parse::default();
    if args.len() < 2 || !args[1].eq_ignore_ascii_case(b"addon") {
        return out;
    }
    out.addon = true;
    let handler_of = |e: &Entry| match phase {
        Phase::Early => e.early,
        Phase::Late => e.late,
    };
    let mut i = 2;
    while i < args.len() && args[i].starts_with(b"--") {
        let Some(entry) = find(&OPTIONS, &args[i][2..]) else {
            out.steps
                .push(error(b"Invalid option: '%s'.\n", &args[i], runner));
            return out;
        };
        if entry.params >= args.len() - i {
            out.steps.push(error(
                b"Not enough parameters for option: '%s'.\n",
                &args[i],
                runner,
            ));
            return out;
        }
        out.late |= entry.late.is_some();
        if let Some(h) = handler_of(entry) {
            let params = &args[i + 1..i + 1 + entry.params];
            if run_handler(h, params, runner, &mut out.steps).is_some() {
                return out;
            }
        }
        i += entry.params + 1;
    }
    let Some(word) = args.get(i) else {
        out.steps
            .push(error(b"Not enough arguments.\n", b"", runner));
        return out;
    };
    let Some(entry) = find(&COMMANDS, word) else {
        out.steps
            .push(error(b"Invalid command: '%s'.\n", word, runner));
        return out;
    };
    if entry.params >= args.len() - i {
        out.steps.push(error(
            b"Not enough parameters for command: '%s'.\n",
            word,
            runner,
        ));
        return out;
    }
    out.late |= entry.late.is_some();
    if let Some(h) = handler_of(entry) {
        run_handler(h, &args[i + 1..], runner, &mut out.steps);
    }
    out
}

/// Text as the C runtime writes it to a stream in text mode: every `\n` becomes `\r\n`.
pub fn crt_text(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + text.len() / 8);
    for &c in text {
        if c == b'\n' {
            out.push(b'\r');
        }
        out.push(c);
    }
    out
}

/// The result of checking one file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileCheck {
    /// The script compiled: exit code 0.
    Ok,
    /// A text file that cannot be read: exit code 2.
    Unreadable,
    /// A script error: exit code 4.
    ScriptError,
}

impl FileCheck {
    /// The single-file exit code.
    pub fn code(self) -> i32 {
        match self {
            Self::Ok => 0,
            Self::Unreadable => 2,
            Self::ScriptError => 4,
        }
    }
}

/// Whether `check` takes `path` as a mob file: its text from the last `.` on is exactly `.mob` (`strrchr`, `strcmp`).
pub fn is_mob_path(path: &[u8]) -> bool {
    path.iter()
        .rposition(|&c| c == b'.')
        .is_some_and(|at| &path[at..] == b".mob")
}

/// `check`'s line for one file.
pub fn check_line(path: &[u8], result: FileCheck, line: i32, message: &[u8]) -> Vec<u8> {
    match result {
        FileCheck::Unreadable => format1(b"%s: Cannot read\n", path),
        FileCheck::ScriptError => [
            path,
            format!(": Script error: line {line}: ").as_bytes(),
            message,
            b"\r\n",
        ]
        .concat(),
        FileCheck::Ok => [path, b": ", message, b"\r\n"].concat(),
    }
}

/// `check`'s tally over its arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckTally {
    /// Files checked.
    pub files: u32,
    /// Arguments that named nothing to check.
    pub missing: u32,
    /// The code of the last file checked.
    pub last: i32,
    /// No file failed and no argument named nothing.
    pub ok: bool,
}

impl Default for CheckTally {
    fn default() -> Self {
        Self {
            files: 0,
            missing: 0,
            last: 0,
            ok: true,
        }
    }
}

impl CheckTally {
    /// Counts a checked file.
    pub fn file(&mut self, result: FileCheck) {
        self.files += 1;
        self.last = result.code();
        if result != FileCheck::Ok {
            self.ok = false;
        }
    }

    /// Counts an argument that named nothing to check.
    pub fn nothing(&mut self) {
        self.missing += 1;
        self.ok = false;
    }

    /// The exit code: one file and nothing missing gives that file's code; no file at all gives 3;
    /// otherwise 0 when everything passed, else 4.
    pub fn code(&self) -> i32 {
        if self.files == 1 && self.missing == 0 {
            self.last
        } else if self.files == 0 {
            3
        } else if self.ok {
            0
        } else {
            4
        }
    }
}

/// `aigraph`'s arguments, checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiGraphArgs {
    /// `multi`: the multiplayer databases (`databaselmp.res`); `single`: `database.res`.
    pub multi: bool,
    /// The mob file name, as given.
    pub mob: Vec<u8>,
    /// The map file name: as given, else derived from the mob ([`mpr_for`]).
    pub mpr: Vec<u8>,
}

/// Checks `aigraph`'s arguments; `Err` is what to print and the exit code.
pub fn aigraph_args(args: &[Vec<u8>], runner: &[u8]) -> Result<AiGraphArgs, (Vec<u8>, i32)> {
    if args.len() < 2 {
        return Err((aigraph_usage(runner), 0));
    }
    // CharLowerA, then an exact comparison.
    let mode = args[0].to_ascii_lowercase();
    let multi = match &mode[..] {
        b"multi" => true,
        b"single" => false,
        _ => {
            return Err((
                b"Invalid argument: expected 'multi' or 'single'\n".to_vec(),
                1,
            ));
        }
    };
    let mob = args[1].clone();
    let mpr = match args.get(2) {
        Some(m) => m.clone(),
        None => mpr_for(&mob),
    };
    Ok(AiGraphArgs { multi, mob, mpr })
}

/// The map file name for `mob`: without a `.mob` ending (any case), with its first `-lmp`
/// (any case) taken out, plus `.mpr`. `zone10-lmp.mob` gives `zone10.mpr`.
pub fn mpr_for(mob: &[u8]) -> Vec<u8> {
    let mut name = if mob.to_ascii_lowercase().ends_with(b".mob") {
        mob[..mob.len() - 4].to_vec()
    } else {
        mob.to_vec()
    };
    let lower = name.to_ascii_lowercase();
    if let Some(at) = lower.windows(4).position(|w| w == b"-lmp") {
        name.drain(at..at + 4);
    }
    name.extend_from_slice(b".mpr");
    name
}

/// The graph file for the full path of the mob's map file: a `.mob` ending (any case) becomes
/// `.grp`; any other name gets `.grp` added.
pub fn graph_file(full_path: &[u8]) -> Vec<u8> {
    let stem = if full_path.to_ascii_lowercase().ends_with(b".mob") {
        &full_path[..full_path.len() - 4]
    } else {
        full_path
    };
    [stem, b".grp"].concat()
}

/// A block's neighbours `(dx, dy)` in the order of its 8 edge words: game.exe's step tables 0x79110C and 0x791130,
/// which the addon's own block measure repeats.
pub const BLOCK_STEPS: [(i32, i32); 8] = [
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
];

/// The edge word for the way to a neighbour block: its cost / 128 (towards zero), at most
/// 0x7FFF; -1 when there is no way.
pub fn edge_word(cost: Option<i32>) -> i16 {
    match cost {
        // The original stores the low 16 bits of the quotient; a negative cost never comes from the search.
        Some(c) => (c / 128).min(0x7FFF) as i16,
        None => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<Vec<u8>> {
        line.split(' ').map(|a| a.as_bytes().to_vec()).collect()
    }

    fn early(line: &str) -> Parse {
        parse(&args(line), Phase::Early, DEFAULT_RUNNER)
    }

    fn exit_of(p: &Parse) -> Option<(String, i32)> {
        p.steps.iter().find_map(|s| match s {
            Step::Exit { text, code } => Some((String::from_utf8_lossy(text).into_owned(), *code)),
            Step::ModDir(_) | Step::GraphGen | Step::Check(_) | Step::AiGraph(_) => None,
        })
    }

    #[test]
    fn without_the_addon_word_nothing_happens() {
        assert_eq!(early("game.exe"), Parse::default());
        assert_eq!(early("game.exe run"), Parse::default());
    }

    #[test]
    fn run_goes_on_and_case_does_not_matter() {
        let p = early("game.exe ADDON Run");
        assert!(p.addon && !p.late && p.steps.is_empty());
    }

    #[test]
    fn help_and_version_exit_with_zero() {
        let (text, code) = exit_of(&early("game.exe addon help")).unwrap();
        assert_eq!(code, 0);
        assert!(text.starts_with("Usage:\n    game.exe addon {command} [arguments...]\n"));
        assert_eq!(exit_of(&early("game.exe addon --help")).unwrap().0, text);
        assert_eq!(
            exit_of(&early("game.exe addon version")).unwrap(),
            (
                "Evil Islands Addon Engine 0.12.4.2\nCopyright (C) 2001-2024 Gipat.Ru\n".to_owned(),
                0
            )
        );
    }

    #[test]
    fn errors_name_the_word_and_exit_with_one() {
        let try_help = "Try 'game.exe addon help' for more information\n";
        assert_eq!(
            exit_of(&early("game.exe addon bogus")).unwrap(),
            (format!("Invalid command: 'bogus'.\n{try_help}"), 1)
        );
        assert_eq!(
            exit_of(&early("game.exe addon")).unwrap(),
            (format!("Not enough arguments.\n{try_help}"), 1)
        );
        assert_eq!(
            exit_of(&early("game.exe addon --bogus run")).unwrap(),
            (format!("Invalid option: '--bogus'.\n{try_help}"), 1)
        );
        assert_eq!(
            exit_of(&early("game.exe addon --moddir")).unwrap(),
            (
                format!("Not enough parameters for option: '--moddir'.\n{try_help}"),
                1
            )
        );
        // The original's option loop never stops at a non-option word that follows the option's own argument.
        assert_eq!(
            exit_of(&early("game.exe addon --moddir x")).unwrap(),
            (format!("Not enough arguments.\n{try_help}"), 1)
        );
    }

    #[test]
    fn moddir_steps_come_before_a_later_error() {
        let p = early("game.exe addon --moddir inv --moddir two run");
        assert_eq!(
            p.steps,
            vec![Step::ModDir(b"inv".to_vec()), Step::ModDir(b"two".to_vec())]
        );
        let p = early("game.exe addon --moddir inv --x run");
        assert_eq!(p.steps[0], Step::ModDir(b"inv".to_vec()));
        assert_eq!(exit_of(&p).unwrap().1, 1);
    }

    #[test]
    fn the_late_commands_run_after_the_patches() {
        let p = early("game.exe addon check a b");
        assert!(p.addon && p.late && p.steps.is_empty());
        let late = parse(&args("game.exe addon check a b"), Phase::Late, b"r");
        assert_eq!(
            late.steps,
            vec![Step::Check(vec![b"a".to_vec(), b"b".to_vec()])]
        );
        let p = early("game.exe addon --moddir m aigraph single z.mob");
        assert_eq!(p.steps, vec![Step::ModDir(b"m".to_vec()), Step::GraphGen]);
        assert!(p.late);
        let late = parse(
            &args("game.exe addon --moddir m aigraph single z.mob"),
            Phase::Late,
            b"r",
        );
        assert_eq!(
            late.steps,
            vec![Step::AiGraph(vec![b"single".to_vec(), b"z.mob".to_vec()])]
        );
    }

    #[test]
    fn text_mode_doubles_carriage_returns() {
        assert_eq!(crt_text(b"a\nb\r\n"), b"a\r\nb\r\r\n");
    }

    #[test]
    fn check_tally_codes() {
        let mut one = CheckTally::default();
        one.file(FileCheck::Unreadable);
        assert_eq!(one.code(), 2);
        assert_eq!(CheckTally::default().code(), 3);
        let mut two = CheckTally::default();
        two.file(FileCheck::Ok);
        two.file(FileCheck::Ok);
        assert_eq!(two.code(), 0);
        two.nothing();
        assert_eq!(two.code(), 4);
        let mut missing = CheckTally::default();
        missing.file(FileCheck::Ok);
        missing.nothing();
        assert_eq!(missing.code(), 4);
    }

    #[test]
    fn check_lines() {
        assert!(is_mob_path(b"maps\\zone1.mob"));
        assert!(!is_mob_path(b"maps\\zone1.MOB"));
        assert!(!is_mob_path(b"a.mob\\b"));
        assert_eq!(
            check_line(b"x.txt", FileCheck::ScriptError, 3, b"boom"),
            b"x.txt: Script error: line 3: boom\r\n"
        );
        assert_eq!(
            check_line(b"x.txt", FileCheck::Unreadable, 0, b""),
            b"x.txt: Cannot read\n"
        );
    }

    #[test]
    fn aigraph_arguments() {
        let r = b"game.exe addon";
        assert_eq!(aigraph_args(&args("single"), r).unwrap_err().1, 0);
        assert_eq!(aigraph_args(&args("both z.mob"), r).unwrap_err().1, 1);
        let a = aigraph_args(&args("MULTI zone10-LMP.Mob"), r).unwrap();
        assert!(a.multi);
        assert_eq!(a.mpr, b"zone10.mpr");
        let a = aigraph_args(&args("single z.mob other.mpr"), r).unwrap();
        assert_eq!(a.mpr, b"other.mpr");
        assert_eq!(mpr_for(b"zone1"), b"zone1.mpr");
        assert_eq!(
            graph_file(b"C:\\G\\maps\\zone1.mob"),
            b"C:\\G\\maps\\zone1.grp"
        );
        assert_eq!(graph_file(b"C:\\G\\maps\\zone1"), b"C:\\G\\maps\\zone1.grp");
    }

    #[test]
    fn edge_words() {
        assert_eq!(edge_word(None), -1);
        assert_eq!(edge_word(Some(255)), 1);
        assert_eq!(edge_word(Some(0x7FFF * 128 + 500)), 0x7FFF);
        assert_eq!(edge_word(Some(i32::MAX)), 0x7FFF);
    }
}
