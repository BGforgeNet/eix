//! TruncatedScript: a script file that ends in the middle of a construct gets the game's own "Unexpected end of file"
//! script error, where the parser read through a null token cursor (a crash, or a hang under the original addon's
//! exception path). The parser walks the token list with a cursor that becomes null after the last token; its loop
//! heads and most cases test it, but eight reads that take a further token do not: three in
//! `CScript::LoadScriptBody` 0x4D1030 (after `(`, after the condition block, after `then`), one in
//! `CScript::LoadFunction` 0x4CFBC0 (a command's arguments), two in `CScript::LoadVariableDeclarations` 0x4D1FE0 and
//! two cases of `CScript::Load` 0x4D23A0's token switch (the `Script` header and its sibling at 0x4D2A42; the other
//! two cases test). Ours tests the cursor there and goes where the function's own test would: its "Unexpected end of
//! file" report, or for `Load`, which has none for these cases, the report made here and the exit `Load` takes after
//! one. Each target holds the same frame and the same live locals as its site.
#![expect(
    unsafe_code,
    reason = "reads the parser's token cursor and list nodes and its line record, and calls the game's script error report"
)]

use core::ffi::{c_char, c_void};

use eix_engine::hook::{self, Context, Mode};

use super::Warnings;
use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// `mov eax, [esi]; lea ecx, [esp+0x18]`, run before the handler (harmless): EAX is then the cursor.
const BODY_BYTES: [u8; 6] = [0x8B, 0x06, 0x8D, 0x4C, 0x24, 0x18];
/// `LoadScriptBody`'s reads after `(`, after the condition block and after `then`, and its report.
const BODY: [usize; 3] = [
    eix_sites::C_SCRIPT_LOAD_SCRIPT_BODY__BA,
    eix_sites::C_SCRIPT_LOAD_SCRIPT_BODY__124,
    eix_sites::C_SCRIPT_LOAD_SCRIPT_BODY__166,
];
const BODY_END: usize = eix_sites::C_SCRIPT_LOAD_SCRIPT_BODY__1FA;

/// `mov ecx, [esp+0x80]`, run before the handler (harmless), before `GetNext` on the cursor EDI points at; the report
/// at the loop's end.
const FUNCTION: usize = eix_sites::C_SCRIPT_LOAD_FUNCTION__735;
const FUNCTION_BYTES: [u8; 7] = [0x8B, 0x8C, 0x24, 0x80, 0x00, 0x00, 0x00];
const FUNCTION_END: usize = eix_sites::C_SCRIPT_LOAD_FUNCTION__B44;

/// `mov eax, [edi]; mov ecx, [eax]; lea esi, [eax+8]`, which the handler does instead (the second read is the
/// fault): EDI points at the cursor.
const DECLARATION_BYTES: [u8; 7] = [0x8B, 0x07, 0x8B, 0x08, 0x8D, 0x70, 0x08];
/// The read after a declaration's `,`, and the null test's report that follows it.
const DECLARATION: usize = eix_sites::C_SCRIPT_LOAD_VARIABLE_DECLARATIONS__CE;
const DECLARATION_END: usize = eix_sites::C_SCRIPT_LOAD_VARIABLE_DECLARATIONS__100;
/// The read after that report when it returns non-zero (SpellAddon's `scriptchecking` makes it), and the function's
/// failure exit: a second report there could loop.
const AFTER_REPORT: usize = eix_sites::C_SCRIPT_LOAD_VARIABLE_DECLARATIONS__159;
const DECLARATIONS_EXIT: usize = eix_sites::C_SCRIPT_LOAD_VARIABLE_DECLARATIONS__324;

/// `Load`'s `Script` case entry, `lea ecx, [esp+0x88]`, run before the handler (harmless); the case then takes a
/// token with `GetNext`.
const SCRIPT_HEADER: usize = eix_sites::C_SCRIPT_LOAD__7C2;
const SCRIPT_HEADER_BYTES: [u8; 7] = [0x8D, 0x8C, 0x24, 0x88, 0x00, 0x00, 0x00];
/// Its sibling case: `mov eax, [esp+0x18]; mov ecx, [eax]`, which the handler does instead (the second is the fault).
const SIBLING: usize = eix_sites::C_SCRIPT_LOAD__6E8;
const SIBLING_BYTES: [u8; 6] = [0x8B, 0x44, 0x24, 0x18, 0x8B, 0x08];
/// `Load`'s exit after a report that returned 0, at the switch's locals.
const LOAD_EXIT: usize = eix_sites::C_SCRIPT_LOAD__9C4;
/// In `Load`'s frame: the cursor, and the record whose +0x0C is the current line (as its own report reads it,
/// 0x4D27CB).
const LOAD_CURSOR_WORD: usize = 0x18 / 4;
const LOAD_LINE_RECORD_WORD: usize = 0x34 / 4;
const LINE: usize = 0x0C;
/// A token list node: the next node's address, then (after the previous one's) the token at +8.
const NODE_TOKEN: u32 = 8;

