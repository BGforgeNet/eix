//! A crash handler's Windows side: the process's unhandled-exception filter, which hands the crash to a handler once,
//! after the filters other modules set later (see [`keep_first`]); what the handler reads of the crash (registers, the
//! access, code addresses on the stack); stopping the other threads; and a minidump through dbghelp.dll's
//! `MiniDumpWriteDump` (loaded before the handler can suspend other threads).
#![expect(
    unsafe_code,
    reason = "the exception filter, reading the crash's records and stack, thread suspension and minidump Win32 calls"
)]

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CREATE_ALWAYS, CreateFileW, FILE_ATTRIBUTE_NORMAL};
use windows_sys::Win32::System::Diagnostics::Debug::{
    EXCEPTION_POINTERS, LPTOP_LEVEL_EXCEPTION_FILTER, MINIDUMP_EXCEPTION_INFORMATION,
    MINIDUMP_TYPE, MiniDumpWithFullMemory, MiniDumpWithIndirectlyReferencedMemory,
    SetUnhandledExceptionFilter,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_IMAGE, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
    PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY,
    VirtualQuery,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, OpenThread, SuspendThread,
    THREAD_SUSPEND_RESUME,
};

use crate::win::os_error;

/// An unhandled exception, as the handler sees it.
#[derive(Debug)]
pub struct Crash {
    /// The exception code.
    pub code: u32,
    /// Where it happened.
    pub address: usize,
    /// The code and address of a fault inside another module's filter, which ended that filter early.
    pub chained_fault: Option<(u32, usize)>,
    pointers: *const EXCEPTION_POINTERS,
    dump_writer: Result<MiniDumpWriteDump, String>,
}

/// The crashing thread's integer registers and flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(missing_docs, reason = "each field is the register of its name")]
pub struct Registers {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
    pub esi: u32,
    pub edi: u32,
    pub ebp: u32,
    pub esp: u32,
    pub eip: u32,
    pub eflags: u32,
}

impl Crash {
    /// Writes a minidump of the process for this exception to `path` (UTF-16, no NUL), with the whole memory when
    /// `full`, else the memory the stacks point at. The error names the step that failed and the system's error.
    pub fn write_minidump(&self, path: &[u16], full: bool) -> Result<(), String> {
        let write = *self.dump_writer.as_ref().map_err(Clone::clone)?;
        // SAFETY: a `Crash` exists only inside the running filter, on the crashing thread, with its pointers.
        unsafe { write_minidump(write, path, self.pointers, full) }
    }

    /// The registers at the exception; none without a context record.
    pub fn registers(&self) -> Option<Registers> {
        if self.pointers.is_null() {
            return None;
        }
        // SAFETY: Windows passes valid pointers for the duration of the filter, which a `Crash` does not outlive.
        let context = unsafe { (*self.pointers).ContextRecord };
        // SAFETY: as above; the record is null or valid.
        let c = unsafe { context.as_ref() }?;
        Some(Registers {
            eax: c.Eax,
            ebx: c.Ebx,
            ecx: c.Ecx,
            edx: c.Edx,
            esi: c.Esi,
            edi: c.Edi,
            ebp: c.Ebp,
            esp: c.Esp,
            eip: c.Eip,
            eflags: c.EFlags,
        })
    }

    /// The exception record's first two parameters, which an access violation and an in-page error fill with the
    /// kind of access (0 read, 1 write, 8 execute) and the address accessed; none with fewer parameters.
    pub fn access(&self) -> Option<(u32, u32)> {
        if self.pointers.is_null() {
            return None;
        }
        // SAFETY: as in `registers`.
        let record = unsafe { (*self.pointers).ExceptionRecord };
        // SAFETY: as above.
        let r = unsafe { record.as_ref() }?;
        (r.NumberParameters >= 2).then(|| {
            (
                r.ExceptionInformation[0] as u32,
                r.ExceptionInformation[1] as u32,
            )
        })
    }

