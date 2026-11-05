//! Where the mod's files stand in for the game's: the `[RES]` archive table and the `Maps` folder index.
//!
//! Paths are raw ANSI bytes, as game.exe passes them.

use std::collections::{BTreeMap, BTreeSet};

use crate::options::Bytes;

/// `path` with a `\` added unless it is empty or ends in one.
pub fn with_slash(mut path: Bytes) -> Bytes {
    if path.last().is_some_and(|&c| c != b'\\') {
        path.push(b'\\');
    }
    path
}

/// The part of `path` from its last `\` on (the `\` included), or `None` when it has none.
pub fn from_last_slash(path: &[u8]) -> Option<&[u8]> {
    path.iter().rposition(|&c| c == b'\\').map(|i| &path[i..])
}

/// `bytes` with ASCII letters lowered; the game's archive keys and the addon's name lookups compare this way.
pub fn lower(bytes: &[u8]) -> Bytes {
    bytes.to_ascii_lowercase()
}

/// The game archives a `[RES]` entry can stand in for, by key name. `speech.res` lies under the CD path.
const OVERRIDABLE: [&str; 12] = [
    "aiinfo.res",
    "database.res",
    "databaselmp.res",
    "gfx.res",
    "materials.res",
    "outro.res",
    "redress.res",
    "sfx.res",
    "speech.res",
    "texts.res",
    "textslmp.res",
    "textures.res",
];

/// `[RES]` entries that add archives to the figure manager.
const FIGURE_KEYS: [&str; 2] = ["figures.res", "menus.res"];
/// `[RES]` entries that add archives to the texture manager.
const TEXTURE_KEYS: [&str; 3] = ["textures.res", "redress.res", "menus.res"];

/// The mod's archive table, from config.reg's `[RES]`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResConfig {
    /// By the lower-case path the game opens an archive with: the mod's archives asked first, as full paths.
    pub overrides: BTreeMap<Bytes, Vec<Bytes>>,
    /// Lower-case paths of the game archives a `-` entry hides.
    pub ignored: BTreeSet<Bytes>,
    /// Extra figure archives, full paths.
    pub figures: Vec<Bytes>,
    /// Extra texture archives, full paths.
    pub textures: Vec<Bytes>,
}

impl ResConfig {
    /// Builds the table from `[RES]` (`entries`), the mod folder (`mod_dir`, ending in `\`), and the game's
    /// `RESOURCES PATH` and `CD-ROM PATH`, empty when not set.
    pub fn new(
        entries: &[(Bytes, Vec<Bytes>)],
        mod_dir: &[u8],
        resources: &[u8],
        cd: &[u8],
    ) -> Self {
        // Not set, the original takes `.\Res\` and `.\`, as the game does: a GOG install keeps its paths only in
        // the Nival tree, so under the starter's tree the game opens `.\res\texts.res`.
        let or = |value: &[u8], default: &[u8]| {
            with_slash(if value.is_empty() { default } else { value }.to_vec())
        };
        let resources = or(resources, b".\\Res\\");
        let cd = or(cd, b".\\");
        let values = |name: &str| {
            entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name.as_bytes()))
                .map(|(_, v)| v.as_slice())
                .unwrap_or_default()
        };
        let key = |base: &[u8], name: &str| lower(&[base, name.as_bytes()].concat());
        let full = |value: &[u8]| [mod_dir, value].concat();
        let mut config = Self::default();
        for name in OVERRIDABLE {
            let k = if name == "speech.res" {
                key(&[&cd[..], b"Res\\"].concat(), name)
            } else {
                key(&resources, name)
            };
            let mut paths = Vec::new();
            for v in values(name) {
                if v.first() == Some(&b'-') {
                    config.ignored.insert(k.clone());
                } else {
                    paths.push(full(v));
                }
            }
            if !paths.is_empty() {
                config.overrides.insert(k, paths);
            }
        }
        for (names, out) in [
            (&FIGURE_KEYS[..], &mut config.figures),
            (&TEXTURE_KEYS[..], &mut config.textures),
        ] {
            for name in names {
                for v in values(name) {
                    if v.first() == Some(&b'-') {
                        config.ignored.insert(key(&resources, name));
                    } else {
                        out.push(full(v));
                    }
                }
            }
        }
        config
    }
}