/// `ScriptError(script, line, level, format, ...)`, cdecl; level 1 is an error.
type ScriptError = unsafe extern "C" fn(*mut c_void, u32, u32, *const c_char) -> u32;
const ERROR_LEVEL: u32 = 1;

static WARNINGS: Warnings = Warnings::new("TruncatedScript");

fn note(site: &str) {
    WARNINGS
        .warn(|| format!("a script ends early ({site}): reported as \"Unexpected end of file\""));
}

/// The word at `at`: the parser's cursor, or a list node's next pointer.
///
/// # Safety
/// `at` must be the cursor's address the parser itself reads, or a live node.
unsafe fn word(at: u32) -> u32 {
    // SAFETY: the caller guarantees the word.
    unsafe { core::ptr::with_exposed_provenance::<u32>(at as usize).read_unaligned() }
}

/// The token read the handler does in place of the game's: EAX the node, ECX its next node, `token` (a register)
/// the node's token.
///
/// # Safety
/// `node` must be a live token list node.
unsafe fn take(ctx: &mut Context, node: u32) -> u32 {
    ctx.eax = node;
    // SAFETY: the caller guarantees the node.
    ctx.ecx = unsafe { word(node) };
    node + NODE_TOKEN
}

extern "C" fn body(_: usize, ctx: &mut Context) {
    if ctx.eax == 0 {
        note("a command or condition");
        ctx.resume = BODY_END;
    }
}

extern "C" fn function(_: usize, ctx: &mut Context) {
    // SAFETY: EDI points at the cursor `LoadFunction` reads next.
    if unsafe { word(ctx.edi) } == 0 {
        note("a command's arguments");
        ctx.resume = FUNCTION_END;
    }
}

extern "C" fn declaration(end: usize, ctx: &mut Context) {
    // SAFETY: EDI points at the cursor `LoadVariableDeclarations` reads next.
    let node = unsafe { word(ctx.edi) };
    if node == 0 {
        note("a variable declaration");
        ctx.resume = end;
        return;
    }
    // SAFETY: a non-null cursor is a live node.
    ctx.esi = unsafe { take(ctx, node) };
}

/// `Load`'s report for a script that ends in one of the two cases, and its exit.
fn load_ended(ctx: &mut Context) {
    note("a script header");
    let line = ctx.stack_word(LOAD_LINE_RECORD_WORD).map_or(0, |record| {
        // SAFETY: the record `Load`'s own report reads its line from, live for the whole parse.
        unsafe { word(record + LINE as u32) }
    });
    // SAFETY: SCRIPT_ERROR is the game's `ScriptError`, of this type.
    let report = unsafe { hook::as_fn::<ScriptError>(eix_sites::SCRIPT_ERROR) };
    let script = core::ptr::with_exposed_provenance_mut::<c_void>(ctx.esi as usize);
    let format = core::ptr::with_exposed_provenance::<c_char>(eix_sites::here(eix_sites::DATA_78D740));
    // SAFETY: ESI is the script `Load` parses; the format is the game's "Unexpected end of file", which takes no
    // argument; the call is made on the parsing thread, as `Load`'s own.
    unsafe { report(script, line, ERROR_LEVEL, format) };
    ctx.resume = LOAD_EXIT;
}

extern "C" fn header(_: usize, ctx: &mut Context) {
    if ctx.stack_word(LOAD_CURSOR_WORD) == Some(0) {
        load_ended(ctx);
    }
}

extern "C" fn sibling(_: usize, ctx: &mut Context) {
    let node = ctx.stack_word(LOAD_CURSOR_WORD).unwrap_or(0);
    if node == 0 {
        load_ended(ctx);
        return;
    }
    // SAFETY: a non-null cursor is a live node.
    let _ = unsafe { take(ctx, node) };
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let mut writes = Vec::new();
    for (i, site) in BODY.into_iter().enumerate() {
        writes.push(Labelled {
            label: format!("TruncatedScript body #{i}"),
            write: hook::context_hook(site, &BODY_BYTES, Mode::Before, body, 0)?,
        });
    }
    writes.push(Labelled {
        label: "TruncatedScript function".to_owned(),
        write: hook::context_hook(FUNCTION, &FUNCTION_BYTES, Mode::Before, function, 0)?,
    });
    for (i, (site, end)) in [
        (DECLARATION, DECLARATION_END),
        (AFTER_REPORT, DECLARATIONS_EXIT),
    ]
    .into_iter()
    .enumerate()
    {
        writes.push(Labelled {
            label: format!("TruncatedScript declaration #{i}"),
            write: hook::context_hook(site, &DECLARATION_BYTES, Mode::Drop, declaration, end)?,
        });
    }
    writes.push(Labelled {
        label: "TruncatedScript script header".to_owned(),
        write: hook::context_hook(SCRIPT_HEADER, &SCRIPT_HEADER_BYTES, Mode::Before, header, 0)?,
    });
    writes.push(Labelled {
        label: "TruncatedScript sibling case".to_owned(),
        write: hook::context_hook(SIBLING, &SIBLING_BYTES, Mode::Drop, sibling, 0)?,
    });
    Ok(writes)
}
