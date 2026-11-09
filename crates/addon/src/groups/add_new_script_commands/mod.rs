//! AddNewScriptCommands: 69 script functions and commands, the `?` and `+` signature letters, a stricter keyword
//! match, and script load errors in addon.log.
//!
//! The keyword table (0x78BCA0, up to 300 ids) is copied and extended with the addon's records: a record whose name
//! the game has (ConsoleString, Add, Mul, Random, ResetTarget, IsDead, IsAlive) keeps the game's id and takes the
//! record's signature; a new one gets id 300 + its record index. The parser's and the dumper's references to the
//! table point at the copy. The evaluators (`CalcFloat`, `CalcObject`, `CalcGroup`, `ScriptRun::Execute` and the
//! game's unused string evaluator 0x4B5650, which the rewritten `CalcParameters` calls for string arguments) look the
//! id up before their own switch; a handler that declines (`Random` with one argument) leaves the game's code to run.
#![expect(
    unsafe_code,
    reason = "reading script commands' arguments and writing their results in game memory"
)]

mod commands;
mod math;
mod text;
mod units;

use std::ffi::{CStr, c_void};
use std::sync::{Mutex, OnceLock};

use eix_core::script::{self, SignatureStep};
use eix_engine::game::mfc::{self, GameString};
use eix_engine::game::object::EngineObject;
use eix_engine::game::script::{
    self as game_script, SCRIPT_KEYWORDS, ScriptCommand, ScriptGroup, ScriptKeyword,
    ScriptObjectRef,
};
use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, Mode};
use eix_engine::patch;

use super::{BuildError, Labelled, Setup};
use crate::{log, report};

/// A handler: true when the call is done, false to let the game's code for the id run.
type Handler = fn(&Call) -> bool;

/// What a handler gets: the command and the evaluator's result slot (a `float*`, a `ScriptObjectRef**`, a
/// `ScriptGroup**` or a `CString*`; null for a command). The original also passes the running script, which none
/// of its handlers reads.
#[derive(Debug)]
struct Call {
    cmd: *mut ScriptCommand,
    result: *mut c_void,
}

/// The game ids below this are the game's.
const FIRST_NEW_ID: i32 = 300;

/// The records in the original's order: name, signature (a result letter first for a function), handler.
const RECORDS: [(&CStr, &CStr, Handler); 69] = [
    (c"GetPlayerOfUnit", c"Fo", units::get_player_of_unit),
    (c"GetUnitExp", c"Fo?s", units::get_unit_exp),
    (c"GetArmor", c"Foff", units::get_armor),
    (c"GetObjectStringID", c"So", text::get_object_string_id),
    (c"FloatToStr", c"Sf?f", text::float_to_str),
    (c"StrToFloat", c"Fs", text::str_to_float),
    (c"StrCat", c"Sss+", text::str_cat),
    (c"StrLen", c"Fs", text::str_len),
    (c"Sin", c"Ff", math::sin),
    (c"Cos", c"Ff", math::cos),
    (c"Pow", c"Fff", math::pow),
    (c"DistancePointPoint", c"Fff", math::distance_point_point),
    (c"NearestUnit", c"Offff", units::nearest_unit),
    (c"UnitsNearPoint", c"Gffff", units::units_near_point),
    (c"GSSetPlayerVar", c"fsf", commands::gs_set_player_var),
    (c"GSGetPlayerVar", c"Ffs", commands::gs_get_player_var),
    (c"GiveExp", c"ff", commands::give_exp),
    (c"GiveUnitExp", c"of", commands::give_unit_exp),
    (c"StartCameraSave", c"s", commands::start_camera_save),
    (c"StopCameraSave", c"", commands::stop_camera_save),
    (c"Say", c"s+", text::say),
    (c"ConsoleString", c"s+", text::console_string),
    (c"Add", c"Fff+", math::add),
    (c"Mul", c"Fff+", math::mul),
    (c"And", c"Fff+", math::and),
    (c"Or", c"Fff+", math::or),
    (c"Max", c"Fff+", math::max),
    (c"Min", c"Fff+", math::min),
    (c"Abs", c"Ff", math::abs),
    (c"Round", c"Ff", math::round),
    (c"Ceil", c"Ff", math::ceil),
    (c"Floor", c"Ff", math::floor),
    (c"Random", c"Ff?f", math::random),
    (c"RandStr", c"Sss+", text::rand_str),
    (c"RandObj", c"Og", units::rand_obj),
    (c"StrHas", c"Fss", text::str_has),
    (c"StrMid", c"Ssff", text::str_mid),
    (c"StrLower", c"Ss", text::str_lower),
    (c"StrUpper", c"Ss", text::str_upper),
    (c"ResetTarget", c"o", commands::reset_target),
    (c"SendCurrentParty", c"f", commands::send_current_party),
    (c"PlayerExists", c"Ff", commands::player_exists),
    (c"Loot", c"fo", commands::loot),
    (c"GameOver", c"", commands::game_over),
    (c"GameOverMSG", c"ss", commands::game_over_msg),
    (c"GetDexterity", c"Fo", units::get_dexterity),
    (c"GetIntelligence", c"Fo", units::get_intelligence),
    (c"GetStrength", c"Fo", units::get_strength),
    (c"GetSkill", c"Fos", units::get_skill),
    (c"GetActions", c"Fo", units::get_actions),
    (c"GetActionsBase", c"Fo", units::get_actions_base),
    (c"GetEncumbrance", c"Fo", units::get_encumbrance),
    (c"GetMaxEncumbrance", c"Fo", units::get_max_encumbrance),
    (c"GetPerk", c"Fos", units::get_perk),
    (c"GetPrototype", c"So", units::get_prototype),
    (c"GetRace", c"So", units::get_race),
    (c"IsFileExists", c"Fs", commands::is_file_exists),
    (c"GSListPlayerVars", c"f", commands::gs_list_player_vars),
    (c"GetObjectTypeID", c"Fo", units::get_object_type_id),
    (c"GetObjectsOnMap", c"Gf+", units::get_objects_on_map),
    (c"IsPlayer", c"Fo", units::is_player),
    (c"GiveUnitWeapon", c"osf", commands::give_unit_weapon),
    (c"GiveUnitArmor", c"osf", commands::give_unit_armor),
    (c"IsDead", c"Fo", units::is_dead),
    (c"IsAlive", c"Fo", units::is_alive),
    (c"ShowAllUnits", c"f", commands::show_all_units),
    (c"PauseGame", c"f", commands::pause_game),
    (c"SetWeather", c"s", commands::set_weather),
    (c"AddMobEx", c"s?f", commands::add_mob_ex),
];

