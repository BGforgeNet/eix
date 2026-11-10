//! Hooks on game.exe code: context hooks (entered by a jump or a call), FPU hooks (context hooks that also keep the
//! game's x87 state), call hooks, function replacement, call redirection.
//!
//! Each builder only prepares: it places its stub or trampoline in the [`exec`] pool and returns the
//! [`SiteWrite`] that installs it, which a patch group applies after checking every site of the group.

// Hand-rolled, not a hooking crate such as ilhook: that one writes its jump when the hook is made, with no check of
// the bytes it replaces and no all-or-nothing group, and it needs std, which this crate does without for the loader.
#![expect(
    unsafe_code,
    reason = "naked entry stubs that save and restore the game's registers"
)]

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::mem::offset_of;

use crate::exec;
use crate::group::SiteWrite;
use crate::patch::{self, PatchError};
use crate::x86::{self, CodeError};
use crate::x87::FpuSave;

/// The game's registers at a context hook, as the stub saved them: PUSHAD's frame, then PUSHFD's, then the resume
/// address. A handler may change any field but `esp`; execution goes on at `resume` with the registers restored.
///
/// Only a stub makes one, on the game's stack right below the stack at the hook, which is what lets
/// [`Context::stack_word`] reach that stack; so it can be neither built outside the engine nor copied.
#[repr(C)]
#[derive(Debug)]
#[non_exhaustive]
pub struct Context {
    /// EDI.
    pub edi: u32,
    /// ESI.
    pub esi: u32,
    /// EBP.
    pub ebp: u32,
    /// PUSHAD's copy of ESP, which points at `eflags`; POPAD skips it, so writing it has no effect.
    pub esp: u32,
    /// EBX.
    pub ebx: u32,
    /// EDX.
    pub edx: u32,
    /// ECX.
    pub ecx: u32,
    /// EAX.
    pub eax: u32,
    /// EFLAGS.
    pub eflags: u32,
    /// Where execution continues: by default the first byte after the hooked instructions.
    pub resume: usize,
}

const _: () = {
    assert!(offset_of!(Context, edi) == 0x00);
    assert!(offset_of!(Context, esi) == 0x04);
    assert!(offset_of!(Context, ebp) == 0x08);
    assert!(offset_of!(Context, esp) == 0x0C);
    assert!(offset_of!(Context, ebx) == 0x10);
    assert!(offset_of!(Context, edx) == 0x14);
    assert!(offset_of!(Context, ecx) == 0x18);
    assert!(offset_of!(Context, eax) == 0x1C);
    assert!(offset_of!(Context, eflags) == 0x20);
    assert!(offset_of!(Context, resume) == 0x24);
    assert!(size_of::<Context>() == 0x28);
};

impl Context {
    /// The game's ESP at the hook site: its stack, with the first argument or local at the address returned.
    pub fn stack(&self) -> usize {
        // The stub's frame ends where the stack at the site begins; unlike `esp`, the handler cannot change that.
        self as *const Self as usize + size_of::<Self>()
    }

    /// The 4-byte word at `address`, when it lies on this thread's stack at or above the site's ESP.
    pub fn stack_at(&self, address: usize) -> Option<u32> {
        let at = self.on_stack(address)?;
        // SAFETY: `at` is a 4-byte range of this thread's stack between the site's ESP and the stack base.
        Some(unsafe { core::ptr::read_unaligned(at as *const u32) })
    }

    /// Word `index` of the stack at the site (0 is the word at ESP).
    pub fn stack_word(&self, index: usize) -> Option<u32> {
        self.stack_at(self.stack().checked_add(index.checked_mul(4)?)?)
    }

    /// Replaces word `index` of the stack at the site; false when it lies outside the thread's stack.
    pub fn set_stack_word(&mut self, index: usize, value: u32) -> bool {
        match index
            .checked_mul(4)
            .and_then(|o| self.stack().checked_add(o))
        {
            Some(address) => self.set_stack_at(address, value),
            None => false,
        }
    }

    /// Replaces the 4-byte word at `address` (a local of the game's frame, an argument); false unless it lies on this
    /// thread's stack at or above the site's ESP.
    pub fn set_stack_at(&mut self, address: usize, value: u32) -> bool {
        let Some(at) = self.on_stack(address) else {
            return false;
        };
        // SAFETY: as in `stack_at`; the word is the game's, which the hook is allowed to change.
        unsafe { core::ptr::write_unaligned(at as *mut u32, value) };
        true
    }

    fn on_stack(&self, address: usize) -> Option<usize> {
        let end = address.checked_add(4)?;
        (address >= self.stack() && end <= stack_base()).then_some(address)
    }
}

/// The game's state at an FPU hook: its x87 state as FNSAVE stored it, then its registers as at a context hook.
///
/// The handler may change the x87 registers through [`FpuSave`] (push, pop, replace) and the integer registers and
/// `resume` through `regs`; the stub restores both. Like [`Context`] only a stub makes one, on the game's stack.
#[repr(C)]
#[derive(Debug)]
#[non_exhaustive]
pub struct FpuContext {
    /// The x87 state.
    pub fpu: FpuSave,
    /// The integer registers, the flags and the resume address.
    pub regs: Context,
}

const _: () = {
    assert!(offset_of!(FpuContext, fpu) == 0);
    assert!(offset_of!(FpuContext, regs) == 108);
    assert!(size_of::<FpuContext>() == 108 + 0x28);
};

/// The calling thread's stack base (its highest address, exclusive), from the thread information block.
fn stack_base() -> usize {
    let base: usize;
    // SAFETY: on 32-bit Windows FS addresses the thread information block, whose word at 4 is the stack base.
    unsafe {
        core::arch::asm!("mov {}, fs:[4]", out(reg) base, options(nostack, readonly, preserves_flags));
    }
    base
}

/// A context hook's handler: the `arg` given at install, and the saved registers.
pub type Handler = extern "C" fn(arg: usize, ctx: &mut Context);

/// An FPU hook's handler: the `arg` given at install, and the saved x87 state and registers. It runs with an empty
/// x87 stack under the game's control word, so [`crate::x87::Ext`]'s arithmetic rounds as the game's code does.
pub type FpuHandler = extern "C" fn(arg: usize, ctx: &mut FpuContext);

/// What a context hook does with the instructions its jump replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// They never run (the original's mode 0).
    Drop,
    /// They run before the handler (mode 1).
    Before,
    /// They run after the handler, then execution goes to the handler's `resume` (mode 2).
    ///
    /// While they run, the chosen resume address sits in the stub, not on the stack, so the hook must not run on two
    /// threads at once, nor re-enter itself through the replaced instructions with a different resume address.
    After,
}

/// Why a hook could not be prepared or a patch group installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookError {
    /// A jump needs 5 bytes; the site has fewer.
    TooShort {
        /// The site.
        site: usize,
        /// Its length.
        len: usize,
    },
    /// A call redirection's site is not a `call rel32`.
    NotCall {
        /// The site.
        site: usize,
        /// Its first byte.
        found: u8,
    },
    /// The replaced instructions cannot be copied.
    Code {
        /// The site.
        site: usize,
        /// What stops the copy.
        error: CodeError,
    },
    /// The instructions a hook covers do not start with the bytes it writes over.
    NotCovering {
        /// The site.
        site: usize,
    },
    /// No executable memory for the stub.
    Memory(String),
    /// A site write failed.
    Patch(PatchError),
}

