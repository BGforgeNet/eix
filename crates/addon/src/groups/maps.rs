//! Maps: a zone's map, mob and quest files come from the mod's `Maps` folder when the mod has them.
//!
//! At start the mod's `Maps\` is listed once (subfolders as `<sub>\<name>`). Before each of the nine uses of the maps
//! folder a hook looks the file's name up in that list, ignoring case; the zone table's use asks whether the top
//! folder has a `.mq` file. The answer stays until the next such hook: `MapsFolder` (0x55E990) then starts from the
//! mod's folder or the game's (the CString 0x7AF460).
#![expect(
    unsafe_code,
    reason = "reading the file names the game passes and building the mod's folder string"
)]

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use eix_core::mod_files::MapsIndex;
use eix_engine::hook::{self, Context, Mode};
use eix_sites::Site;

use super::arg_path::c_str;
use super::{BuildError, Labelled, Setup, game_text};
use crate::log;
use crate::setup::{ansi, path};

static INDEX: OnceLock<MapsIndex> = OnceLock::new();
/// The address of the game string holding the mod folder.
static MOD_FOLDER: OnceLock<u32> = OnceLock::new();
static IN_MOD: AtomicBool = AtomicBool::new(false);

/// Where a site's file name is.
#[derive(Debug, Clone, Copy)]
enum Name {
    Edi,
    Ebp,
    Esi,
    /// The word at EBP minus this.
    Frame(usize),
    /// A stack word.
    Word(usize),
    /// The zone table: the answer is whether the mod has a quest list.
    ZoneTable,
}

/// The nine sites: label, address, replaced bytes, where the name is.
const SITES: [(&str, Site, &[u8], Name); 9] = [
    (
        "Maps #1",
        eix_sites::C_WORLD_SERVER_NEW_ZONE__4E,
        &[0x8D, 0x4C, 0x24, 0x18, 0x51],
        Name::Edi,
    ),
    (
        "Maps #2",
        eix_sites::C_WORLD_SERVER_NEW_ZONE__BC,
        &[0x8D, 0x44, 0x24, 0x20, 0x8B, 0xCE],
        Name::Ebp,
    ),
    (
        "Maps #3",
        eix_sites::MAP_PATH__48,
        &[0x8D, 0x54, 0x24, 0x08, 0x52],
        Name::Esi,
    ),
    (
        "Maps #4",
        eix_sites::C_WORLD_CLIENT_LOAD_ZONE__2DA,
        &[0x8D, 0x4D, 0xD8, 0x51, 0x8B, 0xCE],
        Name::Frame(0x14),
    ),
    (
        "Maps #5",
        eix_sites::GAME_FLOW_BUILD_ZONE_TABLE__52,
        &[0xB9, 0x60, 0xFF, 0x7A, 0x00],
        Name::ZoneTable,
    ),
    (
        "Maps #6",
        eix_sites::LOAD_ZONE_QUESTS__93,
        &[0xB9, 0x60, 0xFF, 0x7A, 0x00],
        Name::Esi,
    ),
    (
        "Maps #7",
        eix_sites::LOAD_ZONE_QUESTS__144,
        &[0xB9, 0x60, 0xFF, 0x7A, 0x00],
        Name::Word(6),
    ),
    (
        "Maps #8",
        eix_sites::MAP_FILE_EXISTS__26,
        &[0xB9, 0x60, 0xFF, 0x7A, 0x00],
        Name::Word(14),
    ),
    (
        "Maps #9",
        eix_sites::WRITE_MAP_FILE__4A,
        &[0xB9, 0x60, 0xFF, 0x7A, 0x00],
        Name::Word(13),
    ),
];

/// `MapsFolder`'s `push 0x7AF460`, the game's maps folder CString.
const SET_PATH: Site = eix_sites::MAPS_FOLDER__19;
const SET_PATH_BYTES: [u8; 5] = [0x68, 0x60, 0xF4, 0x7A, 0x00];

fn name_address(name: Name, ctx: &Context) -> Option<u32> {
    match name {
        Name::Edi => Some(ctx.edi),
        Name::Ebp => Some(ctx.ebp),
        Name::Esi => Some(ctx.esi),
        Name::Frame(back) => ctx.stack_at((ctx.ebp as usize).checked_sub(back)?),
        Name::Word(i) => ctx.stack_word(i),
        Name::ZoneTable => None,
    }
}

extern "C" fn select(site: usize, ctx: &mut Context) {
    let (Some(index), Some((_, _, _, name))) = (INDEX.get(), SITES.get(site)) else {
        return;
    };
    let in_mod = match name {
        Name::ZoneTable => index.has_quest_list,
        &other @ (Name::Edi | Name::Ebp | Name::Esi | Name::Frame(_) | Name::Word(_)) => {
            let address = name_address(other, ctx).unwrap_or(0);
            // SAFETY: at each of these sites the register or word holds the file name the game is about to use, a
            // NUL-terminated string that lives for the call (checked against the game's code at the site).
            let name = unsafe { c_str(address) };
            let found = name.filter(|n| index.has(n.to_bytes()));
            if let Some(n) = found {
                log::info("maps", "the mod's file", &n.to_string_lossy());
            }
            found.is_some()
        }
    };
    IN_MOD.store(in_mod, Ordering::SeqCst);
}

extern "C" fn set_path(_: usize, ctx: &mut Context) {
    if let (true, Some(&folder)) = (IN_MOD.load(Ordering::SeqCst), MOD_FOLDER.get()) {
        // Word 0 is what the copied `push` pushed: the game's folder.
        ctx.set_stack_word(0, folder);
    }
}

/// Lists `dir` (`sub` within the mod's Maps folder) into `index`.
fn list(dir: &std::path::Path, sub: &[u8], index: &mut MapsIndex) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = ansi(&e.file_name());
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            let deeper = if sub.is_empty() {
                name
            } else {
                [sub, b"\\", &name[..]].concat()
            };
            list(&e.path(), &deeper, index);
        } else {
            index.add(sub, &name);
        }
    }
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let maps = path(&[&setup.mod_dir[..], b"Maps"].concat());
    let mut index = MapsIndex::default();
    list(&maps, b"", &mut index);
    log::info(
        "maps",
        &format!(
            "{} files listed, quest list {}",
            index.names.len(),
            if index.has_quest_list {
                "found"
            } else {
                "not found"
            }
        ),
        &maps.display().to_string(),
    );
    let _ = INDEX.set(index);
    let address = game_text::game_string(setup.mod_dir.clone())?;
    let _ = MOD_FOLDER.set(address as u32);
    let mut out = Vec::new();
    for (i, (label, site, original, _)) in SITES.iter().enumerate() {
        out.push(Labelled {
            label: (*label).to_owned(),
            write: hook::context_call(*site, original, Mode::Before, select, i)?,
        });
    }
    out.push(Labelled {
        label: "Maps SetPath".to_owned(),
        write: hook::context_call(SET_PATH, &SET_PATH_BYTES, Mode::Before, set_path, 0)?,
    });
    Ok(out)
}
