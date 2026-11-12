//! EI_Plugin.asi detection: which `.asi` files in the game folder are EI_Plugin builds, and whether the addon
//! supports them. An unsupported one turns the "moved patches" (table T3) off, as addon.dll 0.12.4.2 does in its
//! `*.asi` scan.

/// What one `.asi` file in the game folder is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsiFile {
    /// Not an EI_Plugin build; it does not affect table T3.
    Other,
    /// An EI_Plugin build the addon supports: table T3 stays on.
    Supported,
    /// An EI_Plugin build of another version: table T3 is off.
    Unsupported,
}

/// The checksum a supported EI_Plugin.asi gives over its first 0x125 bytes without byte 0x10E.
pub const SUPPORTED_CHECKSUM: u32 = 0x3A22_0CA3;

/// The checksum's table: a CRC-32 table whose polynomial step runs on a clear low bit instead of a set one, and
/// whose entries have their top byte flipped. Reproduced as is, since the supported value depends on it.
const fn table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut bit = 0;
        while bit < 8 {
            let step = if c & 1 == 0 { 0xEDB8_8320 } else { 0 };
            c = (c >> 1) ^ step;
            bit += 1;
        }
        t[i] = c ^ 0xFF00_0000;
        i += 1;
    }
    t
}

static TABLE: [u32; 256] = table();

/// The addon's checksum of `data`, continuing from `crc` (0 to start).
pub fn checksum(data: &[u8], crc: u32) -> u32 {
    data.iter()
        .fold(crc, |c, &b| (c >> 8) ^ TABLE[usize::from((c as u8) ^ b)])
}

/// Whether a file name, lowered, names an EI_Plugin (it holds `ei` and `plugin`); an unreadable file with such a
/// name turns table T3 off.
pub fn name_is_plugin(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("ei") && lower.contains("plugin")
}

/// Whether `data` holds `name` followed by a NUL at some offset below `len - 0x14`, compared ignoring ASCII case.
fn holds_name(data: &[u8], name: &[u8]) -> bool {
    let end = data.len().saturating_sub(0x14);
    (0..end).any(|i| {
        data.get(i..i + name.len() + 1)
            .is_some_and(|w| w[..name.len()].eq_ignore_ascii_case(name) && w[name.len()] == 0)
    })
}

/// Classifies a `.asi` file by its contents: a PE image of at least 0x200 bytes that names `EI_Plugin.ini` or
/// `EI_Plugin.asi` is an EI_Plugin build. It is supported when it is larger than 64 KiB and either ends in
/// `addon` or has [`SUPPORTED_CHECKSUM`].
pub fn classify(data: &[u8]) -> AsiFile {
    if data.len() < 0x200 || !data.starts_with(b"MZ") {
        return AsiFile::Other;
    }
    if !holds_name(data, b"EI_Plugin.ini") && !holds_name(data, b"EI_Plugin.asi") {
        return AsiFile::Other;
    }
    if data.len() <= 0x1_0000 {
        return AsiFile::Unsupported;
    }
    if data.ends_with(b"addon") {
        return AsiFile::Supported;
    }
    let crc = checksum(&data[0x10F..0x125], checksum(&data[..0x10E], 0));
    if crc == SUPPORTED_CHECKSUM {
        AsiFile::Supported
    } else {
        AsiFile::Unsupported
    }
}

/// The version an `[MOD] AddonVersion` value names, as the addon parses it: digits and at most three
/// dots, never two in a row nor one first; missing parts are 0. `None` for a value the addon rejects.
pub fn parse_version(text: &[u8]) -> Option<[i32; 4]> {
    let mut dots = 0;
    for (i, &c) in text.iter().enumerate() {
        match c {
            b'0'..=b'9' => {}
            b'.' if i > 0 && text[i - 1] != b'.' && dots < 3 => dots += 1,
            _ => return None,
        }
    }
    let mut parts = [0i32; 4];
    for (slot, part) in parts.iter_mut().zip(text.split(|&c| c == b'.')) {
        // The original reads each part with `sscanf("%d")`; a part past i32's range (never a real version) is held
        // at i32::MAX here.
        let value = part.iter().fold(0i64, |v, &d| {
            (v * 10 + i64::from(d - b'0')).min(i64::from(i32::MAX))
        });
        *slot = value as i32;
        if part.is_empty() {
            break;
        }
    }
    Some(parts)
}