impl fmt::Display for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { site, len } => {
                write!(f, "hook at {site:#x}: {len} bytes, a jump needs 5")
            }
            Self::NotCall { site, found } => {
                write!(
                    f,
                    "hook at {site:#x}: expected a call (E8), found {found:02X}"
                )
            }
            Self::Code { site, error } => write!(f, "hook at {site:#x}: {error}"),
            Self::NotCovering { site } => write!(
                f,
                "hook at {site:#x}: the covered instructions do not start with the written bytes"
            ),
            Self::Memory(e) => write!(f, "no memory for a hook stub: {e}"),
            Self::Patch(e) => e.fmt(f),
        }
    }
}

impl core::error::Error for HookError {}

impl From<String> for HookError {
    fn from(e: String) -> Self {
        Self::Memory(e)
    }
}

impl From<PatchError> for HookError {
    fn from(e: PatchError) -> Self {
        Self::Patch(e)
    }
}

fn check_len(site: usize, original: &[u8]) -> Result<(), HookError> {
    if original.len() < 5 {
        return Err(HookError::TooShort {
            site,
            len: original.len(),
        });
    }
    Ok(())
}

fn relocate(site: usize, original: &[u8], to: usize) -> Result<Vec<u8>, HookError> {
    x86::relocate(original, site, to).map_err(|error| HookError::Code { site, error })
}

fn push(code: &mut Vec<u8>, value: usize) {
    code.push(0x68);
    code.extend_from_slice(&(value as u32).to_le_bytes());
}

/// Called by a context stub for modes [`Mode::Drop`] and [`Mode::Before`].
///
/// Stack on entry: `arg`, handler, PUSHAD's frame, EFLAGS, resume address; the frame from PUSHAD on is a
/// [`Context`].
#[unsafe(naked)]
extern "C" fn context_entry() {
    core::arch::naked_asm!(
        "lea ecx, [esp + 8]",
        "push ecx",
        "push dword ptr [esp + 4]",
        "cld",
        "call dword ptr [esp + 12]",
        "add esp, 16",
        "popad",
        "popfd",
        "ret",
    );
}

/// Called by a context stub for [`Mode::After`]; returns into the copied instructions.
///
/// Stack on entry: return address (the copy), `arg`, handler, the stub's resume slot, then the [`Context`]. It
/// stores the handler's resume address in the slot and puts the return address in its place.
#[unsafe(naked)]
extern "C" fn context_entry_after() {
    core::arch::naked_asm!(
        "lea ecx, [esp + 16]",
        "push ecx",
        "push dword ptr [esp + 8]",
        "cld",
        "call dword ptr [esp + 16]",
        "add esp, 8",
        "mov eax, [esp + 12]",
        "mov ecx, [esp + 52]",
        "mov [eax], ecx",
        "mov ecx, [esp]",
        "mov [esp + 52], ecx",
        "add esp, 16",
        "popad",
        "popfd",
        "ret",
    );
}

/// The size of the FNSAVE image an FPU stub keeps below the [`Context`].
const FPU_SAVE: usize = size_of::<FpuSave>();

/// [`context_entry`] for an FPU stub. Stack on entry: `arg`, handler, then the [`FpuContext`].
///
/// After the handler it adds the exception flags the handler's x87 arithmetic raised to the saved status word, as
/// the game's own instructions would have left them, then restores the x87 state.
#[unsafe(naked)]
extern "C" fn fpu_entry() {
    core::arch::naked_asm!(
        "lea ecx, [esp + 8]",
        "push ecx",
        "push dword ptr [esp + 4]",
        "cld",
        "call dword ptr [esp + 12]",
        "add esp, 16",
        "fnstsw ax",
        "and eax, 0x3F",
        "or word ptr [esp + 4], ax",
        "frstor [esp]",
        "add esp, {save}",
        "popad",
        "popfd",
        "ret",
        save = const FPU_SAVE,
    );
}

/// [`context_entry_after`] for an FPU stub. Stack on entry: return address (the copy), `arg`, handler, the stub's
/// resume slot, then the [`FpuContext`], whose resume address is at +160.
#[unsafe(naked)]
extern "C" fn fpu_entry_after() {
    core::arch::naked_asm!(
        "lea ecx, [esp + 16]",
        "push ecx",
        "push dword ptr [esp + 8]",
        "cld",
        "call dword ptr [esp + 16]",
        "add esp, 8",
        "mov eax, [esp + 12]",
        "mov ecx, [esp + {resume}]",
        "mov [eax], ecx",
        "mov ecx, [esp]",
        "mov [esp + {resume}], ecx",
        "add esp, 16",
        "fnstsw ax",
        "and eax, 0x3F",
        "or word ptr [esp + 4], ax",
        "frstor [esp]",
        "add esp, {save}",
        "popad",
        "popfd",
        "ret",
        resume = const 16 + FPU_SAVE + offset_of!(Context, resume),
        save = const FPU_SAVE,
    );
}

/// What a stub calls.
#[derive(Clone, Copy)]
enum Entry {
    /// A context hook's handler.
    Context(Handler),
    /// An FPU hook's handler.
    Fpu(FpuHandler),
}

/// A context hook at `site`, whose instructions are `original` (at least 5 bytes, whole instructions for
/// [`Mode::Before`] and [`Mode::After`]). The site becomes a jump to a stub that calls `handler(arg, context)`.
pub fn context_hook(
    site: usize,
    original: &[u8],
    mode: Mode,
    handler: Handler,
    arg: usize,
) -> Result<SiteWrite, HookError> {
    context_hook_covering(site, original, original, mode, handler, arg)
}

/// [`context_hook`] whose jump is written over `written` (at least 5 bytes) but which stands for the whole
/// instructions `covered` that start with them: those are what [`Mode::Before`] and [`Mode::After`] run, and
/// execution resumes after them by default.
///
/// This is the form for a site whose length cuts an instruction: the jump keeps the site's length, and the stub runs
/// or replaces the instruction whole.
pub fn context_hook_covering(
    site: usize,
    written: &[u8],
    covered: &[u8],
    mode: Mode,
    handler: Handler,
    arg: usize,
) -> Result<SiteWrite, HookError> {
    hook_covering(site, written, covered, mode, Entry::Context(handler), arg)
}

/// A hook for a site in the middle of the game's x87 code: like [`context_hook_covering`], and the stub saves the
/// whole x87 state (FNSAVE) below the registers, runs the handler on an empty x87 stack under the game's control
/// word, and restores the state (FRSTOR) as the handler left it in [`FpuContext::fpu`].
///
/// So a handler can read and replace the values the game's code keeps on the x87 stack, push the value an
/// instruction it replaces would load, or pop one, and its own x87 use never disturbs the game's stack.
pub fn fpu_hook(
    site: usize,
    written: &[u8],
    covered: &[u8],
    mode: Mode,
    handler: FpuHandler,
    arg: usize,
) -> Result<SiteWrite, HookError> {
    hook_covering(site, written, covered, mode, Entry::Fpu(handler), arg)
}

