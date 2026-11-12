//! The features, one module per group of them. The registry ([`crate::registry`]) holds each build and runs them in
//! the original's order.
//!
//! A build gets the feature's flag value (the low byte of the INI value, signed) and returns the feature's writes to
//! game.exe; it returns none for a value its feature does nothing with. Writes expect game.exe's own bytes: where an
//! earlier feature wrote the same bytes, the later one wins, as in the original.

use std::cell::RefCell;
use std::fmt;

use eix_core::spelladdon::{KeyError, Values};
use eix_engine::group::SiteWrite;
use eix_engine::hook::HookError;
use eix_engine::patch::PatchError;

use crate::types::Log;

pub(crate) mod ai;
pub(crate) mod combat;
pub(crate) mod common;
pub(crate) mod items;
pub(crate) mod magic;
pub(crate) mod multiplayer;
pub(crate) mod rebalance;
pub(crate) mod script;
pub(crate) mod startup;
pub(crate) mod world;

/// A feature's preparation: its writes for the flag value `value`.
pub(crate) type Build = fn(ctx: &Ctx<'_>, value: i8) -> Result<Vec<SiteWrite>, BuildError>;

/// Why a feature could not be prepared.
#[derive(Debug)]
pub(crate) struct BuildError(pub(crate) String);

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<HookError> for BuildError {
    fn from(e: HookError) -> Self {
        Self(e.to_string())
    }
}

impl From<PatchError> for BuildError {
    fn from(e: PatchError) -> Self {
        Self(e.to_string())
    }
}

impl From<KeyError> for BuildError {
    fn from(e: KeyError) -> Self {
        Self(e.to_string())
    }
}

impl From<String> for BuildError {
    fn from(e: String) -> Self {
        Self(e)
    }
}

/// What a build reads.
pub(crate) struct Ctx<'a> {
    /// Every INI value.
    pub(crate) values: Inputs<'a>,
    /// addon.log.
    pub(crate) log: Log,
    /// The folder spelladdon.ini was read from, in the ANSI code page, without a trailing `\`.
    pub(crate) ini_dir: Vec<u8>,
    /// Startup actions prepared by a build; only the startup runner performs them.
    pub(crate) actions: RefCell<Vec<Action>>,
    /// Describe fixed image ranges without reading live code or applying run-time writes.
    pub(crate) inventory: bool,
}

/// Read-only feature inputs.
pub(crate) struct Inputs<'a> {
    values: &'a Values,
    #[cfg(test)]
    pub(crate) flags: RefCell<std::collections::BTreeSet<(String, String)>>,
}

impl std::ops::Deref for Inputs<'_> {
    type Target = Values;

    fn deref(&self) -> &Self::Target {
        self.values
    }
}

impl Inputs<'_> {
    pub(crate) fn flag(&self, section: &str, key: &str) -> Result<i8, KeyError> {
        #[cfg(test)]
        self.flags
            .borrow_mut()
            .insert((section.to_owned(), key.to_owned()));
        self.values.flag(section, key)
    }
}

/// An external effect to perform once at startup, in feature order.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Action {
    ImportantAlerts,
    VersionAlerts,
    AddFont(Vec<u8>),
}

impl<'a> Ctx<'a> {
    pub(crate) fn new(values: &'a Values, log: Log) -> Self {
        Self {
            values: Inputs {
                values,
                #[cfg(test)]
                flags: RefCell::default(),
            },
            log,
            ini_dir: Vec::new(),
            actions: RefCell::new(Vec::new()),
            inventory: false,
        }
    }

    pub(crate) fn inventory(values: &'a Values, log: Log) -> Self {
        Self {
            inventory: true,
            ..Self::new(values, log)
        }
    }

    /// A run-time global has no fixed before/after bytes; only its site and width describe all its writes.
    pub(crate) fn runtime_write<T>(&self, site: eix_sites::Site) -> Vec<SiteWrite> {
        if self.inventory {
            vec![SiteWrite::nop(site, &vec![0x90; size_of::<T>()])]
        } else {
            Vec::new()
        }
    }
}

impl fmt::Debug for Ctx<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ctx").finish_non_exhaustive()
    }
}

/// INI values for the features' tests.
#[cfg(test)]
pub(crate) mod testing {
    use std::collections::BTreeMap;
    use std::ops::Range;

    use eix_core::spelladdon::{Profile, Values};
    use eix_engine::group::SiteWrite;
    use eix_sites::Site;

    use super::Ctx;
    use crate::registry::{self, FEATURES};
    use crate::types::Level;