/// The addon version this addon stands in for: a mod whose `AddonVersion` is newer stops the game ("Addon v%s or
/// newer is required to run this mod").
pub const ADDON_VERSION: [i32; 4] = [0, 12, 4, 2];

/// [`ADDON_VERSION`] as the addon prints it: `0.12.4.2`.
pub fn addon_version_text() -> String {
    ADDON_VERSION.map(|n| n.to_string()).join(".")
}

/// What `[MOD] AddonVersion` asks, as the original's option reader checks it: `Err` with the
/// text of the box that stops the game, for a value it cannot parse or a version newer than [`ADDON_VERSION`].
pub fn check_addon_version(text: &[u8]) -> Result<(), String> {
    match parse_version(text) {
        None => Err(format!(
            "Invalid AddonVersion: \"{}\"",
            String::from_utf8_lossy(text)
        )),
        Some(v) if v > ADDON_VERSION => Err(format!(
            "Addon v{}.{}.{}.{} or newer is required to run this mod",
            v[0], v[1], v[2], v[3]
        )),
        Some(_) => Ok(()),
    }
}

/// The oldest mod requirement for which an unsupported EI_Plugin stops the game ("Incompatible (old) version of
/// EI_Plugin found!") instead of turning table T3 off: a mod that needs addon 0.10 or newer.
pub const PLUGIN_STRICT_SINCE: [i32; 4] = [0, 10, 0, 0];

#[cfg(test)]
mod tests {
    use super::*;

    /// A PE-shaped buffer of `len` bytes naming the plugin's INI at 0x200.
    fn plugin(len: usize) -> Vec<u8> {
        let mut d = vec![0u8; len];
        d[..2].copy_from_slice(b"MZ");
        d[0x200..0x20D].copy_from_slice(b"ei_plugin.INI");
        d
    }

    #[test]
    fn the_checksum_table_matches_the_addons() {
        assert_eq!(TABLE[0], 0xD202_EF8D);
        assert_eq!(TABLE[1], 0xA505_DF1B);
        assert_eq!(checksum(b"", 0), 0);
        assert_eq!(checksum(b"EI_Plugin", 0), 0xCE43_917F);
    }

    #[test]
    fn plugins_are_found_by_the_names_they_hold() {
        assert_eq!(classify(&[0; 0x300]), AsiFile::Other);
        assert_eq!(classify(&plugin(0x300)), AsiFile::Unsupported);
        let mut other = plugin(0x300);
        other[0x20D] = b'x';
        assert_eq!(classify(&other), AsiFile::Other);
        let mut big = plugin(0x2_0000);
        assert_eq!(classify(&big), AsiFile::Unsupported);
        let n = big.len();
        big[n - 5..].copy_from_slice(b"addon");
        assert_eq!(classify(&big), AsiFile::Supported);
        assert!(name_is_plugin("EI_Plugin.asi"));
        assert!(name_is_plugin("my_EIplugin2.ASI"));
        assert!(!name_is_plugin("mp3dec.asi"));
    }

    #[test]
    fn versions_parse_as_the_addon_parses_them() {
        assert_eq!(parse_version(b""), Some([0; 4]));
        assert_eq!(parse_version(b"0.12.4.2"), Some([0, 12, 4, 2]));
        assert_eq!(parse_version(b"1.2"), Some([1, 2, 0, 0]));
        assert_eq!(parse_version(b"1."), Some([1, 0, 0, 0]));
        assert_eq!(parse_version(b".1"), None);
        assert_eq!(parse_version(b"1..2"), None);
        assert_eq!(parse_version(b"1.2.3.4.5"), None);
        assert_eq!(parse_version(b"1.2a"), None);
        assert!(parse_version(b"0.10").is_some_and(|v| v >= PLUGIN_STRICT_SINCE));
        assert!(parse_version(b"0.9.9.9").is_some_and(|v| v < PLUGIN_STRICT_SINCE));
        assert_eq!(check_addon_version(b""), Ok(()));
        assert_eq!(check_addon_version(b"0.12.4.2"), Ok(()));
        assert_eq!(
            check_addon_version(b"0.12.5"),
            Err("Addon v0.12.5.0 or newer is required to run this mod".to_owned())
        );
        assert_eq!(
            check_addon_version(b"1.x"),
            Err("Invalid AddonVersion: \"1.x\"".to_owned())
        );
    }
}
