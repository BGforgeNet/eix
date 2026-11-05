//! FixProgressNames: `SelectLoadingMovie` 0x595080 gives the allods gipat2 and jigran their own loading movies
//! (core's [`loading_movie`]) where it would pick progres.bik.
#![expect(
    unsafe_code,
    reason = "reading the allod's name from the game's stack and storing the movie's name in its global"
)]

use eix_core::fixes::loading_movie;
use eix_engine::game::media::LOADING_MOVIE;
use eix_engine::hook::{self, Context, Mode};

use super::arg_path::c_str;
use super::{BuildError, Labelled, Setup};

/// `mov dword ptr [0x791EE0], 0x790A70`: the movie becomes "progres.bik", the choice for any other allod.
const SITE: usize = eix_sites::SELECT_LOADING_MOVIE__B0;
const ORIGINAL: [u8; 10] = [0xC7, 0x05, 0xE0, 0x1E, 0x79, 0x00, 0x70, 0x0A, 0x79, 0x00];

extern "C" fn handler(_: usize, ctx: &mut Context) {
    // Stack word 1 is the function's local CString holding the allod's name.
    let Some(word) = ctx.stack_word(1) else {
        return;
    };
    // SAFETY: at 0x595130 the word is the characters of a constructed CString that lives for the function.
    let Some(name) = (unsafe { c_str(word) }) else {
        return;
    };
    if let Some(movie) = loading_movie(name.to_bytes()) {
        // SAFETY: the build is RU 1.06, on the game's thread; the name is a static string, as the game's own are.
        unsafe { LOADING_MOVIE.write(movie.as_ptr().cast()) };
    }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "FixProgressNames #0".to_owned(),
        write: hook::context_hook(SITE, &ORIGINAL, Mode::Before, handler, 0)?,
    }])
}