    /// The values on the crashing thread's stack, from ESP up to `words` of them or the stack's top, that point into
    /// executable code of a loaded image, each with the 7 bytes before it (where the call that returns there would
    /// end). The caller decides which of them follow a call.
    pub fn code_pointers_on_stack(&self, words: usize) -> Vec<(u32, [u8; 7])> {
        let Some(esp) = self.registers().map(|r| r.esp as usize) else {
            return Vec::new();
        };
        // The stack is committed from ESP's page up to its top: one region.
        let Some(top) = region(esp)
            .filter(|m| readable(m.Protect) && m.State == MEM_COMMIT)
            .map(|m| m.BaseAddress as usize + m.RegionSize)
        else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for slot in (esp..top.min(esp.saturating_add(words * 4))).step_by(4) {
            // SAFETY: inside the committed, readable stack region.
            let value = unsafe { core::ptr::with_exposed_provenance::<u32>(slot).read_unaligned() }
                as usize;
            let Some(start) = value.checked_sub(7) else {
                continue;
            };
            if executable_image(start) && executable_image(value - 1) {
                // SAFETY: both ends lie in executable image memory, which is mapped and readable.
                let before = unsafe {
                    core::ptr::with_exposed_provenance::<[u8; 7]>(start).read_unaligned()
                };
                found.push((value as u32, before));
            }
        }
        found
    }

    /// Suspends every other thread of the process, repeating until a pass stops none, as the original does.
    pub fn stop_other_threads(&self) {
        while suspend_other_threads() > 0 {}
    }
}

/// The memory region holding `address`.
fn region(address: usize) -> Option<MEMORY_BASIC_INFORMATION> {
    // SAFETY: an all-zero MEMORY_BASIC_INFORMATION is a valid value of the plain struct.
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { core::mem::zeroed() };
    let size = size_of::<MEMORY_BASIC_INFORMATION>();
    // SAFETY: any address may be queried; the out buffer matches the size passed.
    let written = unsafe {
        VirtualQuery(
            core::ptr::with_exposed_provenance(address),
            &raw mut info,
            size,
        )
    };
    (written == size).then_some(info)
}

/// The protections that let code run, readable (`PAGE_EXECUTE` alone cannot be read).
const READ_EXECUTE: u32 = PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY;

/// Whether a protection lets the memory be read.
fn readable(protect: u32) -> bool {
    protect & (PAGE_READONLY | PAGE_READWRITE | PAGE_WRITECOPY | READ_EXECUTE) != 0
        && protect & PAGE_GUARD == 0
}

/// Whether `address` lies in committed, executable and readable memory of a loaded image.
fn executable_image(address: usize) -> bool {
    region(address).is_some_and(|m| {
        m.State == MEM_COMMIT
            && m.Type == MEM_IMAGE
            && m.Protect & READ_EXECUTE != 0
            && m.Protect & PAGE_GUARD == 0
    })
}

/// The crash handler: called once, on the crashing thread, after the filters other modules set later; the other
/// threads still run until it calls [`Crash::stop_other_threads`]. It should end the process.
pub type Handler = fn(&Crash);

static HANDLER: AtomicUsize = AtomicUsize::new(0);
/// The filter that was set before ours: a crash inside our handler goes there.
static PREVIOUS: AtomicUsize = AtomicUsize::new(0);
/// The most filters of other modules kept; a later one past these replaces ours unseen.
const MAX_CHAINED: usize = 8;
/// Filters other modules set after ours, in the order they were found (see [`keep_first`]); 0 for an empty slot.
static CHAINED: [AtomicUsize; MAX_CHAINED] = [const { AtomicUsize::new(0) }; MAX_CHAINED];
/// The thread inside the filter, or 0.
static HANDLING: AtomicU32 = AtomicU32::new(0);
/// Whether that thread is running the other modules' filters (else our handler).
static IN_CHAINED: AtomicBool = AtomicBool::new(false);
/// The exception pointers of the crash being handled.
static CRASH: AtomicUsize = AtomicUsize::new(0);

