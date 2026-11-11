//! ObjectsDrawRadius and LandscapeDrawRadius: the client draws objects and terrain out to the registry's radius
//! instead of 100. `CWorldClient::DrawPass` 0x56AFE0 reads the object radius from the float 0x73BAB0 four times, which
//! now point at the setting; `CWorldClient::Render` 0x569E40 pushes the terrain radius as an immediate twice.

use std::sync::atomic::{AtomicU32, Ordering};

use eix_engine::group::SiteWrite;
use eix_sites::Site;

use super::{BuildError, Labelled, Setup};

/// The float 100.0 the object pass reads.
const OBJECTS_RADIUS: Site = eix_sites::RDATA_73BAB0;
/// The `fadd`/`fsub dword ptr [0x73BAB0]` operands.
const OBJECTS_SITES: [Site; 4] = [
    eix_sites::C_WORLD_CLIENT_DRAW_PASS__10A,
    eix_sites::C_WORLD_CLIENT_DRAW_PASS__15C,
    eix_sites::C_WORLD_CLIENT_DRAW_PASS__19A,
    eix_sites::C_WORLD_CLIENT_DRAW_PASS__201,
];
/// The `push 100.0` operands.
const LANDSCAPE_SITES: [Site; 2] = [
    eix_sites::C_WORLD_CLIENT_RENDER__1ED,
    eix_sites::C_WORLD_CLIENT_RENDER__279,
];

/// The registry's DWORD as the float the game reads, unsigned as the original converts it.
fn radius(setting: u32) -> f32 {
    setting as f32
}

/// The float the four operands point at, set before they are written; the game reads it as an `f32`.
static OBJECTS_VALUE: AtomicU32 = AtomicU32::new(0);

pub(super) fn build_objects(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let value = radius(setup.options.settings.objects_draw_radius);
    OBJECTS_VALUE.store(value.to_bits(), Ordering::SeqCst);
    let to = OBJECTS_VALUE.as_ptr().expose_provenance();
    Ok(OBJECTS_SITES
        .iter()
        .enumerate()
        .map(|(i, &site)| Labelled {
            label: format!("ObjectsDrawRadius #{}", i + 1),
            write: SiteWrite::pointer(site, OBJECTS_RADIUS.ru(), to),
        })
        .collect())
}

pub(super) fn build_landscape(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let value = radius(setup.options.settings.landscape_draw_radius).to_le_bytes();
    LANDSCAPE_SITES
        .iter()
        .enumerate()
        .map(|(i, &site)| {
            Ok(Labelled {
                label: format!("LandscapeDrawRadius #{}", i + 1),
                write: SiteWrite::bytes(site, &100.0f32.to_le_bytes(), &value)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_converts_unsigned() {
        assert_eq!(radius(200), 200.0);
        assert_eq!(radius(5376), 5376.0);
        assert_eq!(radius(u32::MAX), 4_294_967_296.0);
    }
}
