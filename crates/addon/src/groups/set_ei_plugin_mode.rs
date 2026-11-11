//! SetEiPluginMode: the mode word EI_Plugin.asi reads at 0x73AF10. 0xAADD marks it as the addon's; bit 7 turns the
//! plugin's own script commands off, since AddNewScriptCommands and PlayerNumbers stand in for them.

use eix_engine::group::SiteWrite;
use eix_sites::Site;

use super::{BuildError, Labelled, Setup};

const SITE: Site = eix_sites::TEXT_73AF10;
const MODE: u32 = 0xAADD_0080;

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "EI_Plugin mode".to_owned(),
        write: SiteWrite::bytes(SITE, &[0; 4], &MODE.to_le_bytes())?,
    }])
}