type Filter = unsafe extern "system" fn(*const EXCEPTION_POINTERS) -> i32;

/// The `EXCEPTION_CONTINUE_SEARCH` result: Windows goes on as without this filter.
const CONTINUE_SEARCH: i32 = 0;
/// The `EXCEPTION_CONTINUE_EXECUTION` result: the filter repaired the fault and the thread goes on.
const CONTINUE_EXECUTION: i32 = -1;

unsafe extern "system" fn filter(pointers: *const EXCEPTION_POINTERS) -> i32 {
    // SAFETY: no preconditions.
    let me = unsafe { GetCurrentThreadId() };
    if HANDLING.load(Ordering::SeqCst) == me {
        if IN_CHAINED.load(Ordering::SeqCst) {
            let crash = CRASH.load(Ordering::SeqCst);
            // Another module's filter passing the crash on to the one it replaced, which was ours: ours runs after
            // it anyway.
            if pointers as usize == crash {
                return CONTINUE_SEARCH;
            }
            // A fault inside another module's filter (um.dll's faults part way under Wine). Left to Windows it would
            // end the process; ours handles the first crash from here instead.
            IN_CHAINED.store(false, Ordering::SeqCst);
            // SAFETY: the pointers Windows passed to this filter.
            let fault = unsafe { code_and_address(pointers) };
            // SAFETY: CRASH holds the pointers of the crash still being handled, further up this thread's stack.
            unsafe { run_handler(core::ptr::with_exposed_provenance(crash), Some(fault)) };
            return CONTINUE_SEARCH;
        }
        // A crash inside the handler goes to the filter that was there before, as in the original.
        let previous = PREVIOUS.load(Ordering::SeqCst);
        if previous == 0 {
            return CONTINUE_SEARCH;
        }
        // SAFETY: PREVIOUS holds the replaced filter, a filter function of the process.
        let previous: Filter = unsafe { crate::hook::as_fn_at(previous) };
        // SAFETY: the pointers Windows passed to this filter.
        return unsafe { previous(pointers) };
    }
    // One crash at a time; a second crashing thread waits here until the first stops it.
    while HANDLING
        .compare_exchange(0, me, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        core::hint::spin_loop();
    }
    // The filters other modules set later run first, newest first as Windows would run them, with the other
    // threads still running (they may wait on those threads' locks). If one repaired the fault, the thread goes on.
    CRASH.store(pointers as usize, Ordering::SeqCst);
    IN_CHAINED.store(true, Ordering::SeqCst);
    for slot in CHAINED.iter().rev() {
        let chained = slot.load(Ordering::SeqCst);
        if chained == 0 {
            continue;
        }
        // SAFETY: CHAINED holds filter functions of the process, stored by `keep_first`.
        let chained: Filter = unsafe { crate::hook::as_fn_at(chained) };
        // SAFETY: the pointers Windows passed to this filter.
        if unsafe { chained(pointers) } == CONTINUE_EXECUTION {
            IN_CHAINED.store(false, Ordering::SeqCst);
            HANDLING.store(0, Ordering::SeqCst);
            return CONTINUE_EXECUTION;
        }
    }
    IN_CHAINED.store(false, Ordering::SeqCst);
    // SAFETY: the pointers Windows passed to this filter.
    unsafe { run_handler(pointers, None) };
    HANDLING.store(0, Ordering::SeqCst);
    CONTINUE_SEARCH
}

/// The exception's code and address; zeros without a record.
///
/// # Safety
/// `pointers` must be null or the pointers Windows passed to a running filter.
unsafe fn code_and_address(pointers: *const EXCEPTION_POINTERS) -> (u32, usize) {
    if pointers.is_null() {
        return (0, 0);
    }
    // SAFETY: the caller guarantees the pointers, valid while the filter runs.
    let record = unsafe { (*pointers).ExceptionRecord };
    // SAFETY: as above; the record is null or valid.
    unsafe { record.as_ref() }.map_or((0, 0), |r| {
        (r.ExceptionCode as u32, r.ExceptionAddress as usize)
    })
}