/// The handler of each id, from the table's build.
static HANDLERS: OnceLock<Vec<Option<Handler>>> = OnceLock::new();
/// The mod folder, for `IsFileExists`.
static MOD_DIR: OnceLock<Vec<u8>> = OnceLock::new();

/// Runs the handler of `call`'s command id; false when there is none or it declines.
fn dispatch(call: &Call) -> bool {
    // SAFETY: the evaluators pass their live command.
    let id = unsafe { (*call.cmd).id };
    let handler = usize::try_from(id)
        .ok()
        .and_then(|i| HANDLERS.get()?.get(i).copied().flatten());
    handler.is_some_and(|h| h(call))
}

impl Call {
    /// The command.
    fn command(&self) -> &ScriptCommand {
        // SAFETY: the command is live for the call; its arguments are prepared.
        unsafe { &*self.cmd }
    }

    /// The float arguments.
    fn floats(&self) -> &[f32] {
        // SAFETY: a constructed array of the live command.
        unsafe { self.command().float_args.as_slice() }
    }

    /// Float argument `i`, 0 past the end.
    fn float(&self, i: usize) -> f32 {
        self.floats().get(i).copied().unwrap_or(0.0)
    }

    /// The string arguments' characters, each up to its NUL.
    fn strings(&self) -> Vec<&CStr> {
        // SAFETY: as for `floats`.
        let strings = unsafe { self.command().string_args.as_slice() };
        strings
            .iter()
            // SAFETY: each is a constructed game string.
            .map(|s| unsafe { CStr::from_ptr(s.chars.cast()) })
            .collect()
    }

    /// String argument `i`, empty past the end.
    fn string(&self, i: usize) -> &CStr {
        self.strings().get(i).copied().unwrap_or(c"")
    }

    /// String argument `i` with the length its header holds, empty past the end.
    fn string_bytes(&self, i: usize) -> &[u8] {
        // SAFETY: as for `floats`.
        let strings = unsafe { self.command().string_args.as_slice() };
        // SAFETY: a constructed game string.
        strings
            .get(i)
            .map(|s| unsafe { s.as_bytes() })
            .unwrap_or_default()
    }