fn hook_covering(
    site: usize,
    written: &[u8],
    covered: &[u8],
    mode: Mode,
    entry: Entry,
    arg: usize,
) -> Result<SiteWrite, HookError> {
    check_len(site, written)?;
    if !covered.starts_with(written) {
        return Err(HookError::NotCovering { site });
    }
    let stub = context_stub(site, covered, mode, entry, arg, site + covered.len(), false)?;
    SiteWrite::jmp(site, written, stub)
}

/// [`context_hook`], entered by a `call` instead of a `jmp`: the site becomes a call to the stub, which drops the
/// return address before anything else runs, so the handler and the copied instructions see the site's stack.
///
/// This is the form for a site whose original is a call into the addon, so the patched bytes keep the instruction
/// kind; the bytes after the call become NOPs.
pub fn context_call(
    site: usize,
    original: &[u8],
    mode: Mode,
    handler: Handler,
    arg: usize,
) -> Result<SiteWrite, HookError> {
    check_len(site, original)?;
    let stub = context_stub(
        site,
        original,
        mode,
        Entry::Context(handler),
        arg,
        site + original.len(),
        true,
    )?;
    SiteWrite::call(site, original, stub)
}

/// [`context_call`] with an FPU stub, as [`fpu_hook`] is to [`context_hook`]: for a handler that reads or changes
/// the game's x87 stack.
pub fn fpu_call(
    site: usize,
    original: &[u8],
    mode: Mode,
    handler: FpuHandler,
    arg: usize,
) -> Result<SiteWrite, HookError> {
    check_len(site, original)?;
    let stub = context_stub(
        site,
        original,
        mode,
        Entry::Fpu(handler),
        arg,
        site + original.len(),
        true,
    )?;
    SiteWrite::call(site, original, stub)
}

/// Runs `handler(arg, context)` in front of the `call rel32` at `site` (its bytes are `original`): the call goes to
/// a stub that runs the handler, then goes on to the call's old target, which returns to the caller.
///
/// The handler sees the call's return address at stack word 0 and its arguments from word 1 on; it may change
/// them and the registers. `resume` starts at the old target. Returns the site write and the old target.
pub fn call_hook(
    site: usize,
    original: [u8; 5],
    handler: Handler,
    arg: usize,
) -> Result<(SiteWrite, usize), HookError> {
    let old = call_target(site, original)?;
    let stub = context_stub(
        site,
        &original,
        Mode::Drop,
        Entry::Context(handler),
        arg,
        old,
        false,
    )?;
    Ok((
        SiteWrite::bytes(site, &original, &patch::call_rel32(site, stub))?,
        old,
    ))
}

/// The target of the `call rel32` at `site` whose bytes are `original`.
fn call_target(site: usize, original: [u8; 5]) -> Result<usize, HookError> {
    if original[0] != 0xE8 {
        return Err(HookError::NotCall {
            site,
            found: original[0],
        });
    }
    let mut rel = [0u8; 4];
    rel.copy_from_slice(&original[1..]);
    Ok((site as u32 + 5).wrapping_add(u32::from_le_bytes(rel)) as usize)
}

/// Places a context or FPU stub and returns its address. `pop_return` makes it start with `lea esp, [esp + 4]`,
/// which drops the return address of a call into it and keeps the flags.
fn context_stub(
    site: usize,
    original: &[u8],
    mode: Mode,
    entry: Entry,
    arg: usize,
    resume: usize,
    pop_return: bool,
) -> Result<usize, HookError> {
    let (before, after): (&[u8], &[u8]) = match mode {
        Mode::Drop => (&[], &[]),
        Mode::Before => (original, &[]),
        Mode::After => (&[], original),
    };
    stub_around(site, before, after, entry, arg, resume, pop_return)
}

/// A context or FPU stub that runs `before`, then the handler, then `after` (both taken from address `origin`),
/// then goes to the handler's `resume`.
fn stub_around(
    origin: usize,
    before: &[u8],
    after: &[u8],
    entry: Entry,
    arg: usize,
    resume: usize,
    pop_return: bool,
) -> Result<usize, HookError> {
    const POP_RETURN: [u8; 4] = [0x8D, 0x64, 0x24, 0x04];
    // sub esp, 108; fnsave [esp]; fldcw [esp]: the handler gets an empty stack under the game's control word.
    const SAVE_FPU: [u8; 9] = [
        0x83,
        0xEC,
        FPU_SAVE as u8,
        0xDD,
        0x34,
        0x24,
        0xD9,
        0x2C,
        0x24,
    ];
    let prefix: &[u8] = if pop_return { &POP_RETURN } else { &[] };
    // Another build: the copies are what the site holds, the handler sees RU 1.06 resume addresses.
    let (before, after) = (for_build(origin, before), for_build(origin, after));
    let (before, after, origin, resume) = (
        &before[..],
        &after[..],
        eix_sites::here(origin),
        eix_sites::here(resume),
    );
    let entry = if eix_sites::translating() {
        shimmed(entry, arg)
    } else {
        (entry, arg)
    };
    let (entry, arg) = entry;
    let (handler, save, entry_drop, entry_after): (usize, &[u8], usize, usize) = match entry {
        Entry::Context(h) => (
            h as usize,
            &[],
            context_entry as *const () as usize,
            context_entry_after as *const () as usize,
        ),
        Entry::Fpu(h) => (
            h as usize,
            &SAVE_FPU,
            fpu_entry as *const () as usize,
            fpu_entry_after as *const () as usize,
        ),
    };
    let tail = !after.is_empty();
    // push resume; pushfd; pushad; [the FPU save;] [push slot;] push handler; push arg; jmp/call entry
    let head =
        prefix.len() + before.len() + 5 + 1 + 1 + save.len() + 5 + 5 + 5 + if tail { 5 } else { 0 };
    // the copy of `after`, then `jmp [slot]` and the slot
    let len = head + if tail { after.len() + 6 + 4 } else { 0 };
    exec::place(len, |at| {
        let mut code = Vec::with_capacity(len);
        code.extend_from_slice(prefix);
        code.extend(relocate(origin, before, at + code.len())?);
        push(&mut code, resume);
        code.extend_from_slice(&[0x9C, 0x60]);
        code.extend_from_slice(save);
        if tail {
            let slot = at + len - 4;
            push(&mut code, slot);
            push(&mut code, handler);
            push(&mut code, arg);
            let from = at + code.len();
            code.extend(patch::call_rel32(from, entry_after));
            let copy_at = at + code.len();
            code.extend(relocate(origin, after, copy_at)?);
            code.extend_from_slice(&[0xFF, 0x25]);
            code.extend_from_slice(&(slot as u32).to_le_bytes());
            code.extend_from_slice(&[0; 4]);
        } else {
            push(&mut code, handler);
            push(&mut code, arg);
            let from = at + code.len();
            code.extend(patch::jmp_rel32(from, entry_drop));
        }
        Ok::<_, HookError>(code)
    })
}

