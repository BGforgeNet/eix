//! The installed encumbrance settings shared by the movement and Lua running-cost hooks.

use std::sync::OnceLock;

use eix_core::encumbrance::{Settings, encumbrance};

pub(crate) static SETTINGS: OnceLock<Settings> = OnceLock::new();

/// What a running unit's drain is multiplied by at `load` of `capacity`: 1 while `[Encumbrance]` is off.
pub(crate) fn drain_multiplier(load: f32, capacity: f32) -> f64 {
    SETTINGS
        .get()
        .map_or(1.0, |s| s.drain_multiplier(encumbrance(load, capacity)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_multiplier_until_installed() {
        assert_eq!(drain_multiplier(500.0, 312.0), 1.0);
    }
}
