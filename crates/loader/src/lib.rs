#![warn(missing_docs, unreachable_pub, clippy::missing_panics_doc)]
#![cfg_attr(not(test), no_std)]
#![expect(
    unsafe_code,
    reason = "DllMain, the DirectInput exports, the entry-point stub and its game.exe writes are the DLL's ABI"
)]
//! `dinput.dll`: the DirectInput proxy that brings addon.dll into game.exe, a drop-in for EI starter's
//! "Addon Loader v1.1.0.0".
//!
//! DllMain checks game.exe and hooks its entry point; the hook runs before WinMain, puts the entry back, finds
//! addon.dll from the starter's registry values, loads it and starts it. The DirectInput exports forward to the
//! system's dinput.dll.
//!
//! It is built without std and imports kernel32, user32, advapi32 and msvcrt only: game.exe imports dinput.dll
//! statically, so a loader that failed to load would stop the game before anything could report why.

extern crate alloc;

mod messages;
mod paths;

use core::ffi::c_void;

use eix_engine::dll::Dll;
use eix_engine::exe;
use eix_engine::heap::ProcessHeap;
use eix_engine::{patch, registry, win};
use eix_sites::Site;

use messages::TITLE;

#[global_allocator]
static HEAP: ProcessHeap = ProcessHeap;

/// Shows the panic in an error box and ends the process: nothing may unwind into game.exe.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    win::error_box(TITLE, info);
    win::terminate_process(3)
}

/// The prebuilt `core` and `alloc` carry unwind tables that name this symbol, so a DLL without std must define it.
/// With `panic = "abort"` nothing unwinds and it is never called; if it were, it ends the process.
#[cfg(not(test))]
#[unsafe(no_mangle)]
extern "C" fn rust_eh_personality() -> ! {
    win::terminate_process(3)
}

type Hinstance = *mut c_void;
type Hresult = i32;
/// `E_FAIL`: what the exports return when the system dinput.dll cannot be used.
const E_FAIL: Hresult = 0x8000_4005_u32 as Hresult;
const DLL_PROCESS_ATTACH: u32 = 1;

type CreateAw = unsafe extern "system" fn(Hinstance, u32, *mut *mut c_void, *mut c_void) -> Hresult;
type CreateEx = unsafe extern "system" fn(
    Hinstance,
    u32,
    *const c_void,
    *mut *mut c_void,
    *mut c_void,
) -> Hresult;
type Apply = unsafe extern "C" fn();

/// RU 1.06's entry point.
const ENTRY: Site = eix_sites::ENTRY;
/// The entry point's bytes the hook replaces (`push ebp; mov ebp, esp; push -1`).
const ENTRY_BYTES: [u8; 5] = [0x55, 0x8B, 0xEC, 0x6A, 0xFF];
/// game.exe's `.text` in memory, through the end of its last page.
const TEXT: Site = eix_sites::TEXT_SECTION;
const TEXT_LEN: usize = 0x33_A000;
/// Slack after `.text`'s code where an addon without `Apply` gets a `jmp [HANDLER_SLOT]`.
const HANDLER_JMP: Site = eix_sites::TEXT_73A670;
/// Slack after `.text`'s code holding the address of the addon's `Handler`.
const HANDLER_SLOT: Site = eix_sites::TEXT_73A762;

/// Resolves the system's export on every call; the game creates DirectInput once or twice.
///
/// A failure is shown in an error box, and the caller gets `E_FAIL`: the original loader returned 1 (`S_FALSE`, a
/// success code with no interface), which would leave the game with an unset pointer.
fn system_export<F: Copy>(name: &core::ffi::CStr) -> Option<F> {
    let found = Dll::system("dinput.dll").and_then(|dll| {
        // SAFETY: every caller names an export together with its documented signature.
        unsafe { dll.proc::<F>(name) }
            .ok_or_else(|| alloc::format!("{} has no export {name:?}", dll.path()))
    });
    match found {
        Ok(f) => Some(f),
        Err(e) => {
            win::error_box(TITLE, e);
            None
        }
    }
}

/// Forwards to the system `DirectInputCreateA`.
///
/// # Safety
/// The caller passes `DirectInputCreateA`'s arguments.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DirectInputCreateA(
    inst: Hinstance,
    version: u32,
    out: *mut *mut c_void,
    outer: *mut c_void,
) -> Hresult {
    match system_export::<CreateAw>(c"DirectInputCreateA") {
        // SAFETY: the caller's arguments go to the function they were meant for.
        Some(f) => unsafe { f(inst, version, out, outer) },
        None => E_FAIL,
    }
}

/// Forwards to the system `DirectInputCreateW`.
///
/// # Safety
/// The caller passes `DirectInputCreateW`'s arguments.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DirectInputCreateW(
    inst: Hinstance,
    version: u32,
    out: *mut *mut c_void,
    outer: *mut c_void,
) -> Hresult {
    match system_export::<CreateAw>(c"DirectInputCreateW") {
        // SAFETY: the caller's arguments go to the function they were meant for.
        Some(f) => unsafe { f(inst, version, out, outer) },
        None => E_FAIL,
    }
}

