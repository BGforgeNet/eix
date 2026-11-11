//! EmptyMusicTheme: a music theme whose track list has no names plays nothing, where the game divided by zero.
//! `CMusicSystem::UpdateMusic` 0x6B5350 picks a track of the theme's list at random, `div ebp` (0x6B54F0) by the
//! list's count (+0x2C); it tests the count against 1 (0x6B54B7) but not against 0, while the other list's pick is
//! guarded (0x6B55C5). The shipped `music.reg` has no empty list; a mod's can. Ours skips the pick and the track change
//! when the count is 0, as that guard does (0x6B568B).

use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use super::Warnings;
use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// `cmp ebp, 1; jne 0x6B54C2`: EBP the list's count.
const COUNT_TEST: Site = eix_sites::C_MUSIC_SYSTEM_UPDATE_MUSIC__167;
const COUNT_TEST_BYTES: [u8; 5] = [0x83, 0xFD, 0x01, 0x75, 0x06];
/// The one name's pick, the random pick, and the end of the update.
const ONE_NAME: Site = eix_sites::C_MUSIC_SYSTEM_UPDATE_MUSIC__16C;
const RANDOM_NAME: Site = eix_sites::C_MUSIC_SYSTEM_UPDATE_MUSIC__172;
const DONE: Site = eix_sites::C_MUSIC_SYSTEM_UPDATE_MUSIC__33B;

/// The update runs every frame while the theme stays.
static WARNINGS: Warnings = Warnings::new("EmptyMusicTheme");

extern "C" fn count_test(_: usize, ctx: &mut Context) {
    ctx.resume = match ctx.ebp {
        0 => {
            WARNINGS.warn(|| "a music theme's track list has no names: no track".to_owned());
            DONE
        }
        1 => ONE_NAME,
        _ => RANDOM_NAME,
    };
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "EmptyMusicTheme #0".to_owned(),
        write: hook::context_hook(COUNT_TEST, &COUNT_TEST_BYTES, Mode::Drop, count_test, 0)?,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_branches_go_where_the_jne_went() {
        assert_eq!(ONE_NAME, COUNT_TEST.offset(COUNT_TEST_BYTES.len()));
        assert_eq!(
            RANDOM_NAME,
            ONE_NAME.offset(usize::from(COUNT_TEST_BYTES[4]))
        );
    }
}