/// Calls the handler for the crash of `pointers`.
///
/// # Safety
/// `pointers` must be null or the pointers Windows passed to a filter still running on this thread.
unsafe fn run_handler(pointers: *const EXCEPTION_POINTERS, chained_fault: Option<(u32, usize)>) {
    let handler = HANDLER.load(Ordering::SeqCst);
    if handler == 0 {
        return;
    }
    // SAFETY: as the caller guarantees.
    let (code, address) = unsafe { code_and_address(pointers) };
    // SAFETY: HANDLER holds a `Handler`, stored by `install`.
    let handler: Handler = unsafe { crate::hook::as_fn_at(handler) };
    handler(&Crash {
        code,
        address,
        chained_fault,
        pointers,
        dump_writer: load_dump_writer(),
    });
}

/// Makes the crash filter the process's unhandled-exception filter, calling `handler`.
pub fn install(handler: Handler) {
    HANDLER.store(handler as usize, Ordering::SeqCst);
    PREVIOUS.store(set_filter(), Ordering::SeqCst);
}

/// Puts the crash filter back as the process's filter when another module has set its own since (a plugin, or a
/// DLL's C runtime as it loads), and keeps that one to run before ours. Windows runs only the last filter set, so
/// one that does not pass the crash on (Universal Mod's um.dll) would otherwise replace ours: no dump, no party
/// backup, no box. Returns the other filter's address when it is a new one, or `Err` with it when every slot is
/// taken. Windows tells no one of a new filter, so the caller repeats this; two set between two calls run as the
/// later one passes the crash on, and the earlier one may run twice.
pub fn keep_first() -> Option<Result<usize, usize>> {
    let other = set_filter();
    if other == 0 || other == filter as *const () as usize {
        return None;
    }
    if CHAINED.iter().any(|s| s.load(Ordering::SeqCst) == other) {
        return None;
    }
    // Only the caller's thread adds, so the first empty slot stays empty until stored.
    Some(
        match CHAINED.iter().find(|s| s.load(Ordering::SeqCst) == 0) {
            Some(slot) => {
                slot.store(other, Ordering::SeqCst);
                Ok(other)
            }
            None => Err(other),
        },
    )
}

/// Sets the crash filter; returns the filter it replaced, or 0.
fn set_filter() -> usize {
    let new: LPTOP_LEVEL_EXCEPTION_FILTER = Some(filter);
    // SAFETY: `filter` has the filter's type and stays valid for the process.
    let previous = unsafe { SetUnhandledExceptionFilter(new) };
    previous.map_or(0, |f| f as usize)
}