    /// Object argument `i`: the variable or temporary the argument points at, null past the end.
    fn object_ref(&self, i: usize) -> *mut ScriptObjectRef {
        // SAFETY: as for `floats`.
        let refs = unsafe { self.command().object_args.as_slice() };
        refs.get(i).copied().unwrap_or(core::ptr::null_mut())
    }

    /// The object of object argument `i`, or null.
    fn object(&self, i: usize) -> *mut EngineObject {
        let r = self.object_ref(i);
        if r.is_null() {
            return core::ptr::null_mut();
        }
        // SAFETY: a bound argument points at a live reference.
        unsafe { (*r).object }
    }

    /// Group argument `i`, null past the end.
    fn group(&self, i: usize) -> *mut ScriptGroup {
        // SAFETY: as for `floats`.
        let groups = unsafe { self.command().group_args.as_slice() };
        groups.get(i).copied().unwrap_or(core::ptr::null_mut())
    }

    /// Writes a function's float result.
    fn set_float(&self, value: f32) {
        // SAFETY: a float function's evaluator (`CalcFloat`) passes its live `float*`.
        unsafe { self.result.cast::<f32>().write(value) };
    }

    /// Assigns a function's string result; the text ends at its first NUL, as the C string the original assigns.
    fn set_text(&self, text: &[u8]) {
        let text = crate::setup::c_string_until_nul(text);
        // SAFETY: a string function's evaluator passes its live, constructed `CString*`.
        unsafe { mfc::assign(self.result.cast::<GameString>(), &text) };
    }

    /// An object function's result: the temporary reference the evaluator made.
    fn result_ref(&self) -> *mut ScriptObjectRef {
        // SAFETY: `CalcObject` passes the slot holding its new temporary's address.
        unsafe { self.result.cast::<*mut ScriptObjectRef>().read() }
    }

    /// A group function's result: the temporary group the evaluator made.
    fn result_group(&self) -> *mut ScriptGroup {
        // SAFETY: `CalcGroup` passes the slot holding its new temporary's address.
        unsafe { self.result.cast::<*mut ScriptGroup>().read() }
    }
}

/// `<command>: Invalid argument(s) passed to function`.
fn invalid_arguments(command: &str) -> bool {
    report::script_error(command, b"Invalid argument(s) passed to function")
}

// --- The evaluators' hooks ---------------------------------------------------------------------------------------

/// The stack word at `address`, 0 when it is off the stack.
fn word_at(ctx: &Context, address: usize) -> usize {
    ctx.stack_at(address).unwrap_or(0) as usize
}

/// The hook's call for an evaluator: the command in ESI, the result slot at the given stack address.
fn evaluate(ctx: &mut Context, result: usize, done: usize) {
    let call = Call {
        cmd: ctx.esi as *mut ScriptCommand,
        result: word_at(ctx, result) as *mut c_void,
    };
    if dispatch(&call) {
        ctx.resume = done;
    }
}

/// `SSCCmd::CalcFloat` 0x4B41E0 (script, vars, `float*`): done at its epilogue.
extern "C" fn calc_float(_: usize, ctx: &mut Context) {
    let ebp = ctx.ebp as usize;
    evaluate(ctx, ebp + 0x10, eix_sites::SSC_CMD_CALC_FLOAT__12C3);
}

/// `SSCCmd::CalcObject` 0x4B56C0: the result slot already holds the new temporary.
extern "C" fn calc_object(_: usize, ctx: &mut Context) {
    let ebp = ctx.ebp as usize;
    evaluate(ctx, ebp + 0x10, eix_sites::SSC_CMD_CALC_OBJECT__1D5);
}

/// `SSCCmd::CalcGroup` 0x4B5B00 (no frame pointer: its result slot at the stack +0x54).
extern "C" fn calc_group(_: usize, ctx: &mut Context) {
    let esp = ctx.stack();
    evaluate(ctx, esp + 0x54, eix_sites::SSC_CMD_CALC_GROUP__338);
}

/// The string evaluator 0x4B5650, after its copied `push 0x78B748`: done past its `FatalError` call.
extern "C" fn calc_string(_: usize, ctx: &mut Context) {
    let esp = ctx.stack();
    evaluate(ctx, esp + 0x1C, eix_sites::SSC_CMD_CALC_FLOAT__14D7);
}

/// `ScriptRun::Execute` 0x4B6760: the command in ESI; done at the switch's end.
extern "C" fn execute(_: usize, ctx: &mut Context) {
    let call = Call {
        cmd: ctx.esi as *mut ScriptCommand,
        result: core::ptr::null_mut(),
    };
    if dispatch(&call) {
        ctx.resume = eix_sites::TEXT_4B9BC5;
    }
}