    /// Fixed `(section, key, value)` triples; every other key takes its built-in default.
    struct Ini<'a>(&'a [(&'a str, &'a str, &'a str)]);

    impl Profile for Ini<'_> {
        fn int(&self, section: &str, key: &str, default: i32) -> i32 {
            self.0
                .iter()
                .find(|(s, k, _)| *s == section && *k == key)
                // The OS keeps the low 32 bits of a longer number.
                .map_or(default, |(_, _, v)| v.parse::<i64>().unwrap() as i32)
        }

        fn text(&self, section: &str, key: &str, default: &str, size: usize) -> Vec<u8> {
            let v = self
                .0
                .iter()
                .find(|(s, k, _)| *s == section && *k == key)
                .map_or(default, |(_, _, v)| *v);
            let mut buf = vec![0xCC; size];
            buf[..v.len()].copy_from_slice(v.as_bytes());
            buf[v.len()] = 0;
            buf
        }
    }

    /// The values read from these triples.
    pub(crate) fn values(pairs: &[(&str, &str, &str)]) -> Values {
        Values::read(&Ini(pairs))
    }

    pub(crate) fn no_log(_: Level, _: &str, _: &str, _: &str) {}

    /// Each write as site, expected bytes, new bytes.
    pub(crate) fn plain(writes: &[SiteWrite]) -> Vec<(Site, Vec<u8>, Vec<u8>)> {
        writes
            .iter()
            .map(|w| (w.site(), w.expected().to_vec(), w.new_bytes().to_vec()))
            .collect()
    }

    /// game.exe's `.text` and `.rdata`, where the original's site bytes are compared.
    const CODE: Range<usize> = eix_sites::TEXT_SECTION.ru()..eix_sites::DATA_SECTION.ru();

    /// What every feature `ini` switches on changes in [`CODE`], merged in feature order as at start-up: each address
    /// whose last byte written differs from the first write's expected byte, with that byte.
    fn changed(ini: &[(&str, &str, &str)]) -> BTreeMap<usize, u8> {
        let v = values(ini);
        let ctx = Ctx::new(&v, no_log);
        let mut bytes = BTreeMap::new();
        for f in &FEATURES {
            let value = v.flag(f.section, f.key).unwrap_or(0);
            if value == 0 || registry::skipped(f, value, &v).is_some() {
                continue;
            }
            let Some(build) = f.build else { continue };
            let writes = build(&ctx, value)
                .unwrap_or_else(|e| panic!("[{}] {}={value}: {e}", f.section, f.key));
            for w in writes {
                for (i, (&was, &now)) in w.expected().iter().zip(w.new_bytes()).enumerate() {
                    bytes
                        .entry(w.site().ru() + i)
                        .and_modify(|e: &mut (u8, u8)| e.1 = now)
                        .or_insert((was, now));
                }
            }
        }
        bytes
            .into_iter()
            .filter(|&(at, (was, now))| CODE.contains(&at) && was != now)
            .map(|(at, (_, now))| (at, now))
            .collect()
    }

    /// What the original does for one INI: the flag and its value, more INI keys, and the bytes of [`CODE`] it
    /// changes as `(site, "hex bytes")`, `??` where our bytes may differ (a branch displacement into our own code).
    pub(crate) type Original = (
        &'static str,
        &'static str,
        i8,
        &'static [(&'static str, &'static str, &'static str)],
        &'static [(Site, &'static str)],
    );

    /// Asserts that each run's INI makes our features change the same bytes of [`CODE`] to the same values.
    pub(crate) fn assert_original(runs: &[Original]) {
        for &(section, key, value, more, sites) in runs {
            let flag = value.to_string();
            let mut ini = vec![(section, key, flag.as_str())];
            ini.extend_from_slice(more);
            let ours = changed(&ini);
            let mut theirs = BTreeMap::new();
            for &(site, text) in sites {
                for (i, b) in text.split(' ').enumerate() {
                    let b = (b != "??").then(|| u8::from_str_radix(b, 16).unwrap());
                    theirs.insert(site.ru() + i, b);
                }
            }
            for (at, now) in &ours {
                match theirs.get(at) {
                    Some(Some(b)) => assert_eq!(
                        now, b,
                        "{key}={value} {more:?}: {at:#x} is {now:02X}, the original's {b:02X}"
                    ),
                    Some(None) => {}
                    None => {
                        panic!("{key}={value} {more:?}: we change {at:#x}, the original does not")
                    }
                }
            }
            for (at, b) in &theirs {
                assert!(
                    b.is_none() || ours.contains_key(at),
                    "{key}={value} {more:?}: the original changes {at:#x}, we do not"
                );
            }
        }
    }
}