/// Suspends every other running thread of the process; returns how many this pass stopped (a thread that was
/// already suspended does not count).
fn suspend_other_threads() -> usize {
    // SAFETY: no preconditions.
    let process = unsafe { GetCurrentProcessId() };
    // SAFETY: no preconditions.
    let me = unsafe { GetCurrentThreadId() };
    // SAFETY: a snapshot of the system's threads; checked below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, process) };
    if snapshot == INVALID_HANDLE_VALUE {
        return 0;
    }
    let size = size_of::<THREADENTRY32>() as u32;
    let mut entry = THREADENTRY32 {
        dwSize: size,
        cntUsage: 0,
        th32ThreadID: 0,
        th32OwnerProcessID: 0,
        tpBasePri: 0,
        tpDeltaPri: 0,
        dwFlags: 0,
    };
    let mut stopped = 0;
    // SAFETY: `entry` is sized as the API requires.
    let mut more = unsafe { Thread32First(snapshot, &mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == process && entry.th32ThreadID != me {
            // SAFETY: a thread id from the snapshot; a failure returns null.
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if !thread.is_null() {
                // SAFETY: a thread handle opened for suspension.
                let previous = unsafe { SuspendThread(thread) };
                // SAFETY: the handle is ours to close.
                unsafe { CloseHandle(thread) };
                if previous == 0 {
                    stopped += 1;
                }
            }
        }
        entry.dwSize = size;
        // SAFETY: as for `Thread32First`.
        more = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
    }
    // SAFETY: the snapshot handle is ours to close.
    unsafe { CloseHandle(snapshot) };
    stopped
}

/// `MiniDumpWriteDump`: process, process id, file, dump type, exception, user streams, callback.
type MiniDumpWriteDump = unsafe extern "system" fn(
    HANDLE,
    u32,
    HANDLE,
    MINIDUMP_TYPE,
    *const MINIDUMP_EXCEPTION_INFORMATION,
    *const c_void,
    *const c_void,
) -> i32;

/// Resolves the dump writer while other threads can still release the loader lock; the DLL stays loaded.
fn load_dump_writer() -> Result<MiniDumpWriteDump, String> {
    // SAFETY: a NUL-terminated name.
    let module = unsafe { LoadLibraryW(windows_sys::w!("dbghelp.dll")) };
    if module.is_null() {
        return Err(os_error("cannot load dbghelp.dll"));
    }
    // SAFETY: a loaded module and a NUL-terminated export name.
    let Some(f) = (unsafe { GetProcAddress(module, c"MiniDumpWriteDump".as_ptr().cast()) }) else {
        return Err(os_error("dbghelp.dll has no MiniDumpWriteDump"));
    };
    // SAFETY: dbghelp's `MiniDumpWriteDump` has this signature.
    Ok(unsafe { crate::hook::as_fn_at(f as usize) })
}

/// # Safety
/// `write` must be dbghelp's loaded export; `pointers` must be null or the pointers Windows passed to the running
/// filter, on the crashing thread.
unsafe fn write_minidump(
    write: MiniDumpWriteDump,
    path: &[u16],
    pointers: *const EXCEPTION_POINTERS,
    full: bool,
) -> Result<(), String> {
    let name: Vec<u16> = path.iter().copied().chain([0]).collect();
    // SAFETY: a NUL-terminated path; a failure returns INVALID_HANDLE_VALUE.
    let file = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_WRITE,
            0,
            core::ptr::null(),
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            core::ptr::null_mut(),
        )
    };
    if file == INVALID_HANDLE_VALUE {
        return Err(os_error("cannot create the dump file"));
    }
    // SAFETY: no preconditions.
    let thread_id = unsafe { GetCurrentThreadId() };
    let info = MINIDUMP_EXCEPTION_INFORMATION {
        ThreadId: thread_id,
        ExceptionPointers: pointers.cast_mut(),
        ClientPointers: 0,
    };
    let kind = if full {
        MiniDumpWithFullMemory
    } else {
        MiniDumpWithIndirectlyReferencedMemory
    };
    // SAFETY: no preconditions.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: no preconditions.
    let process_id = unsafe { GetCurrentProcessId() };
    let info_ptr = if pointers.is_null() {
        core::ptr::null()
    } else {
        &raw const info
    };
    // SAFETY: our own process, an open file, and the running filter's exception pointers or none.
    let written = unsafe {
        write(
            process,
            process_id,
            file,
            kind,
            info_ptr,
            core::ptr::null(),
            core::ptr::null(),
        )
    } != 0;
    let result = if written {
        Ok(())
    } else {
        Err(os_error("MiniDumpWriteDump failed"))
    };
    // SAFETY: the file handle is ours to close.
    unsafe { CloseHandle(file) };
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // No test writes a real dump: Wine's dbghelp faults inside MiniDumpWriteDump on a self-dump of the test binary.
    #[test]
    fn a_dump_to_an_unwritable_path_fails() {
        let path: Vec<u16> = "C:\\no\\such\\folder\\x.dmp".encode_utf16().collect();
        let write = load_dump_writer().unwrap();
        // SAFETY: the loaded export; no exception is passed and the call stops at the file.
        assert!(unsafe { write_minidump(write, &path, core::ptr::null(), false) }.is_err());
    }
}
