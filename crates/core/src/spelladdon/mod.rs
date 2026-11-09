//! spelladdon.ini keys, built-in defaults and conversions.
//!
//! - Flags keep the low byte of `GetPrivateProfileIntA`, compared signed: 256 is 0, 255 is -1.
//! - Integers keep the 32-bit value or its low byte, as the module stores each.
//! - String keys are read into a buffer that holds the debug build's stack filler (0xCC) after the string's NUL,
//!   then converted by `atof`, `atoi` (frame integers) or the digit parser ([`number`]).
//!
//! The reads go through a [`Profile`]: in the game, the system's `GetPrivateProfile*A`, so the file's parsing
//! (duplicate keys, case, white space around names) is the system's, as for the original.

use std::ops::RangeInclusive;

mod keys;
pub mod number;

pub use keys::KEYS;

/// How an integer key is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    /// The low byte.
    Byte,
    /// The 32-bit value.
    Dword,
}

/// What the module does with a string key's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conv {
    /// `atof`, stored as a 32-bit float.
    Float,
    /// The digit parser, stored as a 64-bit float.
    Digits,
    /// Both `atof` (a 32-bit float) and `atoi` (a 32-bit integer).
    FloatInt,
    /// Used as text.
    Text,
}

/// A key's kind and built-in default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A switch: byte `index` of the module's flag array, default 0.
    Flag {
        /// The index in the flag array.
        index: u8,
    },
    /// An integer read with `GetPrivateProfileIntA`.
    Int {
        /// The default passed to the read.
        default: i32,
        /// How the value is kept.
        width: Width,
    },
    /// A string read with `GetPrivateProfileStringA`.
    Text {
        /// The default passed to the read.
        default: &'static str,
        /// The buffer size passed to the read.
        size: usize,
        /// The conversion applied to it.
        conv: Conv,
    },
}

/// One key the module reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    /// The section, as the module spells it.
    pub section: &'static str,
    /// The key name, as the module spells it (`"disablepausekey "` has its trailing space).
    pub name: &'static str,
    /// What is read and kept.
    pub kind: Kind,
}

impl Key {
    const fn flag(section: &'static str, name: &'static str, index: u8) -> Self {
        Self {
            section,
            name,
            kind: Kind::Flag { index },
        }
    }

    const fn int(section: &'static str, name: &'static str, default: i32, width: Width) -> Self {
        Self {
            section,
            name,
            kind: Kind::Int { default, width },
        }
    }

    const fn text(
        section: &'static str,
        name: &'static str,
        default: &'static str,
        size: usize,
        conv: Conv,
    ) -> Self {
        Self {
            section,
            name,
            kind: Kind::Text {
                default,
                size,
                conv,
            },
        }
    }
}

/// The INI reads the module makes; `section`, `key` and `default` are ASCII.
pub trait Profile {
    /// `GetPrivateProfileIntA(section, key, default, file)`.
    fn int(&self, section: &str, key: &str, default: i32) -> i32;

    /// `GetPrivateProfileStringA(section, key, default, buffer, size, file)` into a buffer of `size` bytes that
    /// holds 0xCC before the call: the whole buffer afterwards.
    fn text(&self, section: &str, key: &str, default: &str, size: usize) -> Vec<u8>;
}

/// Why a value could not be looked up: a defect in the caller, which names a key the module does not read or asks
/// for it as the wrong kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// The module reads no such key.
    Unknown {
        /// The section asked for.
        section: String,
        /// The key asked for.
        name: String,
    },
    /// The key exists but is kept differently.
    Kind {
        /// `[section] name`.
        key: String,
        /// What the caller asked for.
        wanted: &'static str,
        /// What the key is.
        found: Kind,
    },
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown { section, name } => {
                write!(f, "SpellAddon reads no key [{section}] {name:?}")
            }
            Self::Kind { key, wanted, found } => {
                write!(f, "{key} is {found:?}, not {wanted}")
            }
        }
    }
}

impl std::error::Error for KeyError {}

/// What one read gave.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Raw {
    Int(i32),
    Buffer(Vec<u8>),
}

/// Every key's value as read, in [`KEYS`] order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Values {
    raw: Vec<Raw>,
}

/// One INI diagnostic for addon.log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Whether this is a warning rather than an informational line.
    pub warning: bool,
    /// The section and quoted key name.
    pub key: String,
    /// What was used or ignored.
    pub message: String,
}