// --- The parser's hooks ------------------------------------------------------------------------------------------

/// `CScript::LoadFunction` 0x4CFBC0, after a separator token (its id in EAX, `)` is 0x6E): the signature letters
/// `?` and `+` at the signature position (the frame's +0x8C), for the command in EBP.
extern "C" fn signature(_: usize, ctx: &mut Context) {
    const POSITION: usize = 0x8C / 4;
    let Some(at) = ctx.stack_word(POSITION) else {
        return;
    };
    let at = at as *const u8;
    // SAFETY: the position points into the keyword's signature, a NUL-terminated static string.
    let rest = unsafe { CStr::from_ptr(at.cast()) }.to_bytes();
    // SAFETY: as above; a `+` is never a signature's first byte, so the byte before lies in it.
    let previous = (!rest.is_empty() && rest[0] == b'+').then(|| unsafe { *at.wrapping_sub(1) });
    let cmd = ctx.ebp as *mut ScriptCommand;
    let step = script::signature_step(rest, previous, ctx.eax == 0x6E);
    let new_position = match step {
        SignatureStep::None => return,
        SignatureStep::Skip => at.wrapping_add(1),
        SignatureStep::DropOptional(counts) => {
            // SAFETY: the parser's live command; the arrays are the game's.
            unsafe { drop_slots(cmd, counts) };
            at.wrapping_add(rest.len())
        }
        SignatureStep::Repeat(kind) => {
            // SAFETY: as above.
            unsafe { add_slot(cmd, kind) };
            at.wrapping_sub(1)
        }
    };
    ctx.set_stack_word(POSITION, new_position as u32);
}

/// Drops the optional parameters' slots.
///
/// # Safety
/// `cmd` must be the parser's live command, on the game's thread.
unsafe fn drop_slots(cmd: *mut ScriptCommand, counts: script::Counts) {
    // SAFETY: the caller guarantees the command; each array is a constructed game array.
    let c = unsafe { &mut *cmd };
    // The original sets negative lengths when fewer slots exist than optional letters; they stay at 0 here.
    if counts.floats != 0 {
        // SAFETY: as above.
        unsafe {
            game_script::resize_floats(&raw mut c.float_args, c.float_args.len - counts.floats)
        };
    }
    if counts.strings != 0 {
        // SAFETY: as above.
        unsafe {
            game_script::resize_strings(&raw mut c.string_args, c.string_args.len - counts.strings)
        };
    }
    if counts.objects != 0 {
        // SAFETY: as above.
        unsafe {
            game_script::resize_pointers(&raw mut c.object_args, c.object_args.len - counts.objects)
        };
    }
    if counts.groups != 0 {
        // SAFETY: as above.
        unsafe {
            game_script::resize_pointers(&raw mut c.group_args, c.group_args.len - counts.groups)
        };
    }
}

/// Adds one slot of the repeated type.
///
/// # Safety
/// As for [`drop_slots`].
unsafe fn add_slot(cmd: *mut ScriptCommand, kind: Option<script::ArgType>) {
    // SAFETY: the caller guarantees the command.
    let c = unsafe { &mut *cmd };
    match kind {
        // SAFETY: a constructed game array each.
        Some(script::ArgType::Float) => unsafe {
            game_script::resize_floats(&raw mut c.float_args, c.float_args.len + 1)
        },
        // SAFETY: as above.
        Some(script::ArgType::String) => unsafe {
            game_script::resize_strings(&raw mut c.string_args, c.string_args.len + 1)
        },
        // SAFETY: as above.
        Some(script::ArgType::Object) => unsafe {
            game_script::resize_pointers(&raw mut c.object_args, c.object_args.len + 1)
        },
        // SAFETY: as above.
        Some(script::ArgType::Group) => unsafe {
            game_script::resize_pointers(&raw mut c.group_args, c.group_args.len + 1)
        },
        None => {}
    }
}

/// `CScript::LoadCommands` 0x4D1330: For (0x6B) and ForIf (0x6C) load a body; every other keyword goes on to the
/// command case (0x4D17CB). The original also sends a record flagged as a block there, and no record is.
extern "C" fn block_keyword(_: usize, ctx: &mut Context) {
    ctx.resume = if ctx.eax == 0x6B || ctx.eax == 0x6C {
        eix_sites::C_SCRIPT_LOAD_COMMANDS__2B2
    } else {
        eix_sites::C_SCRIPT_LOAD_COMMANDS__49B
    };
}