/// The mod's `Maps` folder as the addon lists it at start.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapsIndex {
    /// Files of the top folder by name, files of subfolders as `<sub>\<name>`.
    pub names: Vec<Bytes>,
    /// Whether the top folder holds a file ending in `.mq` (compared with case), which makes the zone table come
    /// from the mod.
    pub has_quest_list: bool,
}

impl MapsIndex {
    /// Adds a file found at `sub` (empty for the top folder, else `a` or `a\b`).
    pub fn add(&mut self, sub: &[u8], name: &[u8]) {
        if sub.is_empty() {
            if name.ends_with(b".mq") {
                self.has_quest_list = true;
            }
            self.names.push(name.to_vec());
        } else {
            self.names.push([sub, b"\\", name].concat());
        }
    }

    /// Whether the mod has the file `name` (ignoring ASCII case).
    pub fn has(&self, name: &[u8]) -> bool {
        self.names.iter().any(|n| n.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, values: &[&str]) -> (Bytes, Vec<Bytes>) {
        (
            name.as_bytes().to_vec(),
            values.iter().map(|v| v.as_bytes().to_vec()).collect(),
        )
    }

    #[test]
    fn paths_get_their_slash_and_tail() {
        assert_eq!(with_slash(b"C:\\Mods\\inv".to_vec()), b"C:\\Mods\\inv\\");
        assert_eq!(with_slash(b"C:\\x\\".to_vec()), b"C:\\x\\");
        assert!(with_slash(Vec::new()).is_empty());
        assert_eq!(
            from_last_slash(b"res\\movies\\intro.bik"),
            Some(&b"\\intro.bik"[..])
        );
        assert_eq!(from_last_slash(b"intro.bik"), None);
    }

    #[test]
    fn res_entries_override_ignore_and_add() {
        let entries = [
            entry("Textures.res", &["res\\textures.res"]),
            entry("database.res", &["res\\a.res", "res\\b.res"]),
            entry("speech.res", &["-"]),
            entry("figures.res", &["res\\figs.res"]),
            entry("unknown.res", &["res\\u.res"]),
        ];
        let c = ResConfig::new(&entries, b"C:\\Mods\\inv\\", b"C:\\GOG\\EI\\Res", b".\\");
        assert_eq!(
            c.overrides.get(&b"c:\\gog\\ei\\res\\textures.res"[..]),
            Some(&vec![b"C:\\Mods\\inv\\res\\textures.res".to_vec()])
        );
        assert_eq!(
            c.overrides
                .get(&b"c:\\gog\\ei\\res\\database.res"[..])
                .map(Vec::len),
            Some(2)
        );
        assert!(c.ignored.contains(&b".\\res\\speech.res"[..]));
        assert_eq!(c.overrides.len(), 2);
        assert_eq!(c.figures, [b"C:\\Mods\\inv\\res\\figs.res".to_vec()]);
        assert_eq!(c.textures, [b"C:\\Mods\\inv\\res\\textures.res".to_vec()]);
    }

    #[test]
    fn res_paths_not_set_take_the_defaults() {
        let entries = [
            entry("texts.res", &["res\\texts.res"]),
            entry("speech.res", &["-"]),
            entry("menus.res", &["-"]),
        ];
        let c = ResConfig::new(&entries, b"C:\\Mods\\inv\\", b"", b"");
        assert_eq!(
            c.overrides.keys().collect::<Vec<_>>(),
            [&b".\\res\\texts.res".to_vec()]
        );
        assert!(c.ignored.contains(&b".\\res\\speech.res"[..]));
        assert!(c.ignored.contains(&b".\\res\\menus.res"[..]));
    }

    #[test]
    fn maps_index_matches_names_ignoring_case() {
        let mut m = MapsIndex::default();
        m.add(b"", b"Zone1.mob");
        m.add(b"sub", b"x.mpr");
        assert!(!m.has_quest_list);
        m.add(b"sub", b"q.mq");
        assert!(!m.has_quest_list, "only the top folder's .mq counts");
        m.add(b"", b"quests.MQ");
        assert!(!m.has_quest_list, "the suffix is compared with case");
        m.add(b"", b"quests.mq");
        assert!(m.has_quest_list);
        assert!(m.has(b"zone1.MOB"));
        assert!(m.has(b"SUB\\x.mpr"));
        assert!(!m.has(b"x.mpr"));
    }
}
