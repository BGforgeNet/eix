//! LoadFailureLeaks: a file or texture that fails to load frees what its load took. `CMappedResource::GetFile`
//! 0x671B40 deletes the file's node when its seek or read fails (0x671DC3) but not the data buffer it allocated for it
//! (+0x0C, 0x671D0A); `CTextureManager::LoadTexture` 0x6922A0 gives the opened resource back (slot +0x50, 0x69250D)
//! only on success, not when the header is unknown (0x692536); `LoadTextureFrom` 0x6926F0 frees its buffer (0x692835)
//! only on success, not on the same failure (0x692863). Both texture failures go on to `FatalError`, which returns
//! under AntiCrash. Ours frees each at its failure path.
#![expect(
    unsafe_code,
    reason = "frees the game's buffers and gives its resource back on the game's failure paths"
)]

use core::ffi::c_void;

use eix_engine::game::mfc;
use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// GetFile: `mov ecx, esi; mov [esp+0x3c], 0xa` before the node's destruction, ESI the node.
const GET_FILE_FAILED: Site = eix_sites::MAPPED_RESOURCE_GET_FILE__28B;
const GET_FILE_FAILED_BYTES: [u8; 10] =
    [0x8B, 0xCE, 0xC7, 0x44, 0x24, 0x3C, 0x0A, 0x00, 0x00, 0x00];
/// The node's data buffer.
const NODE_DATA: usize = 0x0C;
/// LoadTexture and LoadTextureFrom: `mov eax, [ebp]; mov ecx, ebp` before the message, EBX the manager; ESI the
/// resource in LoadTexture, EBX the buffer in LoadTextureFrom.
const TEXTURE_FAILED_BYTES: [u8; 5] = [0x8B, 0x45, 0x00, 0x8B, 0xCD];
const LOAD_TEXTURE_FAILED: Site = eix_sites::TEXTURE_MANAGER_LOAD_TEXTURE__296;
const LOAD_TEXTURE_FROM_FAILED: Site = eix_sites::TEXTURE_MANAGER_LOAD_TEXTURE_FROM__173;
/// The manager's virtual slot that gives a resource back, as the success path calls it (`call [edx+0x50]`).
const RELEASE_SLOT: usize = 0x50;

type Release = unsafe extern "thiscall" fn(*mut c_void, *mut c_void);

extern "C" fn get_file_failed(_: usize, ctx: &mut Context) {
    if ctx.esi == 0 {
        return;
    }
    let data = core::ptr::with_exposed_provenance_mut::<*mut c_void>(ctx.esi as usize + NODE_DATA);
    // SAFETY: ESI is the node GetFile allocated.
    let buffer = unsafe { data.read() };
    // SAFETY: the node's buffer, from the game's `operator new` and freed nowhere else.
    unsafe { mfc::delete(buffer) };
    // SAFETY: as the read; the node is destroyed next, so nothing reads the pointer again.
    unsafe { data.write(core::ptr::null_mut()) };
}

extern "C" fn load_texture_failed(_: usize, ctx: &mut Context) {
    let manager = core::ptr::with_exposed_provenance_mut::<*const usize>(ctx.ebx as usize);
    // SAFETY: EBX is the texture manager, an object with a vtable.
    let vtable = unsafe { manager.read() };
    // SAFETY: its slot +0x50, which the success path calls.
    let slot = unsafe { vtable.wrapping_byte_add(RELEASE_SLOT).read() };
    // SAFETY: that slot is thiscall with the resource, as the success path calls it.
    let release = unsafe { hook::as_fn_at::<Release>(slot) };
    let resource = core::ptr::with_exposed_provenance_mut(ctx.esi as usize);
    // SAFETY: ESI is the resource the manager's slot +0x4C opened, given back once.
    unsafe { release(manager.cast(), resource) };
}

extern "C" fn load_texture_from_failed(_: usize, ctx: &mut Context) {
    // SAFETY: EBX is the buffer LoadTextureFrom allocated with the game's `operator new`, freed nowhere else.
    unsafe { mfc::delete(core::ptr::with_exposed_provenance_mut(ctx.ebx as usize)) };
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let sites: [(Site, &[u8], hook::Handler); 3] = [
        (GET_FILE_FAILED, &GET_FILE_FAILED_BYTES, get_file_failed),
        (
            LOAD_TEXTURE_FAILED,
            &TEXTURE_FAILED_BYTES,
            load_texture_failed,
        ),
        (
            LOAD_TEXTURE_FROM_FAILED,
            &TEXTURE_FAILED_BYTES,
            load_texture_from_failed,
        ),
    ];
    let mut writes = Vec::with_capacity(sites.len());
    for (i, (site, bytes, handler)) in sites.into_iter().enumerate() {
        writes.push(Labelled {
            label: format!("LoadFailureLeaks #{i}"),
            write: hook::context_hook(site, bytes, Mode::Before, handler, 0)?,
        });
    }
    Ok(writes)
}