/// `ClassifyToken` 0x4CF600: a keyword match is rejected (0x4CF717, past the best-match update the copied
/// instructions still make) when the keyword starts with a letter and the text goes on with a letter, a digit, `_`
/// or `#`. The entry's name is at EBP, the text at ESI, the keyword's length in EBX.
extern "C" fn keyword_match(_: usize, ctx: &mut Context) {
    let entry = ctx.ebp as *const ScriptKeyword;
    // SAFETY: EBP walks the keyword table, whose live entries have a name.
    let name = unsafe { (*entry).name };
    // SAFETY: a keyword name is a non-empty static C string.
    let first = unsafe { *name };
    // SAFETY: the text matched the keyword's `ebx` bytes, so its byte `ebx` is its NUL or the next character.
    let next = unsafe { *(ctx.esi as *const u8).wrapping_add(ctx.ebx as usize) };
    if script::rejects_keyword_match(first, next) {
        ctx.resume = eix_sites::CLASSIFY_TOKEN__117;
    }
}

/// The script text's lines, kept by the debug-level hook for the error's context.
static SOURCE_LINES: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());

/// `CScript::Load` 0x4D23A0 (only at addon.log's debug level): keeps the text's lines up to its last line end.
extern "C" fn keep_source(_: usize, ctx: &mut Context) {
    let Some(text) = ctx.stack_word(3) else {
        return;
    };
    let text = text as *const GameString;
    // SAFETY: Load's first argument is the script text, a constructed game string.
    let chars = unsafe { (*text).chars };
    // SAFETY: a constructed string's characters are NUL-terminated.
    let bytes = unsafe { CStr::from_ptr(chars.cast()) }.to_bytes();
    let mut lines: Vec<Vec<u8>> = bytes.split(|&c| c == b'\n').map(<[u8]>::to_vec).collect();
    // The text after the last line end is not a line, as in the original's split.
    lines.pop();
    *SOURCE_LINES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = lines;
}

/// `ScriptError` 0x4CF470: the load error goes to addon.log too, with its context when the text was kept.
extern "C" fn script_error(_: usize, ctx: &mut Context) {
    let line = ctx.eax as i32;
    // SAFETY: ECX points at ScriptError's formatted message, a NUL-terminated buffer on its stack.
    let message = unsafe { CStr::from_ptr(ctx.ecx as *const _) }.to_string_lossy();
    let lines = SOURCE_LINES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for entry in error_lines(line, &message, &lines) {
        log::error("scripts", &entry, "");
    }
}

/// The log entries for a script error at `line` with `message`, given the kept source `lines`.
fn error_lines(line: i32, message: &str, lines: &[Vec<u8>]) -> Vec<String> {
    let count = lines.len();
    let in_range = usize::try_from(line).is_ok_and(|l| l >= 1 && l <= count);
    let mut out = vec![format!(
        "Script error: {line}: \"{message}\".{}",
        if in_range { " Error context:" } else { "" }
    )];
    if in_range {
        // From two lines before to two after, as far as the text goes; a line below 3 shows none (the original's
        // unsigned start index).
        if let Ok(first) = usize::try_from(line - 3) {
            let end = usize::try_from(line + 2).unwrap_or(0).min(count);
            for (i, text) in lines.iter().enumerate().take(end).skip(first) {
                let number = i + 1;
                let mark = if number as i32 == line { "  <---" } else { "" };
                out.push(format!(
                    "{number:4}: {}{mark}",
                    String::from_utf8_lossy(text)
                ));
            }
        }
    }
    out
}

// --- Install -----------------------------------------------------------------------------------------------------