/// A context stub for a jump or call the caller writes itself (the form for a site whose patched bytes must keep a
/// given layout): `before` runs first, then `handler(arg, context)`, then `after`, then execution goes on at the
/// handler's `resume`, by default `resume`. `before` and `after` are code taken from address `origin`: their 32-bit
/// branches keep their targets. `entered_by_call` drops the return address of a call into the stub first.
///
/// With `after`, the [`Mode::After`] caveat holds. Returns the stub's address.
pub fn stub(
    origin: usize,
    before: &[u8],
    after: &[u8],
    entered_by_call: bool,
    handler: Handler,
    arg: usize,
    resume: usize,
) -> Result<usize, HookError> {
    stub_around(
        origin,
        before,
        after,
        Entry::Context(handler),
        arg,
        resume,
        entered_by_call,
    )
}

/// [`stub`] with an FPU stub, as [`fpu_hook`] is to [`context_hook`]: for a handler that reads or changes the game's
/// x87 stack.
pub fn fpu_stub(
    origin: usize,
    before: &[u8],
    after: &[u8],
    entered_by_call: bool,
    handler: FpuHandler,
    arg: usize,
    resume: usize,
) -> Result<usize, HookError> {
    stub_around(
        origin,
        before,
        after,
        Entry::Fpu(handler),
        arg,
        resume,
        entered_by_call,
    )
}

/// Replaces the function at `site` with `replacement`, which must have the original's signature and calling
/// convention (an `extern "thiscall"` fn for a member function). `original` holds the function's first whole
/// instructions, at least 5 bytes.
///
/// Returns the site write and a trampoline that runs the original function.
pub fn replace_function(
    site: usize,
    original: &[u8],
    replacement: usize,
) -> Result<(SiteWrite, usize), HookError> {
    let trampoline = trampoline(site, original)?;
    Ok((SiteWrite::jmp(site, original, replacement)?, trampoline))
}

/// A copy of `original` from `site` that goes on at `site + original.len()`: runs the code a hook replaces.
pub fn trampoline(site: usize, original: &[u8]) -> Result<usize, HookError> {
    check_len(site, original)?;
    let len = original.len() + 5;
    let copy = for_build(site, original);
    let (site, original) = (eix_sites::here(site), &copy[..]);
    exec::place(len, |at| {
        let mut code = relocate(site, original, at)?;
        let from = at + code.len();
        code.extend(patch::jmp_rel32(from, site + original.len()));
        Ok::<_, HookError>(code)
    })
}

/// The code at RU 1.06 address `site` whose bytes are `original`, as the running build holds it: the bytes found
/// there when they are `original` translated, else `original` (the site's write then fails its check).
fn for_build(site: usize, original: &[u8]) -> alloc::borrow::Cow<'_, [u8]> {
    if !eix_sites::translating() || original.is_empty() {
        return alloc::borrow::Cow::Borrowed(original);
    }
    // SAFETY: a hook site lies in game.exe's mapped code, which the build check placed; nothing writes it while the
    // patch groups are built.
    alloc::borrow::Cow::Owned(unsafe { crate::translate::expected(site, original) })
}

/// The entry and argument a stub calls on another build: a shim that gives the handler RU 1.06 resume addresses.
fn shimmed(entry: Entry, arg: usize) -> (Entry, usize) {
    match entry {
        Entry::Context(h) => {
            let pair: &'static (Handler, usize) =
                alloc::boxed::Box::leak(alloc::boxed::Box::new((h, arg)));
            (Entry::Context(shim_context), pair as *const _ as usize)
        }
        Entry::Fpu(h) => {
            let pair: &'static (FpuHandler, usize) =
                alloc::boxed::Box::leak(alloc::boxed::Box::new((h, arg)));
            (Entry::Fpu(shim_fpu), pair as *const _ as usize)
        }
    }
}

extern "C" fn shim_context(pair: usize, ctx: &mut Context) {
    // SAFETY: `shimmed` leaked the pair, so it lives for the process.
    let &(handler, arg) = unsafe { &*(pair as *const (Handler, usize)) };
    ctx.resume = eix_sites::ru(ctx.resume);
    handler(arg, ctx);
    ctx.resume = eix_sites::here(ctx.resume);
}

extern "C" fn shim_fpu(pair: usize, ctx: &mut FpuContext) {
    // SAFETY: as in `shim_context`.
    let &(handler, arg) = unsafe { &*(pair as *const (FpuHandler, usize)) };
    ctx.regs.resume = eix_sites::ru(ctx.regs.resume);
    handler(arg, ctx);
    ctx.regs.resume = eix_sites::here(ctx.regs.resume);
}

/// Points the `call rel32` at `site` (its bytes are `original`) at `to`. Returns the site write and the call's old
/// target.
pub fn redirect_call(
    site: usize,
    original: [u8; 5],
    to: usize,
) -> Result<(SiteWrite, usize), HookError> {
    let old = call_target(site, original)?;
    Ok((
        SiteWrite::bytes(site, &original, &patch::call_rel32(site, to))?,
        old,
    ))
}

/// `address` (an RU 1.06 address in game.exe, or any other) as a function pointer of type `F`, in the running build.
///
/// # Safety
/// `address` must be a function of exactly type `F`, which must be a function pointer type.
pub unsafe fn as_fn<F: Copy>(address: usize) -> F {
    const { assert!(size_of::<F>() == size_of::<usize>()) };
    let address = eix_sites::here(address);
    // SAFETY: both are pointer-sized; the caller guarantees `F` is the function's type.
    unsafe { core::mem::transmute_copy(&address) }
}

#[cfg(test)]
mod tests {
    use core::hint::black_box;
    use core::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::group::{self, Outcome};
    use crate::x87::Ext;

    /// A function's address.
    macro_rules! at {
        ($f:expr) => {
            $f as *const () as usize
        };
    }

    type Fastcall = extern "fastcall" fn(u32) -> u32;
    type Thiscall = extern "thiscall" fn(u32, u32) -> u32;

    /// The five bytes every context test hooks: `mov eax, ecx; add eax, 0x10`.
    const CONTEXT_SITE: [u8; 5] = [0x89, 0xC8, 0x83, 0xC0, 0x10];

    // Each context test hooks its own copy of `x -> x + 0x10 + 0x100`; its `ret` is at +10.
    macro_rules! context_target {
        ($name:ident) => {
            #[unsafe(naked)]
            extern "fastcall" fn $name(_x: u32) -> u32 {
                core::arch::naked_asm!(
                    ".byte 0x89, 0xC8, 0x83, 0xC0, 0x10",
                    ".byte 0x05, 0x00, 0x01, 0x00, 0x00",
                    "ret",
                );
            }
        };
    }
    context_target!(target_drop);
    context_target!(target_before);
    context_target!(target_after);
    context_target!(target_after_default);
    context_target!(target_refused);
    context_target!(target_stub);