/// Diagnostics for duplicate keys in an INI file's text.
pub fn duplicate_keys(text: &[u8]) -> Vec<Diagnostic> {
    use std::collections::BTreeMap;

    let mut section = None;
    let mut first = BTreeMap::new();
    let mut diagnostics = Vec::new();
    for line in text.split(|&c| c == b'\n') {
        let line = line.trim_ascii();
        if line.starts_with(b";") {
            continue;
        }
        if let Some(rest) = line.strip_prefix(b"[") {
            section = rest
                .iter()
                .position(|&c| c == b']')
                .map(|end| rest[..end].trim_ascii().to_ascii_lowercase());
            continue;
        }
        let Some(section) = &section else { continue };
        let Some(equal) = line.iter().position(|&c| c == b'=') else {
            continue;
        };
        let name = line[..equal].trim_ascii().to_ascii_lowercase();
        let value = String::from_utf8_lossy(line[equal + 1..].trim_ascii()).into_owned();
        let key = (section.clone(), name);
        if let Some(used) = first.get(&key) {
            diagnostics.push(Diagnostic {
                warning: true,
                key: format!(
                    "[{}] {:?}",
                    String::from_utf8_lossy(section),
                    String::from_utf8_lossy(&key.1)
                ),
                message: format!("duplicate key: using {used:?}, ignoring {value:?}"),
            });
        } else {
            first.insert(key, value);
        }
    }
    diagnostics
}

// The pause-key spellings are one name to the native profile reader, so they are not an alias pair here.
const ALIASES: [(&str, &str, &str, &str); 4] = [
    ("gameplay", "avrgarmors", "gameplay", "avgarmors"),
    ("gameplay", "spellsaimod", "speed", "spellsaimod"),
    (
        "gameplay",
        "spellsarealwaysconstructable",
        "speed",
        "spellsarealwaysconstructable",
    ),
    (
        "gameplay",
        "showallquestsinmulti",
        "speed",
        "showallquestsinmulti",
    ),
];

fn present(profile: &impl Profile, section: &str, name: &str) -> bool {
    // Two defaults distinguish an absent key from an explicitly empty value without a reserved sentinel.
    profile.text(section, name, "", 2)[0] != 0 || profile.text(section, name, "x", 2)[0] == 0
}

/// Whether the number `written`, which the system read as `value`, lies outside `range`: either does, so a number
/// that wraps into the range is caught too.
fn out_of_range(written: &str, value: i32, range: RangeInclusive<i128>) -> bool {
    let text = written.trim();
    let unsigned = text.trim_start_matches(['-', '+']);
    let number = if let Some(hex) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        i128::from_str_radix(hex, 16).map(|n| if text.starts_with('-') { -n } else { n })
    } else {
        text.parse::<i128>()
    };
    !range.contains(&i128::from(value))
        || match number {
            Ok(n) => !range.contains(&n),
            Err(e) => matches!(
                e.kind(),
                std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow
            ),
        }
}

/// The number written for a key, as the system reads its text.
fn written(profile: &impl Profile, section: &str, name: &str) -> String {
    let buffer = profile.text(section, name, "", 128);
    String::from_utf8_lossy(buffer.split(|&c| c == 0).next().unwrap_or(&buffer)).into_owned()
}

impl Values {
    /// Reads every key and reports configuration diagnostics.
    pub fn read_with_diagnostics(profile: &impl Profile, mut log: impl FnMut(Diagnostic)) -> Self {
        let raw = KEYS
            .iter()
            .map(|k| {
                let (mut section, mut name) = (k.section, k.name);
                if let Some(&(alias_section, alias, _, _)) = ALIASES.iter()
                    .find(|&&(_, _, s, n)| s == k.section && n == k.name)
                    && present(profile, alias_section, alias)
                {
                    let canonical = present(profile, k.section, k.name);
                    log(Diagnostic {
                        warning: canonical,
                        key: format!("[{}] {:?}", k.section, k.name),
                        message: format!("{} alias [{alias_section}] {alias:?}", if canonical { "ignoring" } else { "using" }),
                    });
                    if !canonical {
                        (section, name) = (alias_section, alias);
                    }
                }
                match k.kind {
                Kind::Flag { .. } => {
                    let value = profile.int(section, name, 0);
                    let written = written(profile, section, name);
                    if out_of_range(&written, value, -128..=127) {
                        log(Diagnostic {
                            warning: true,
                            key: format!("[{}] {:?}", k.section, k.name),
                            message: format!("switch value {written} is outside the signed byte range; using {}", value as i8),
                        });
                    }
                    Raw::Int(value)
                },
                Kind::Int { default, width } => {
                    let value = profile.int(section, name, default);
                    if width == Width::Byte {
                        // An absent key reads as empty text and takes its default, which is in range.
                        let written = written(profile, section, name);
                        if !written.is_empty() && out_of_range(&written, value, 0..=255) {
                            log(Diagnostic {
                                warning: true,
                                key: format!("[{}] {:?}", k.section, k.name),
                                message: format!("value {written} is outside the byte range 0..255; using {}", value as u8),
                            });
                        }
                    }
                    Raw::Int(value)
                },
                Kind::Text { default, size, .. } => {
                    Raw::Buffer(profile.text(section, name, default, size))
                }
            }})
            .collect();
        Self { raw }
    }

