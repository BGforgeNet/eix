//! Where to look for addon.dll, from the starter's registry values, in the original loader's order.
#![forbid(unsafe_code)]

use alloc::vec::Vec;

/// `HKCU` key of the starter's own values.
pub(crate) const STARTER_KEY: &str = "Software\\Gipat.Ru\\EI_Starter";
/// `HKCU` key of the addon's settings.
pub(crate) const SETTINGS_KEY: &str = "Software\\Gipat.Ru\\EI_Starter\\Settings";

const ADDON_DLL: &str = "addon.dll";

/// The registry values the candidates come from, as UTF-16; empty when not set.
#[derive(Debug, Default)]
pub(crate) struct Values {
    /// `EI_Starter\AddonPath`: a path to addon.dll itself, used as it is.
    pub(crate) starter_addon_path: Vec<u16>,
    /// `EI_Starter\Settings\AddonPath`: the selected mod's folder.
    pub(crate) settings_addon_path: Vec<u16>,
    /// `EI_Starter\Path`: the starter's folder.
    pub(crate) starter_path: Vec<u16>,
}

/// The addon.dll paths to try, in order; a value that is not set adds none.
pub(crate) fn candidates(values: &Values) -> Vec<Vec<u16>> {
    let in_folder = |folder: &[u16]| {
        let mut path = folder.to_vec();
        if path.last() != Some(&u16::from(b'\\')) {
            path.push(u16::from(b'\\'));
        }
        path.extend(ADDON_DLL.encode_utf16());
        path
    };
    [
        (!values.starter_addon_path.is_empty()).then(|| values.starter_addon_path.clone()),
        (!values.settings_addon_path.is_empty()).then(|| in_folder(&values.settings_addon_path)),
        (!values.starter_path.is_empty()).then(|| in_folder(&values.starter_path)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn found(values: &Values) -> Vec<String> {
        candidates(values)
            .iter()
            .map(|p| String::from_utf16(p).unwrap())
            .collect()
    }

    #[test]
    fn all_three_in_the_original_order() {
        let values = Values {
            starter_addon_path: w("D:\\addon\\addon.dll"),
            settings_addon_path: w("C:\\EIStarter\\Mods\\inv"),
            starter_path: w("C:\\EIStarter\\"),
        };
        assert_eq!(
            found(&values),
            [
                "D:\\addon\\addon.dll",
                "C:\\EIStarter\\Mods\\inv\\addon.dll",
                "C:\\EIStarter\\addon.dll",
            ]
        );
    }

    #[test]
    fn values_not_set_add_no_candidate() {
        let values = Values {
            starter_path: w("C:\\EIStarter"),
            ..Values::default()
        };
        assert_eq!(found(&values), ["C:\\EIStarter\\addon.dll"]);
        assert!(candidates(&Values::default()).is_empty());
    }

    #[test]
    fn the_starter_addon_path_is_taken_as_it_is() {
        let values = Values {
            starter_addon_path: w("C:\\EIStarter\\Mods\\inv"),
            ..Values::default()
        };
        assert_eq!(found(&values), ["C:\\EIStarter\\Mods\\inv"]);
    }
}