/// `SSCCmd::CalcParameters` 0x4B4030's string case (from +0x36, 115 bytes), which the original fills with a call of
/// the string evaluator 0x4B5650 for the argument's slot.
const CALC_PARAMETERS: usize = eix_sites::SSC_CMD_CALC_PARAMETERS__36;
const CALC_PARAMETERS_BYTES: [u8; 115] = [
    0x55, 0x8B, 0x6C, 0x24, 0x0C, 0x56, 0x8B, 0x70, 0x08, 0x57, 0x55, 0x8D, 0x46, 0x54, 0x8B, 0xCE,
    0x50, 0xE8, 0xE4, 0xFE, 0xFF, 0xFF, 0x8B, 0x4C, 0x24, 0x10, 0x8D, 0x56, 0x68, 0x81, 0xC1, 0xA0,
    0x00, 0x00, 0x00, 0x51, 0x52, 0x8B, 0xCE, 0xE8, 0xCE, 0xFE, 0xFF, 0xFF, 0x8B, 0x86, 0x84, 0x00,
    0x00, 0x00, 0x33, 0xFF, 0x85, 0xC0, 0x7E, 0x28, 0x53, 0x33, 0xDB, 0x8B, 0x86, 0x80, 0x00, 0x00,
    0x00, 0x8B, 0x4C, 0x24, 0x14, 0x03, 0xC3, 0x50, 0x55, 0x51, 0x8B, 0xCE, 0xE8, 0x79, 0xFF, 0xFF,
    0xFF, 0x8B, 0x86, 0x84, 0x00, 0x00, 0x00, 0x47, 0x83, 0xC3, 0x0C, 0x3B, 0xF8, 0x7C, 0xDC, 0x5B,
    0x68, 0x48, 0xB7, 0x78, 0x00, 0xE8, 0xB0, 0xBF, 0xFC, 0xFF, 0x83, 0xC4, 0x04, 0x5F, 0x5E, 0x5D,
    0xC2, 0x0C, 0x00,
];
/// The string evaluator the rewritten case calls.
const CALC_STRING: usize = eix_sites::SSC_CMD_CALC_FLOAT__1470;

/// The rewritten case: `mov edx, [eax+4]; mov ecx, [ecx+0x1C]; lea edx, [ecx+edx*4]` (the argument's string slot),
/// `mov ecx, [esp+8]; push edx; mov edx, [esp+8]; push ecx; mov ecx, [eax+8]; push edx` (vars, script, the nested
/// call's command), `call 0x4B5650; ret 0xC`, then NOPs.
fn calc_parameters_code() -> [u8; 115] {
    const HEAD: [u8; 23] = [
        0x8B, 0x50, 0x04, 0x8B, 0x49, 0x1C, 0x8D, 0x14, 0x91, 0x8B, 0x4C, 0x24, 0x08, 0x52, 0x8B,
        0x54, 0x24, 0x08, 0x51, 0x8B, 0x48, 0x08, 0x52,
    ];
    let mut code = [0x90u8; 115];
    code[..23].copy_from_slice(&HEAD);
    code[23..28].copy_from_slice(&patch::call_rel32(CALC_PARAMETERS + 23, CALC_STRING));
    code[28..31].copy_from_slice(&[0xC2, 0x0C, 0x00]);
    code
}

/// The table's references in the parser (and the script dumper): each operand holds 0x78BCA0.
const TABLE_REFERENCES: [usize; 15] = [
    eix_sites::CLASSIFY_TOKEN__9B,
    eix_sites::CLASSIFY_TOKEN__B4,
    eix_sites::C_SCRIPT_LOAD_FUNCTION__658,
    eix_sites::C_SCRIPT_LOAD_FUNCTION__65D,
    eix_sites::C_SCRIPT_LOAD_FUNCTION__C45,
    eix_sites::C_SCRIPT_LOAD_FUNCTION__C4A,
    eix_sites::FUNCTION_SIGNATURE__1,
    eix_sites::FUNCTION_SIGNATURE__C,
    eix_sites::KEYWORD_NAME__2,
    eix_sites::KEYWORD_NAME__7,
    eix_sites::C_SCRIPT_LOAD_COMMANDS__19B,
    eix_sites::C_SCRIPT_LOAD_COMMANDS__1A0,
    eix_sites::C_SCRIPT_QUALIFY_NAME__A14,
    eix_sites::C_SCRIPT_QUALIFY_NAME__A22,
    eix_sites::C_SCRIPT_QUALIFY_NAME__E1C,
];