    /// Reads every key without collecting configuration diagnostics.
    pub fn read(profile: &impl Profile) -> Self {
        Self::read_with_diagnostics(profile, |_| {})
    }

    fn find(&self, section: &str, name: &str) -> Result<(&'static Key, &Raw), KeyError> {
        KEYS.iter()
            .zip(&self.raw)
            .find(|(k, _)| k.section == section && k.name == name)
            .ok_or_else(|| KeyError::Unknown {
                section: section.to_owned(),
                name: name.to_owned(),
            })
    }

    fn wrong(key: &Key, wanted: &'static str) -> KeyError {
        KeyError::Kind {
            key: format!("[{}] {}", key.section, key.name),
            wanted,
            found: key.kind,
        }
    }

    /// A flag: the low byte of what was read, signed.
    pub fn flag(&self, section: &str, name: &str) -> Result<i8, KeyError> {
        match self.find(section, name)? {
            (
                Key {
                    kind: Kind::Flag { .. },
                    ..
                },
                Raw::Int(v),
            ) => Ok(*v as i8),
            (key, _) => Err(Self::wrong(key, "a flag")),
        }
    }

    /// An integer kept as 32 bits.
    pub fn int(&self, section: &str, name: &str) -> Result<i32, KeyError> {
        match self.find(section, name)? {
            (
                Key {
                    kind:
                        Kind::Int {
                            width: Width::Dword,
                            ..
                        },
                    ..
                },
                Raw::Int(v),
            ) => Ok(*v),
            (key, _) => Err(Self::wrong(key, "a 32-bit integer")),
        }
    }

    /// An integer kept as its low byte.
    pub fn byte(&self, section: &str, name: &str) -> Result<u8, KeyError> {
        match self.find(section, name)? {
            (
                Key {
                    kind:
                        Kind::Int {
                            width: Width::Byte, ..
                        },
                    ..
                },
                Raw::Int(v),
            ) => Ok(*v as u8),
            (key, _) => Err(Self::wrong(key, "a byte integer")),
        }
    }

    /// The 32-bit float for a string key, read with `atof`.
    pub fn float(&self, section: &str, name: &str) -> Result<f32, KeyError> {
        match self.find(section, name)? {
            (
                Key {
                    kind:
                        Kind::Text {
                            conv: Conv::Float | Conv::FloatInt,
                            ..
                        },
                    ..
                },
                Raw::Buffer(b),
            ) => Ok(number::atof(b) as f32),
            (key, _) => Err(Self::wrong(key, "a 32-bit float")),
        }
    }

    /// The 64-bit float the digit parser gives for a string key.
    pub fn double(&self, section: &str, name: &str) -> Result<f64, KeyError> {
        match self.find(section, name)? {
            (
                Key {
                    kind:
                        Kind::Text {
                            conv: Conv::Digits, ..
                        },
                    ..
                },
                Raw::Buffer(b),
            ) => Ok(number::digits(b)),
            (key, _) => Err(Self::wrong(key, "a digit-parsed 64-bit float")),
        }
    }

    /// The `atoi` integer the module also keeps for a string key read both ways.
    pub fn text_int(&self, section: &str, name: &str) -> Result<i32, KeyError> {
        match self.find(section, name)? {
            (
                Key {
                    kind:
                        Kind::Text {
                            conv: Conv::FloatInt,
                            ..
                        },
                    ..
                },
                Raw::Buffer(b),
            ) => Ok(number::atoi(b)),
            (key, _) => Err(Self::wrong(key, "a string key read with atoi")),
        }
    }

    /// A text key's string, without its NUL.
    pub fn text(&self, section: &str, name: &str) -> Result<&[u8], KeyError> {
        match self.find(section, name)? {
            (
                Key {
                    kind:
                        Kind::Text {
                            conv: Conv::Text, ..
                        },
                    ..
                },
                Raw::Buffer(b),
            ) => Ok(b.split(|&c| c == 0).next().unwrap_or(b)),
            (key, _) => Err(Self::wrong(key, "a text key")),
        }
    }

    /// The flags that are not 0, in read order: what the INI switches on.
    pub fn flags_on(&self) -> impl Iterator<Item = (&'static Key, i8)> {
        KEYS.iter()
            .zip(&self.raw)
            .filter_map(|(k, r)| match (k.kind, r) {
                (Kind::Flag { .. }, Raw::Int(v)) if *v as i8 != 0 => Some((k, *v as i8)),
                _ => None,
            })
    }
}

#[cfg(test)]
mod tests;
