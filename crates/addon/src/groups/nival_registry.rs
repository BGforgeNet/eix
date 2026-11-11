//! NivalRegistry: WinMain's registry root becomes the starter's tree, so the game reads and writes its settings under
//! `HKCU\Software\Gipat.ru\EI_Starter\EvilIslands` (its `push` operand at 0x4568EA, the string 0x7848A8 before).

use eix_engine::group::SiteWrite;
use eix_sites::Site;

use super::{BuildError, Labelled, Setup};

/// The operand of `push 0x7848A8` at 0x4568E9 in WinMain, the root CString's text.
const SITE: Site = eix_sites::TEXT_4568EA;
const GAME_ROOT: Site = eix_sites::DATA_7848A8;
static STARTER_ROOT: &std::ffi::CStr = c"Software\\Gipat.ru\\EI_Starter\\EvilIslands";

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "Set new Nival Registry path".to_owned(),
        write: SiteWrite::pointer(SITE, GAME_ROOT.ru(), STARTER_ROOT.as_ptr() as usize),
    }])
}