/// The hooks after the table: label number, site, replaced bytes, mode, handler.
type HookSite = (usize, usize, &'static [u8], Mode, hook::Handler);
const HOOKS: [HookSite; 8] = [
    (
        1,
        eix_sites::SSC_CMD_CALC_FLOAT__7A,
        &[0x8B, 0x06, 0x83, 0xC0, 0xBB],
        Mode::After,
        calc_float,
    ),
    (
        2,
        eix_sites::SSC_CMD_CALC_GROUP__AF,
        &[0x8B, 0x06, 0x83, 0xC0, 0xA7],
        Mode::After,
        calc_group,
    ),
    (
        3,
        eix_sites::SSC_CMD_CALC_OBJECT__6F,
        &[0x8B, 0x06, 0x83, 0xC0, 0xB2],
        Mode::After,
        calc_object,
    ),
    (
        4,
        eix_sites::SSC_CMD_CALC_FLOAT__14CD,
        &[0x68, 0x48, 0xB7, 0x78, 0x00],
        Mode::Before,
        calc_string,
    ),
    (
        5,
        eix_sites::SCRIPT_RUN_EXECUTE__EE,
        &[0x8B, 0x06, 0xC7, 0x45, 0xD4, 0x01, 0x00, 0x00, 0x00],
        Mode::Before,
        execute,
    ),
    (
        6,
        eix_sites::C_SCRIPT_LOAD_FUNCTION__70C,
        &[0x8B, 0x44, 0x24, 0x1C, 0x83, 0xF8, 0x6E],
        Mode::Before,
        signature,
    ),
    (
        7,
        eix_sites::C_SCRIPT_LOAD_COMMANDS__2A4,
        &[0x83, 0xF8, 0x6B, 0x74, 0x09],
        Mode::Drop,
        block_keyword,
    ),
    (
        8,
        eix_sites::CLASSIFY_TOKEN__F0,
        &[0x8B, 0x54, 0x24, 0x1C, 0x89, 0x5C, 0x24, 0x10],
        Mode::After,
        keyword_match,
    ),
];
/// `ScriptError`'s `lea ecx, [esi+0x1EC]`, after the message is formatted.
const SCRIPT_ERROR: (usize, [u8; 6]) = (
    eix_sites::SCRIPT_ERROR__32,
    [0x8D, 0x8E, 0xEC, 0x01, 0x00, 0x00],
);
/// `CScript::Load`'s `mov eax, fs:[0]`, at its start.
const LOAD: (usize, [u8; 6]) = (
    eix_sites::C_SCRIPT_LOAD__7,
    [0x64, 0xA1, 0x00, 0x00, 0x00, 0x00],
);

/// The game's keywords, then the records, as the original builds the table; and each id's handler.
///
/// # Safety
/// The process must be game.exe RU 1.06 before WinMain: the game's keyword table is read in place.
unsafe fn build_table() -> Result<(Vec<ScriptKeyword>, Vec<Option<Handler>>), String> {
    let mut table = Vec::with_capacity(300);
    let mut at = SCRIPT_KEYWORDS.as_ptr();
    loop {
        // SAFETY: the game's table is terminated by a null name; `at` stays within it.
        let k = unsafe { at.read() };
        if k.name.is_null() {
            break;
        }
        table.push(k);
        at = at.wrapping_add(1);
    }
    let mut handlers: Vec<Option<Handler>> = vec![None; FIRST_NEW_ID as usize + RECORDS.len()];
    for (index, (name, signature, handler)) in RECORDS.iter().enumerate() {
        let function = script::is_function_signature(signature.to_bytes());
        let existing = table.iter_mut().find(|k| {
            // SAFETY: every entry's name is a NUL-terminated static string.
            script::same_name(
                unsafe { CStr::from_ptr(k.name.cast()) }.to_bytes(),
                name.to_bytes(),
            )
        });
        let id = match existing {
            Some(k) if k.id < FIRST_NEW_ID => {
                k.function = core::ptr::null();
                k.command = core::ptr::null();
                if function {
                    k.function = signature.as_ptr().cast();
                } else {
                    k.command = signature.as_ptr().cast();
                }
                k.id
            }
            Some(_) => return Err(format!("{} is registered twice", name.to_string_lossy())),
            None => {
                let id = FIRST_NEW_ID + index as i32;
                table.push(ScriptKeyword {
                    name: name.as_ptr().cast(),
                    id,
                    function: if function {
                        signature.as_ptr().cast()
                    } else {
                        core::ptr::null()
                    },
                    command: if function {
                        core::ptr::null()
                    } else {
                        signature.as_ptr().cast()
                    },
                });
                id
            }
        };
        if let Some(slot) = usize::try_from(id).ok().and_then(|i| handlers.get_mut(i)) {
            *slot = Some(*handler);
        }
    }
    table.push(ScriptKeyword {
        name: core::ptr::null(),
        id: 0,
        function: core::ptr::null(),
        command: core::ptr::null(),
    });
    Ok((table, handlers))
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    report::init(&setup.options);
    let _ = MOD_DIR.set(setup.mod_dir.clone());
    // SAFETY: groups are built before WinMain, in RU 1.06 (checked before the WinMain hook).
    let (table, handlers) = unsafe { build_table() }?;
    let table: &'static [ScriptKeyword] = Box::leak(table.into_boxed_slice());
    let address = table.as_ptr() as usize;
    let _ = HANDLERS.set(handlers);
    let label = |n: usize| format!("AddNewScriptCommands #{n}");
    let mut out = vec![Labelled {
        label: label(0),
        write: SiteWrite::bytes(
            CALC_PARAMETERS,
            &CALC_PARAMETERS_BYTES,
            &calc_parameters_code(),
        )?,
    }];
    for (n, site, original, mode, handler) in HOOKS {
        out.push(Labelled {
            label: label(n),
            write: hook::context_hook(site, original, mode, handler, 0)?,
        });
    }
    for (i, site) in TABLE_REFERENCES.iter().enumerate() {
        out.push(Labelled {
            label: label(9 + i),
            write: SiteWrite::pointer(*site, SCRIPT_KEYWORDS.address(), address),
        });
    }
    out.push(Labelled {
        label: label(24),
        write: hook::context_hook(
            SCRIPT_ERROR.0,
            &SCRIPT_ERROR.1,
            Mode::After,
            script_error,
            0,
        )?,
    });
    // The original keeps the source lines only when its addon.log is at the debug level.
    if log::threshold() <= 0 {
        out.push(Labelled {
            label: label(25),
            write: hook::context_hook(LOAD.0, &LOAD.1, Mode::After, keep_source, 0)?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_record_passes_the_originals_check() {
        for (name, signature, _) in RECORDS {
            assert!(
                script::signature_is_valid(signature.to_bytes()),
                "{}",
                name.to_string_lossy()
            );
        }
    }

    /// The script reference documents each record under its name and signature, and nothing else as the addon's.
    #[test]
    fn every_record_is_documented_with_its_signature() {
        use eix_docs::scripting::{Scripting, Source};
        let data = Scripting::parse(eix_docs::SCRIPTING_DATA).expect("docs/scripting.yml is valid");
        let documented: Vec<_> = data
            .entries
            .iter()
            .filter(|e| e.source == Source::Addon)
            .collect();
        for (name, signature, _) in RECORDS {
            let name = name.to_str().expect("record names are ASCII");
            let entry = documented
                .iter()
                .find(|e| e.name == name)
                .unwrap_or_else(|| panic!("{name} is not in docs/scripting.yml"));
            assert_eq!(
                entry.signature(),
                signature.to_str().expect("signatures are ASCII"),
                "{name}"
            );
        }
        assert_eq!(documented.len(), RECORDS.len());
    }

    #[test]
    fn the_rewritten_case_calls_the_string_evaluator() {
        let code = calc_parameters_code();
        assert_eq!(code[23], 0xE8);
        let rel = i32::from_le_bytes([code[24], code[25], code[26], code[27]]);
        assert_eq!(
            (CALC_PARAMETERS as i64 + 28 + i64::from(rel)) as usize,
            CALC_STRING
        );
        assert_eq!(&code[28..31], &[0xC2, 0x0C, 0x00]);
        assert!(code[31..].iter().all(|&b| b == 0x90));
    }

    /// The script error a multiplayer game logs at its start, which the original logs as the same single line
    /// (addon.log's default level keeps no source lines).
    #[test]
    fn the_multiplayer_start_script_error_is_one_line() {
        assert_eq!(
            error_lines(1, "Parameter of type group expected", &[]),
            vec!["Script error: 1: \"Parameter of type group expected\".".to_owned()]
        );
        // `Add` (a record) does not match inside the game's `AddObject`, only before a separator.
        assert!(script::rejects_keyword_match(b'A', b'O'));
        assert!(!script::rejects_keyword_match(b'A', b'('));
    }

    #[test]
    fn errors_show_their_context_from_line_3() {
        let lines: Vec<Vec<u8>> = (1..=6).map(|i| format!("l{i}").into_bytes()).collect();
        assert_eq!(
            error_lines(9, "m", &lines),
            vec!["Script error: 9: \"m\".".to_owned()]
        );
        assert_eq!(error_lines(2, "m", &lines).len(), 1);
        assert_eq!(
            error_lines(4, "m", &lines),
            vec![
                "Script error: 4: \"m\". Error context:".to_owned(),
                "   2: l2".to_owned(),
                "   3: l3".to_owned(),
                "   4: l4  <---".to_owned(),
                "   5: l5".to_owned(),
                "   6: l6".to_owned(),
            ]
        );
    }
}
