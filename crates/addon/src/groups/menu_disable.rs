//! MenuMultiDisable and MenuSingleDisable (`[MOD] Multi=0`, `Single=0`): the main menu's multiplayer or
//! single-player entries do nothing. The entries' cases in the menu's click and hover jump tables
//! (`InputForward<&CMainMenu::OnLButtonDown>` and `<&CMainMenu::OnMouseMove>`) point at the tables' do-nothing
//! cases, 0x6331FA and 0x6334E6.

use super::{BuildError, Labelled, Setup, pointer};

const CLICK_NOTHING: usize = eix_sites::TEXT_6331FA;
const HOVER_NOTHING: usize = eix_sites::TEXT_6334E6;

pub(super) fn multi(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![
        pointer(
            "Disable Menu (Multi) #1",
            eix_sites::TEXT_633234,
            eix_sites::TEXT_6330A9,
            CLICK_NOTHING,
        ),
        pointer(
            "Disable Menu (Multi) #2",
            eix_sites::TEXT_63353C,
            eix_sites::TEXT_6334B1,
            HOVER_NOTHING,
        ),
    ])
}

pub(super) fn single(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![
        pointer(
            "Disable Menu (Single) #1",
            eix_sites::TEXT_633220,
            eix_sites::TEXT_632DB4,
            CLICK_NOTHING,
        ),
        pointer(
            "Disable Menu (Single) #2",
            eix_sites::TEXT_633224,
            eix_sites::TEXT_632E2E,
            CLICK_NOTHING,
        ),
        pointer(
            "Disable Menu (Single) #3",
            eix_sites::TEXT_633528,
            eix_sites::TEXT_63347A,
            HOVER_NOTHING,
        ),
        pointer(
            "Disable Menu (Single) #4",
            eix_sites::TEXT_63352C,
            eix_sites::TEXT_633485,
            HOVER_NOTHING,
        ),
    ])
}