    fn install(name: &'static str, write: Result<SiteWrite, HookError>) -> Outcome {
        // SAFETY: the sites are this test binary's own functions, which no other thread runs while it is written.
        unsafe { group::apply(name, true, || Ok(vec![write?])) }.outcome
    }

    fn address(f: Fastcall) -> usize {
        f as usize
    }

    extern "C" fn drop_handler(site: usize, ctx: &mut Context) {
        ctx.eax = ctx.ecx * 2;
        ctx.resume = site + 10;
    }

    extern "C" fn before_handler(site: usize, ctx: &mut Context) {
        ctx.eax *= 3;
        ctx.resume = site + 10;
    }

    extern "C" fn after_handler(site: usize, ctx: &mut Context) {
        ctx.ecx *= 5;
        ctx.resume = site + 10;
    }

    extern "C" fn after_default_handler(_: usize, ctx: &mut Context) {
        ctx.ecx *= 5;
    }

    #[test]
    fn context_hook_modes() {
        type Case = (Fastcall, Mode, Handler, fn(u32) -> u32);
        let cases: [Case; 4] = [
            // The replaced instructions never run; the add of 0x100 is skipped.
            (target_drop, Mode::Drop, drop_handler, |x| x * 2),
            // They run first, so the handler sees x + 0x10.
            (target_before, Mode::Before, before_handler, |x| {
                (x + 0x10) * 3
            }),
            // They run after the handler changed ECX, then go to the handler's resume.
            (target_after, Mode::After, after_handler, |x| x * 5 + 0x10),
            (
                target_after_default,
                Mode::After,
                after_default_handler,
                |x| x * 5 + 0x110,
            ),
        ];
        for (f, mode, handler, want) in cases {
            let site = address(f);
            assert_eq!(black_box(f)(7), 7 + 0x110);
            let write = context_hook(site, &CONTEXT_SITE, mode, handler, site);
            assert_eq!(install("context", write), Outcome::Applied, "{mode:?}");
            for x in [0, 7, 1000] {
                assert_eq!(black_box(f)(x), want(x), "{mode:?} with {x}");
            }
        }
    }

    #[test]
    fn a_stub_runs_its_code_around_the_handler() {
        let site = address(target_stub);
        // add ecx, 1 / the handler's ecx * 5 / mov eax, ecx; then the target's own add of 0x100.
        let stub = stub(
            site,
            &[0x83, 0xC1, 0x01],
            &[0x89, 0xC8],
            false,
            after_default_handler,
            0,
            site + 5,
        )
        .unwrap();
        let write = SiteWrite::jmp(site, &CONTEXT_SITE, stub);
        assert_eq!(install("stub", write), Outcome::Applied);
        for x in [0, 7, 1000] {
            assert_eq!(black_box(target_stub as Fastcall)(x), (x + 1) * 5 + 0x100);
        }
    }

    #[test]
    fn context_hook_keeps_the_stack_and_restores_registers() {
        static SEEN: AtomicUsize = AtomicUsize::new(0);
        extern "C" fn handler(_: usize, ctx: &mut Context) {
            SEEN.store(ctx.stack(), Ordering::SeqCst);
            ctx.edx = 0x1234;
        }
        // The hooked bytes (dropped) are `push ebx; push esi; push edi; push ebp; nop`; then `mov eax, esp`, and
        // `add eax, edx`. EAX is the caller's ESP plus the handler's EDX.
        #[unsafe(naked)]
        extern "fastcall" fn target(_x: u32) -> u32 {
            core::arch::naked_asm!(
                ".byte 0x53, 0x56, 0x57, 0x55, 0x90",
                "mov eax, esp",
                "add eax, edx",
                "ret"
            );
        }
        let site = at!(target);
        let write = context_hook(
            site,
            &[0x53, 0x56, 0x57, 0x55, 0x90],
            Mode::Drop,
            handler,
            0,
        );
        assert_eq!(install("registers", write), Outcome::Applied);
        let esp = black_box(target as Fastcall)(0) as usize - 0x1234;
        // The handler saw the ESP the code at the site sees.
        assert_eq!(SEEN.load(Ordering::SeqCst), esp);
    }

    type Stdcall = extern "system" fn(u32) -> u32;

    /// Its argument: `mov eax, [esp + 4]; nop`, then `ret 4`. The copied `mov` reads the right word only if the
    /// call-entered stub dropped its return address first.
    #[unsafe(naked)]
    extern "system" fn first_arg(_a: u32) -> u32 {
        core::arch::naked_asm!(".byte 0x8B, 0x44, 0x24, 0x04, 0x90", "ret 4");
    }

    static FIRST_ARG_SEEN: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn first_arg_handler(_: usize, ctx: &mut Context) {
        FIRST_ARG_SEEN.store(ctx.stack_word(1).unwrap_or(0) as usize, Ordering::SeqCst);
        ctx.eax += 1;
    }

    #[test]
    fn call_entered_context_hook_sees_the_sites_stack() {
        let site = at!(first_arg);
        let write = context_call(
            site,
            &[0x8B, 0x44, 0x24, 0x04, 0x90],
            Mode::Before,
            first_arg_handler,
            0,
        );
        assert_eq!(
            write.as_ref().map(|w| w.site()),
            Ok(site),
            "the site is a call"
        );
        assert_eq!(install("context call", write), Outcome::Applied);
        // SAFETY: the function's first byte is mapped and nothing writes it.
        assert_eq!(unsafe { *(site as *const u8) }, 0xE8);
        assert_eq!(black_box(first_arg as Stdcall)(41), 42);
        assert_eq!(FIRST_ARG_SEEN.load(Ordering::SeqCst), 41);
    }

    #[unsafe(naked)]
    extern "system" fn doubled_stdcall(_x: u32) -> u32 {
        core::arch::naked_asm!("mov eax, [esp + 4]", "add eax, eax", "ret 4");
    }

    /// `doubled_stdcall(x)`: `push dword ptr [esp + 4]` (4 bytes), then the call at +4, then `ret 4`.
    #[unsafe(naked)]
    extern "system" fn calls_doubled(_x: u32) -> u32 {
        core::arch::naked_asm!(
            ".byte 0xFF, 0x74, 0x24, 0x04",
            "call {f}",
            "ret 4",
            f = sym doubled_stdcall
        );
    }

    static CALL_RETURN_SEEN: AtomicUsize = AtomicUsize::new(0);
    /// Bit 0: the argument was replaced; bit 1: a word past the stack was refused for reading, bit 2 for writing.
    static BOUNDS_SEEN: AtomicUsize = AtomicUsize::new(0);

    // A panic cannot leave a handler (it is called from the stub), so the checks are recorded and asserted later.
    extern "C" fn add_ten_to_first_arg(_: usize, ctx: &mut Context) {
        CALL_RETURN_SEEN.store(ctx.stack_word(0).unwrap_or(0) as usize, Ordering::SeqCst);
        let x = ctx.stack_word(1).unwrap_or(0);
        let mut seen = usize::from(ctx.set_stack_word(1, x + 10));
        seen |= usize::from(ctx.stack_word(usize::MAX / 8).is_none()) << 1;
        seen |= usize::from(!ctx.set_stack_word(usize::MAX / 8, 0)) << 2;
        BOUNDS_SEEN.store(seen, Ordering::SeqCst);
    }

