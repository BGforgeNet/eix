//! WideScreen: in a mode wider than 4:3 the 800x600 interface is drawn in the 4:3 part in the middle, and the
//! battle screen's panels move out to the screen's edges (behaviour from ei-rebuild's
//! notes, MIT).
//!
//! The layout (core's [`Layout`]) is computed once, before WinMain, from `VideoResolution`. Its sites, numbered as
//! `WideScreen #n`:
//! - [`scaling`]: layout to screen and back with the margin, the places scaled without it, the text extent, the
//!   global map's pick, the fonts, the projection, the backdrop;
//! - [`shapes`]: regions and sprites moved by the panel offset;
//! - [`panels`]: the panel builders that set the panel offset, and the layout constants they move by `extra`;
//! - [`screens`]: the camp's and the hero screen's curtains, and the tutorial frames.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicI32, Ordering};

use eix_core::widescreen::{Layout, Mode};
use eix_engine::group::SiteWrite;
use eix_sites::Site;

use super::{BuildError, Labelled, Setup};

mod panels;
mod scaling;
mod screens;
mod shapes;

/// The layout, set by [`build`] before any site is written.
static LAYOUT: OnceLock<Layout> = OnceLock::new();

/// An 800x600 mode's layout, the identity scaling; only a handler running before [`build`] could see it, and none
/// can, since the sites are written after the layout is set.
const IDENTITY: Layout = Layout {
    width: 800,
    height: 600,
    width43: 800,
    margin: 0,
    extra: 0,
};

fn layout() -> Layout {
    LAYOUT.get().copied().unwrap_or(IDENTITY)
}

/// The panel offset: `-extra` or `extra` while one of the battle screen's panel builders runs, 0 otherwise. Regions
/// and sprites made meanwhile move along x by it. The game's thread alone uses it.
static PANEL_OFFSET: AtomicI32 = AtomicI32::new(0);

fn panel_offset() -> i32 {
    PANEL_OFFSET.load(Ordering::Relaxed)
}

fn set_panel_offset(offset: i32) {
    PANEL_OFFSET.store(offset, Ordering::Relaxed);
}

/// A site write and its number in the `WideScreen #n` labels.
type Numbered = (usize, SiteWrite);

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let layout = match Mode::from_resolution(setup.options.settings.video_resolution.as_deref()) {
        Mode::Wide(l) => l,
        Mode::Invalid(message) => return Err(BuildError::from(message)),
        // The enabling predicate turns these off before the group is built.
        Mode::NotWide(reason) => return Err(BuildError::from(reason)),
    };
    let set = *LAYOUT.get_or_init(|| layout);
    if set != layout {
        return Err(BuildError::from(format!(
            "the layout is already set for {}x{}",
            set.width, set.height
        )));
    }
    let mut sites: Vec<Numbered> = Vec::new();
    sites.extend(shapes::build()?);
    sites.extend(scaling::build(&layout)?);
    sites.extend(panels::build(&layout)?);
    sites.extend(screens::build(&layout)?);
    sites.sort_by_key(|(n, _)| *n);
    Ok(sites
        .into_iter()
        .map(|(n, write)| Labelled {
            label: format!("WideScreen #{n}"),
            write,
        })
        .collect())
}

/// `new` over the 4 bytes of `old` at `site` (an immediate, a float or a rel32 operand).
fn word(site: Site, old: [u8; 4], new: [u8; 4]) -> Result<SiteWrite, BuildError> {
    Ok(SiteWrite::bytes(site, &old, &new)?)
}

/// Raw bytes of the same length.
fn bytes(site: Site, old: &[u8], new: &[u8]) -> Result<SiteWrite, BuildError> {
    Ok(SiteWrite::bytes(site, old, new)?)
}

#[cfg(test)]
mod tests;