/// Forwards to the system `DirectInputCreateEx`.
///
/// # Safety
/// The caller passes `DirectInputCreateEx`'s arguments.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DirectInputCreateEx(
    inst: Hinstance,
    version: u32,
    iid: *const c_void,
    out: *mut *mut c_void,
    outer: *mut c_void,
) -> Hresult {
    match system_export::<CreateEx>(c"DirectInputCreateEx") {
        // SAFETY: the caller's arguments go to the function they were meant for.
        Some(f) => unsafe { f(inst, version, iid, out, outer) },
        None => E_FAIL,
    }
}

/// The jump the hook writes over the entry point.
fn hook_bytes() -> [u8; 5] {
    patch::jmp_rel32(ENTRY, entry_stub as *const () as usize)
}

/// The entry point in the running build, set before the entry is hooked.
static ENTRY_HERE: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(ENTRY.ru());

/// Jumped to from game.exe's entry point: calls `on_entry`, then continues at the restored entry. The call clobbers
/// only the caller-saved registers, as the original loader's stub does.
#[unsafe(naked)]
extern "C" fn entry_stub() {
    core::arch::naked_asm!(
        "call {on_entry}",
        "push dword ptr [{entry}]",
        "ret",
        on_entry = sym on_entry,
        entry = sym ENTRY_HERE,
    );
}

/// Runs at game.exe's entry point, before WinMain. A failure ends the process after its error box, as the
/// original loader does: the game does not start without its addon.
extern "C" fn on_entry() {
    if let Err(text) = start_addon() {
        win::error_box(TITLE, text);
        win::exit_process(1);
    }
}

fn start_addon() -> Result<(), alloc::string::String> {
    // SAFETY: the entry point is mapped code; only this thread runs at this point of process start.
    unsafe { patch::write(ENTRY, &hook_bytes(), &ENTRY_BYTES) }
        .map_err(|e| alloc::format!("Cannot restore game.exe's entry point: {e}"))?;
    let values = paths::Values {
        starter_addon_path: registry::hkcu_string(paths::STARTER_KEY, "AddonPath"),
        settings_addon_path: registry::hkcu_string(paths::SETTINGS_KEY, "AddonPath"),
        starter_path: registry::hkcu_string(paths::STARTER_KEY, "Path"),
    };
    let candidates = paths::candidates(&values);
    let Some(path) = candidates.iter().find(|p| win::file_exists(p)) else {
        return Err(messages::not_found(&candidates));
    };
    let addon = Dll::load(path).map_err(|code| messages::load_failed(code, path))?;
    // SAFETY: addon.dll's `Apply` takes no arguments and returns nothing.
    if let Some(apply) = unsafe { addon.proc::<Apply>(c"Apply") } {
        // SAFETY: called once, before WinMain, on the main thread, as the original loader calls it.
        unsafe { apply() };
        return Ok(());
    }
    // With neither export the original writes a null slot and the game runs with no addon; so does ours.
    let handler = addon.address(c"Handler").unwrap_or(0);
    install_handler(handler)
        .map_err(|e| alloc::format!("Cannot install {}'s Handler: {e}", addon.path()))
}

/// An addon without `Apply`: the original loader leaves `jmp [HANDLER_SLOT]` at HANDLER_JMP and Handler's address
/// in the slot, and does not call it.
fn install_handler(handler: usize) -> Result<(), patch::PatchError> {
    let [s0, s1, s2, s3] = HANDLER_SLOT.ru32().to_le_bytes();
    // The original leaves `.text` writable on this path; an addon built for it may write code without unprotecting it.
    // SAFETY: `.text` is game.exe's mapped code section.
    unsafe { patch::unprotect(TEXT, TEXT_LEN) }?;
    // SAFETY: the slot lies in `.text`'s mapped slack; only this thread runs at this point of process start.
    unsafe { patch::write(HANDLER_SLOT, &[0; 4], &(handler as u32).to_le_bytes()) }?;
    // SAFETY: as above.
    unsafe { patch::write(HANDLER_JMP, &[0; 6], &[0xFF, 0x25, s0, s1, s2, s3]) }
}

/// Checks game.exe and hooks its entry point.
fn attach() -> Result<(), alloc::string::String> {
    let id = exe::current().map_err(|e| alloc::format!("Invalid version of game.exe!\n\n{e}"))?;
    let Some(build) = exe::identify(&id) else {
        return Err(messages::invalid_version(&id));
    };
    eix_sites::select(build);
    ENTRY_HERE.store(ENTRY.here(), core::sync::atomic::Ordering::Relaxed);
    // SAFETY: the entry point is mapped code that has not run yet; the loader runs DllMain on this one thread.
    unsafe { patch::write(ENTRY, &ENTRY_BYTES, &hook_bytes()) }
        .map_err(|e| alloc::format!("Cannot hook game.exe's entry point: {e}"))
}

/// The DLL entry point. A game.exe the loader does not know gets an error box, and the load fails, which stops the
/// game, as with the original loader.
///
/// # Safety
/// Called by the Windows loader only.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(
    module: Hinstance,
    reason: u32,
    _reserved: *mut c_void,
) -> i32 {
    if reason != DLL_PROCESS_ATTACH {
        return 1;
    }
    // SAFETY: `module` is our own handle, as the Windows loader passes it to DllMain.
    unsafe { win::disable_thread_library_calls(module) };
    match attach() {
        Ok(()) => 1,
        Err(text) => {
            win::error_box(TITLE, text);
            0
        }
    }
}