    #[test]
    fn call_hook_changes_the_arguments_then_runs_the_old_target() {
        let site = at!(calls_doubled) + 4;
        let original = patch::call_rel32(site, at!(doubled_stdcall));
        let (write, old) = call_hook(site, original, add_ten_to_first_arg, 0).unwrap();
        assert_eq!(old, at!(doubled_stdcall));
        assert_eq!(install("call hook", Ok(write)), Outcome::Applied);
        assert_eq!(black_box(calls_doubled as Stdcall)(5), 30);
        assert_eq!(CALL_RETURN_SEEN.load(Ordering::SeqCst), site + 5);
        assert_eq!(BOUNDS_SEEN.load(Ordering::SeqCst), 0b111);
        assert_eq!(
            call_hook(site, [0x90; 5], add_ten_to_first_arg, 0).map(|(_, old)| old),
            Err(HookError::NotCall { site, found: 0x90 })
        );
    }

    static ADD_TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);

    /// `this + a`, as a member function: `mov eax, [esp+4]; add eax, ecx; ret 4`.
    #[unsafe(naked)]
    extern "thiscall" fn add(_this: u32, _a: u32) -> u32 {
        core::arch::naked_asm!(".byte 0x8B, 0x44, 0x24, 0x04, 0x01, 0xC8", "ret 4");
    }

    extern "thiscall" fn add_times_ten(this: u32, a: u32) -> u32 {
        // SAFETY: the trampoline runs the original `add`, which has this type.
        let original: Thiscall = unsafe { as_fn(ADD_TRAMPOLINE.load(Ordering::SeqCst)) };
        original(this, a) * 10
    }

    #[test]
    fn function_replacement_calls_through_its_trampoline() {
        let site = at!(add);
        assert_eq!(black_box(add as Thiscall)(3, 4), 7);
        let (write, trampoline) = replace_function(
            site,
            &[0x8B, 0x44, 0x24, 0x04, 0x01, 0xC8],
            at!(add_times_ten),
        )
        .unwrap();
        ADD_TRAMPOLINE.store(trampoline, Ordering::SeqCst);
        assert_eq!(install("replace", Ok(write)), Outcome::Applied);
        assert_eq!(black_box(add as Thiscall)(3, 4), 70);
    }

    #[unsafe(naked)]
    extern "fastcall" fn seven() -> u32 {
        core::arch::naked_asm!("mov eax, 7", "ret");
    }

    /// `7 + x`: starts with `call seven` (E8), then `add eax, ecx; ret`.
    #[unsafe(naked)]
    extern "fastcall" fn calls_seven(_x: u32) -> u32 {
        core::arch::naked_asm!("call {seven}", ".byte 0x01, 0xC8", "ret", seven = sym seven);
    }

    /// `x == 0 ? 2 : 1`, starting with `test ecx, ecx; je rel32`.
    #[unsafe(naked)]
    extern "fastcall" fn is_zero(_x: u32) -> u32 {
        core::arch::naked_asm!(
            ".byte 0x85, 0xC9, 0x0F, 0x84",
            ".long 2f - . - 4",
            "mov eax, 1",
            "ret",
            "2:",
            "mov eax, 2",
            "ret",
        );
    }

    /// 9, starting with `jmp rel32` over two `int3`.
    #[unsafe(naked)]
    extern "fastcall" fn nine() -> u32 {
        core::arch::naked_asm!(
            ".byte 0xE9",
            ".long 2f - . - 4",
            "int3",
            "int3",
            "2:",
            "mov eax, 9",
            "ret"
        );
    }

    static CALLS_SEVEN: AtomicUsize = AtomicUsize::new(0);
    static IS_ZERO: AtomicUsize = AtomicUsize::new(0);
    static NINE: AtomicUsize = AtomicUsize::new(0);

    extern "fastcall" fn calls_seven_plus(x: u32) -> u32 {
        // SAFETY: the trampoline runs the original, which has this type.
        let f: Fastcall = unsafe { as_fn(CALLS_SEVEN.load(Ordering::SeqCst)) };
        f(x) + 1000
    }

    extern "fastcall" fn is_zero_plus(x: u32) -> u32 {
        // SAFETY: as above.
        let f: Fastcall = unsafe { as_fn(IS_ZERO.load(Ordering::SeqCst)) };
        f(x) + 100
    }

    extern "fastcall" fn nine_plus() -> u32 {
        // SAFETY: as above.
        let f: extern "fastcall" fn() -> u32 = unsafe { as_fn(NINE.load(Ordering::SeqCst)) };
        f() + 1
    }

    #[test]
    fn trampolines_run_relocated_branches() {
        // E8: the copied call still reaches `seven`.
        let site = at!(calls_seven);
        let original = patch::call_rel32(site, at!(seven));
        let (write, t) = replace_function(site, &original, at!(calls_seven_plus)).unwrap();
        CALLS_SEVEN.store(t, Ordering::SeqCst);
        assert_eq!(install("e8", Ok(write)), Outcome::Applied);
        assert_eq!(black_box(calls_seven as Fastcall)(5), 1012);

        // 0F 84, taken and not taken: both paths leave the copy.
        let site = at!(is_zero);
        let original = [0x85, 0xC9, 0x0F, 0x84, 6, 0, 0, 0];
        let (write, t) = replace_function(site, &original, at!(is_zero_plus)).unwrap();
        IS_ZERO.store(t, Ordering::SeqCst);
        assert_eq!(install("0f84", Ok(write)), Outcome::Applied);
        assert_eq!(black_box(is_zero as Fastcall)(0), 102);
        assert_eq!(black_box(is_zero as Fastcall)(5), 101);

        // E9.
        let site = at!(nine);
        let (write, t) = replace_function(site, &[0xE9, 2, 0, 0, 0], at!(nine_plus)).unwrap();
        NINE.store(t, Ordering::SeqCst);
        assert_eq!(install("e9", Ok(write)), Outcome::Applied);
        assert_eq!(black_box(nine as extern "fastcall" fn() -> u32)(), 10);
    }

    #[unsafe(naked)]
    extern "fastcall" fn plus_one(_x: u32) -> u32 {
        core::arch::naked_asm!("lea eax, [ecx + 1]", "ret");
    }

    /// Calls `plus_one`: `call plus_one; ret`.
    #[unsafe(naked)]
    extern "fastcall" fn calls_plus_one(_x: u32) -> u32 {
        core::arch::naked_asm!("call {f}", "ret", f = sym plus_one);
    }

    static OLD_CALL: AtomicUsize = AtomicUsize::new(0);

    extern "fastcall" fn doubled(x: u32) -> u32 {
        // SAFETY: the old target is `plus_one`, of this type.
        let f: Fastcall = unsafe { as_fn(OLD_CALL.load(Ordering::SeqCst)) };
        f(x) * 2
    }

    #[test]
    fn call_redirection_keeps_the_old_target_callable() {
        let site = at!(calls_plus_one);
        assert_eq!(black_box(calls_plus_one as Fastcall)(4), 5);
        let original = patch::call_rel32(site, at!(plus_one));
        let (write, old) = redirect_call(site, original, at!(doubled)).unwrap();
        assert_eq!(old, at!(plus_one));
        OLD_CALL.store(old, Ordering::SeqCst);
        assert_eq!(install("redirect", Ok(write)), Outcome::Applied);
        assert_eq!(black_box(calls_plus_one as Fastcall)(4), 10);
        assert_eq!(
            redirect_call(site, [0xE9, 0, 0, 0, 0], 0),
            Err(HookError::NotCall { site, found: 0xE9 })
        );
    }

    #[test]
    fn refused_install_leaves_the_code_alone() {
        let site = address(target_refused);
        // SAFETY: the function's first five bytes are mapped and nothing writes them.
        let before = unsafe { core::slice::from_raw_parts(site as *const u8, 5) }.to_vec();
        let wrong = [0x89, 0xC8, 0x83, 0xC0, 0x11];
        let write = context_hook(site, &wrong, Mode::Before, drop_handler, site);
        match install("refused", write) {
            Outcome::Failed {
                error: HookError::Patch(PatchError::Mismatch { found, .. }),
                unrestored,
            } => {
                assert_eq!(found, CONTEXT_SITE);
                assert!(unrestored.is_empty());
            }
            other @ (Outcome::Applied | Outcome::Disabled | Outcome::Failed { .. }) => {
                panic!("installed over the wrong bytes: {other:?}")
            }
        }
        // SAFETY: as above.
        let after = unsafe { core::slice::from_raw_parts(site as *const u8, 5) };
        assert_eq!(after, before);
        assert_eq!(black_box(target_refused as Fastcall)(7), 7 + 0x110);
    }

    #[test]
    fn short_or_uncopyable_sites_are_refused() {
        assert_eq!(
            context_hook(0x1000, &[0x90; 4], Mode::Drop, drop_handler, 0),
            Err(HookError::TooShort {
                site: 0x1000,
                len: 4
            })
        );
        // cmp eax, 0x6B; je +9: a short branch cannot be copied.
        assert_eq!(
            context_hook(
                0x1000,
                &[0x83, 0xF8, 0x6B, 0x74, 0x09],
                Mode::Before,
                drop_handler,
                0
            ),
            Err(HookError::Code {
                site: 0x1000,
                error: CodeError::ShortBranch { offset: 3 }
            })
        );
        // Dropped instructions are never copied, so any bytes do.
        assert!(
            context_hook(
                0x1000,
                &[0x83, 0xF8, 0x6B, 0x74, 0x09],
                Mode::Drop,
                drop_handler,
                0
            )
            .is_ok()
        );
        // The covered instructions must start with the written bytes.
        assert_eq!(
            context_hook_covering(0x1000, &[0x90; 5], &[0x91; 6], Mode::Drop, drop_handler, 0),
            Err(HookError::NotCovering { site: 0x1000 })
        );
    }

    #[test]
    fn a_covering_hook_writes_its_length_and_resumes_after_the_covered_code() {
        // `mov eax, [esp + 0x110]` is 7 bytes; the jump is written over its first 5, and Before runs it whole.
        #[unsafe(naked)]
        extern "fastcall" fn target(_x: u32) -> u32 {
            core::arch::naked_asm!(
                "sub esp, 0x10C",
                "mov [esp], ecx",
                ".byte 0x8B, 0x84, 0x24, 0x00, 0x00, 0x00, 0x00",
                "add esp, 0x10C",
                "ret"
            );
        }
        extern "C" fn plus_one(_: usize, ctx: &mut Context) {
            ctx.eax += 1;
        }
        let covered = [0x8B, 0x84, 0x24, 0, 0, 0, 0];
        let site = find(at!(target), &covered);
        let write = context_hook_covering(site, &covered[..5], &covered, Mode::Before, plus_one, 0);
        assert_eq!(write.as_ref().map(SiteWrite::size), Ok(5));
        assert_eq!(install("covering", write), Outcome::Applied);
        assert_eq!(black_box(target as Fastcall)(41), 42);
    }

    /// Recorded by the FPU handlers: ST(0) as float bits, the stack depth, the control word, a flag word.
    static FPU_ST0: AtomicUsize = AtomicUsize::new(0);
    static FPU_DEPTH: AtomicUsize = AtomicUsize::new(0);
    static FPU_CONTROL: AtomicUsize = AtomicUsize::new(0);
    static FPU_SEEN: AtomicUsize = AtomicUsize::new(0);

    fn record(fpu: &FpuSave) {
        let st0 = fpu.st(0).map_or(u32::MAX, |v| v.to_f32().to_bits());
        FPU_ST0.store(st0 as usize, Ordering::SeqCst);
        FPU_DEPTH.store(fpu.depth(), Ordering::SeqCst);
        FPU_CONTROL.store(usize::from(fpu.control_word()), Ordering::SeqCst);
    }

    /// `a + ...`: `fld dword ptr [esp + 4]`, five NOPs (the site), `faddp st(1), st`, `ret`.
    #[unsafe(naked)]
    extern "C" fn fpu_push_target(_a: f32) -> f32 {
        core::arch::naked_asm!(
            ".byte 0xD9, 0x44, 0x24, 0x04",
            ".byte 0x90, 0x90, 0x90, 0x90, 0x90",
            ".byte 0xDE, 0xC1",
            "ret"
        );
    }

    extern "C" fn push_ten(_: usize, ctx: &mut FpuContext) {
        record(&ctx.fpu);
        let pushed = ctx.fpu.push(Ext::new(10.0));
        FPU_SEEN.store(usize::from(pushed), Ordering::SeqCst);
    }

    /// [`fpu_push_target`]'s twin, for a site of its own.
    #[unsafe(naked)]
    extern "C" fn fpu_call_target(_a: f32) -> f32 {
        core::arch::naked_asm!(
            ".byte 0xD9, 0x44, 0x24, 0x04",
            ".byte 0x90, 0x90, 0x90, 0x90, 0x90",
            ".byte 0xDE, 0xC1",
            "ret"
        );
    }

    extern "C" fn push_twenty(_: usize, ctx: &mut FpuContext) {
        let _ = ctx.fpu.push(Ext::new(20.0));
    }

    #[test]
    fn an_fpu_call_pushes_onto_the_games_stack() {
        let site = at!(fpu_call_target) + 4;
        let write = fpu_call(site, &[0x90; 5], Mode::Drop, push_twenty, 0);
        assert_eq!(install("fpu call", write), Outcome::Applied);
        // SAFETY: the site is mapped and nothing writes it.
        assert_eq!(unsafe { *(site as *const u8) }, 0xE8);
        assert_eq!(
            black_box(fpu_call_target as extern "C" fn(f32) -> f32)(2.5),
            22.5
        );
    }

    #[test]
    fn an_fpu_hook_pushes_onto_the_games_stack() {
        let site = at!(fpu_push_target) + 4;
        let write = fpu_hook(site, &[0x90; 5], &[0x90; 5], Mode::Drop, push_ten, 0);
        assert_eq!(install("fpu push", write), Outcome::Applied);
        assert_eq!(
            black_box(fpu_push_target as extern "C" fn(f32) -> f32)(2.5),
            12.5
        );
        assert_eq!(FPU_ST0.load(Ordering::SeqCst), 2.5f32.to_bits() as usize);
        assert_eq!(FPU_DEPTH.load(Ordering::SeqCst), 1);
        assert_eq!(FPU_SEEN.load(Ordering::SeqCst), 1);
    }

    /// Sets single precision (and clears the exception flags), loads 1.0 seven times, doubles ST(0) at the site
    /// (`fadd st, st` and three NOPs), adds the stack up and stores the status word at `status`; restores the caller's
    /// control word.
    #[unsafe(naked)]
    extern "C" fn fpu_full_target(_status: *mut u16) -> f32 {
        core::arch::naked_asm!(
            "sub esp, 4",
            "fnstcw word ptr [esp]",
            "mov ax, word ptr [esp]",
            "and ax, 0xFCFF",
            "mov word ptr [esp + 2], ax",
            "fldcw word ptr [esp + 2]",
            "fnclex",
            ".byte 0xD9, 0xE8, 0xD9, 0xE8, 0xD9, 0xE8, 0xD9, 0xE8, 0xD9, 0xE8, 0xD9, 0xE8, 0xD9, 0xE8",
            ".byte 0xD8, 0xC0, 0x90, 0x90, 0x90",
            ".byte 0xDE, 0xC1, 0xDE, 0xC1, 0xDE, 0xC1, 0xDE, 0xC1, 0xDE, 0xC1, 0xDE, 0xC1",
            "mov ecx, [esp + 8]",
            "fnstsw word ptr [ecx]",
            "fldcw word ptr [esp]",
            "add esp, 4",
            "ret",
        );
    }

    /// Where `bytes` first occur in the 64 bytes at `f`: a site in a test function whose assembled length before the
    /// site is the assembler's choice.
    fn find(f: usize, bytes: &[u8]) -> usize {
        // SAFETY: the test functions are longer than 64 bytes or followed by more of the test binary's code.
        let code = unsafe { core::slice::from_raw_parts(f as *const u8, 64) };
        f + code
            .windows(bytes.len())
            .position(|w| w == bytes)
            .unwrap_or(usize::MAX - f)
    }

    extern "C" fn add_five_in_single_precision(_: usize, ctx: &mut FpuContext) {
        record(&ctx.fpu);
        // 1 + 2^-30 is 1 at the game's single precision; the inexact result raises the precision flag.
        let rounded = Ext::new(1.0).fadd(1.0 / 1_073_741_824.0) == Ext::new(1.0);
        let deep = ctx.fpu.st(6) == Some(Ext::new(1.0)) && ctx.fpu.st(7).is_none();
        let st0 = ctx.fpu.st(0).unwrap_or(Ext::new(f64::NAN));
        let replaced = ctx.fpu.set_st(0, st0.fadd(5.0));
        FPU_SEEN.store(
            usize::from(rounded) | usize::from(deep) << 1 | usize::from(replaced) << 2,
            Ordering::SeqCst,
        );
    }

    #[test]
    fn an_fpu_hook_keeps_a_deep_stack_and_runs_under_the_games_control_word() {
        let covered = [0xD8, 0xC0, 0x90, 0x90, 0x90];
        let site = find(at!(fpu_full_target), &covered);
        let write = fpu_hook(
            site,
            &covered,
            &covered,
            Mode::Before,
            add_five_in_single_precision,
            0,
        );
        assert_eq!(install("fpu full", write), Outcome::Applied);
        let mut status = 0u16;
        // 2 (doubled before the handler) + 5, then six more ones.
        assert_eq!(
            black_box(fpu_full_target as extern "C" fn(*mut u16) -> f32)(&mut status),
            13.0
        );
        assert_eq!(FPU_ST0.load(Ordering::SeqCst), 2.0f32.to_bits() as usize);
        assert_eq!(FPU_DEPTH.load(Ordering::SeqCst), 7);
        assert_eq!(FPU_CONTROL.load(Ordering::SeqCst) & 0x300, 0);
        assert_eq!(FPU_SEEN.load(Ordering::SeqCst), 0b111);
        // The handler's inexact addition left the precision flag, as the game's own instruction would have.
        assert_eq!(status & 0x20, 0x20);
    }

    /// `(a + ...) * 2`: `fld dword ptr [esp + 4]`, then `fadd st, st` and three NOPs (the site), `ret`.
    #[unsafe(naked)]
    extern "C" fn fpu_after_target(_a: f32) -> f32 {
        core::arch::naked_asm!(
            ".byte 0xD9, 0x44, 0x24, 0x04",
            ".byte 0xD8, 0xC0, 0x90, 0x90, 0x90",
            "ret"
        );
    }

    extern "C" fn add_one(_: usize, ctx: &mut FpuContext) {
        if let Some(st0) = ctx.fpu.st(0) {
            ctx.fpu.set_st(0, st0.fadd(1.0));
        }
    }

    /// `a`: loads `a` and `b`, then the site pops `b` (`fstp st(0)` and three NOPs).
    #[unsafe(naked)]
    extern "C" fn fpu_pop_target(_a: f32, _b: f32) -> f32 {
        core::arch::naked_asm!(
            ".byte 0xD9, 0x44, 0x24, 0x04",
            ".byte 0xD9, 0x44, 0x24, 0x08",
            ".byte 0xDD, 0xD8, 0x90, 0x90, 0x90",
            "ret"
        );
    }

    extern "C" fn pop_instead(_: usize, ctx: &mut FpuContext) {
        let popped = ctx.fpu.pop().map_or(u32::MAX, |v| v.to_f32().to_bits());
        FPU_ST0.store(popped as usize, Ordering::SeqCst);
        ctx.regs.eax = 0x5A5A;
    }

    #[test]
    fn an_fpu_hook_runs_covered_code_after_or_pops_in_its_place() {
        let site = at!(fpu_after_target) + 4;
        let covered = [0xD8, 0xC0, 0x90, 0x90, 0x90];
        let write = fpu_hook(site, &covered, &covered, Mode::After, add_one, 0);
        assert_eq!(install("fpu after", write), Outcome::Applied);
        assert_eq!(
            black_box(fpu_after_target as extern "C" fn(f32) -> f32)(3.0),
            8.0
        );

        let site = at!(fpu_pop_target) + 8;
        let covered = [0xDD, 0xD8, 0x90, 0x90, 0x90];
        let write = fpu_hook(site, &covered, &covered, Mode::Drop, pop_instead, 0);
        assert_eq!(install("fpu pop", write), Outcome::Applied);
        assert_eq!(
            black_box(fpu_pop_target as extern "C" fn(f32, f32) -> f32)(4.0, 9.0),
            4.0
        );
        assert_eq!(FPU_ST0.load(Ordering::SeqCst), 9.0f32.to_bits() as usize);
    }
}
