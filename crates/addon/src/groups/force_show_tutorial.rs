//! ForceShowTutorial (`[MOD] ForceShowTutorial`): the "show tutorial" option keeps the value the game starts with.
//! Its two stores, from the registry (`GameOptions::LoadFromRegistry`, 0x45C028) and from the options window
//! (`COptionsWindow::Apply`, 0x652663), become calls of a bare `ret`.

use eix_engine::exec;
use eix_engine::group::SiteWrite;

use super::{BuildError, Labelled, Setup};

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let ret = exec::place(1, |_| Ok::<_, String>(vec![0xC3]))?;
    let call = |label: &str, site: usize, original: &[u8]| -> Result<Labelled, BuildError> {
        Ok(Labelled {
            label: label.to_owned(),
            write: SiteWrite::call(site, original, ret)?,
        })
    };
    Ok(vec![
        call(
            "ShowTutorial #1",
            eix_sites::GAME_OPTIONS_LOAD_FROM_REGISTRY__E18,
            &[0x89, 0x86, 0x08, 0x02, 0x00, 0x00],
        )?,
        call(
            "ShowTutorial #2",
            eix_sites::C_OPTIONS_WINDOW_APPLY__1C3,
            &[0x89, 0x15, 0x78, 0xBE, 0x79, 0x00],
        )?,
    ])
}
