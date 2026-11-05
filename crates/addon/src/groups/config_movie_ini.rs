//! ConfigMovieIni: the movie list comes from the mod's movie.ini (`[CONFIG] movie.ini`): the call of the INI reader
//! 0x695BF0 at 0x4582DB gets the mod's path as its argument.

use super::{BuildError, Labelled, Setup, mod_file, path_as_first_arg};

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let path = mod_file(setup, &setup.options.config.config.movie_ini)?;
    Ok(vec![path_as_first_arg(
        "Movie.ini",
        eix_sites::PLAY_MOVIES__6B,
        [0xE8, 0x10, 0xD9, 0x23, 0x00],
        path,
    )?])
}
