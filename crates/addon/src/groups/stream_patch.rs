//! StreamPatch: a streamed sound comes from the mod's `Stream\` folder when the mod has the file. `SwitchStream`
//! joins its folder and the file name by the call of `CString operator+` 0x700B9F at 0x6BB628 (result, folder, name
//! as stack words 1 to 3); the hook gives it the mod's folder.
#![expect(
    unsafe_code,
    reason = "building a game CString and reading the one the game passes"
)]

use std::sync::OnceLock;

use eix_engine::game::mfc::GameString;
use eix_engine::hook::{self, Context};

use super::{BuildError, Labelled, Setup, game_text};
use crate::setup::path;

/// The mod's `Stream\` folder: the text, and the address of the game string holding it.
static FOLDER: OnceLock<(Vec<u8>, usize)> = OnceLock::new();

extern "C" fn handler(_: usize, ctx: &mut Context) {
    let Some((folder, string)) = FOLDER.get() else {
        return;
    };
    let Some(name) = ctx.stack_word(3) else {
        return;
    };
    let name = core::ptr::with_exposed_provenance::<GameString>(name as usize);
    if name.is_null() {
        return;
    }
    // SAFETY: at 0x6BB628 the third argument points at the stream's file name, a constructed game string that lives
    // for the call (`SwitchStream`'s own).
    let name = unsafe { &*name };
    // SAFETY: as above: a constructed string, unchanged during the call.
    let bytes = unsafe { name.as_bytes() };
    if bytes.is_empty() || !path(&[&folder[..], bytes].concat()).is_file() {
        return;
    }
    ctx.set_stack_word(2, *string as u32);
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let folder = [&setup.mod_dir[..], b"Stream\\"].concat();
    let address = game_text::game_string(folder.clone())?;
    let _ = FOLDER.set((folder, address));
    let (write, _) = hook::call_hook(
        eix_sites::C_SOUND_MANAGER_READ_STREAM_VANILLA__288,
        [0xE8, 0x72, 0x55, 0x04, 0x00],
        handler,
        0,
    )?;
    Ok(vec![Labelled {
        label: "StreamPatch".to_owned(),
        write,
    }])
}
