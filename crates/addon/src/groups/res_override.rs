//! ResOverride: the mod's `.res` archives (config.reg `[RES]`) stand in front of the game's.
//!
//! - The mod's archives open at the first archive open or at `MaterialManager::Create`, whichever comes first.
//! - Each game archive the game opens is matched by its lower-case path against `[RES]`'s keys; a `-` entry hides
//!   the game's archive.
//! - A file lookup (`GetFile`: thirteen calls and the archive vtable's slot) asks the mod's archives for that archive
//!   first, then the archive unless hidden; a `t\` or `s\` name not found is tried once more without the prefix.
//! - A listing (`ListFiles`, four calls) appends the mod's listings, then the archive's own, to the caller's array,
//!   which is not emptied first, as in the original.
//! - `figures.res` and `menus.res` entries are extra figure archives, `textures.res`, `redress.res` and `menus.res`
//!   ones extra texture archives.
#![expect(
    unsafe_code,
    reason = "game.exe's archive functions, the paths it passes, and the replacement GetFile and ListFiles"
)]

use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString, c_void};
use std::sync::{Mutex, OnceLock, PoisonError};

use eix_core::mod_files::{ResConfig, lower};
use eix_engine::game::res::{self, GET_FILE, MappedResource, PathRange, StringArray};
use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, Mode};

use super::arg_path::c_str;
use super::{BuildError, Labelled, Setup};
use crate::log;

/// An archive's address, as the tables key it.
type Archive = usize;

#[derive(Default)]
struct Tables {
    loaded: bool,
    /// By `[RES]` key, the mod's open archives.
    mod_archives: HashMap<Vec<u8>, Vec<Archive>>,
    /// Game archives that `[RES]` overrides, with their key.
    overridden: HashMap<Archive, Vec<u8>>,
    /// Game archives a `-` entry hides.
    ignored: HashSet<Archive>,
}

static CONFIG: OnceLock<ResConfig> = OnceLock::new();
/// Never held while game code runs: the game's archive calls come back into these hooks.
static TABLES: Mutex<Option<Tables>> = Mutex::new(None);

fn with_tables<R>(f: impl FnOnce(&mut Tables) -> R) -> R {
    let mut guard = TABLES.lock().unwrap_or_else(PoisonError::into_inner);
    f(guard.get_or_insert_with(Tables::default))
}

/// Opens the mod's archives, once.
fn ensure_loaded() {
    let Some(config) = CONFIG.get() else {
        return;
    };
    if with_tables(|t| std::mem::replace(&mut t.loaded, true)) {
        return;
    }
    for (key, paths) in &config.overrides {
        for p in paths {
            let shown = String::from_utf8_lossy(p).into_owned();
            let Ok(c) = CString::new(p.clone()) else {
                log::error("res", "a mod archive path holds a NUL", &shown);
                continue;
            };
            // The game keeps archives for the process's life; these are the addon's and are never freed.
            let archive: &'static mut MappedResource =
                Box::leak(Box::new(MappedResource::zeroed()));
            let ptr = core::ptr::from_mut(archive);
            // SAFETY: the build is RU 1.06; `ptr` is a fresh zeroed block of the archive's size, on the game's
            // thread (the first archive open or the material manager's set-up).
            unsafe { res::construct(ptr) };
            // SAFETY: as above; `ptr` is now a constructed archive and `c` a NUL-terminated path.
            if unsafe { res::open(ptr, &c, 0) } {
                log::info("res", "mod archive opened", &shown);
                with_tables(|t| {
                    t.mod_archives
                        .entry(key.clone())
                        .or_default()
                        .push(ptr.expose_provenance());
                });
            } else {
                log::error("res", "cannot open the mod archive", &shown);
            }
        }
    }
}

/// `OpenResource`'s entry: notes a game archive `[RES]` names, once the mod's archives are open.
extern "C" fn on_open(_: usize, ctx: &mut Context) {
    ensure_loaded();
    let Some(config) = CONFIG.get() else {
        return;
    };
    let archive = ctx.ecx as Archive;
    // Word 0 is the pushed SEH handler, 1 the `-1` before it, 2 the return address, 3 the path.
    let Some(word) = ctx.stack_word(3) else {
        return;
    };
    // SAFETY: `OpenResource`'s first argument is the archive's path, a NUL-terminated string live for the call.
    let Some(path) = (unsafe { c_str(word) }) else {
        return;
    };
    let key = lower(path.to_bytes());
    with_tables(|t| {
        if config.ignored.contains(&key) {
            t.ignored.insert(archive);
        }
        if config.overrides.contains_key(&key) {
            t.overridden.insert(archive, key);
        }
    });
}

/// The archive destructor's entry: forgets the archive.
extern "C" fn on_close(_: usize, ctx: &mut Context) {
    let archive = ctx.ecx as Archive;
    with_tables(|t| {
        t.overridden.remove(&archive);
        t.ignored.remove(&archive);
    });
}

extern "C" fn on_materials(_: usize, _: &mut Context) {
    ensure_loaded();
}

fn each_path(list: &[Vec<u8>], mut f: impl FnMut(&CStr)) {
    for p in list {
        match CString::new(p.clone()) {
            Ok(c) => f(&c),
            Err(_) => log::error(
                "res",
                "an archive path holds a NUL",
                &String::from_utf8_lossy(p),
            ),
        }
    }
}

extern "C" fn on_figures(_: usize, ctx: &mut Context) {
    let Some(config) = CONFIG.get() else {
        return;
    };
    let manager = core::ptr::with_exposed_provenance_mut::<c_void>(ctx.esi as usize);
    each_path(&config.figures, |p| {
        // SAFETY: at 0x5AA544 ESI is the figure manager being created, on the game's thread.
        unsafe { res::figures_add(manager, p) };
    });
}

extern "C" fn on_linked_figures(_: usize, ctx: &mut Context) {
    let Some(config) = CONFIG.get() else {
        return;
    };
    let loader = core::ptr::with_exposed_provenance_mut::<c_void>(ctx.esi as usize);
    each_path(&config.figures, |p| {
        let bytes = p.to_bytes();
        let range = PathRange {
            begin: bytes.as_ptr(),
            end: bytes.as_ptr().wrapping_add(bytes.len()),
        };
        // SAFETY: at 0x4F6952 ESI is the linked-figure loader being built; `range` covers a live string.
        unsafe { res::linked_figures_add(loader, &range) };
    });
}

extern "C" fn on_textures(_: usize, ctx: &mut Context) {
    let Some(config) = CONFIG.get() else {
        return;
    };
    let manager = core::ptr::with_exposed_provenance_mut::<c_void>(ctx.esi as usize);
    each_path(&config.textures, |p| {
        // SAFETY: at 0x6915EF ESI is the texture manager being created, on the game's thread.
        unsafe { res::textures_add(manager, p) };
    });
}

/// The mod's archives for `archive`, and whether `archive` itself is hidden.
fn sources(archive: Archive) -> (Vec<Archive>, bool) {
    with_tables(|t| {
        let mods = t
            .overridden
            .get(&archive)
            .and_then(|k| t.mod_archives.get(k))
            .cloned()
            .unwrap_or_default();
        (mods, t.ignored.contains(&archive))
    })
}

fn find(archive: Archive, name: *const u8, size: *mut u32) -> *mut c_void {
    let (mods, ignored) = sources(archive);
    for m in mods {
        let m = core::ptr::with_exposed_provenance_mut::<MappedResource>(m);
        // SAFETY: `m` is a mod archive opened in `ensure_loaded`; name and size are the game's, passed on.
        let data = unsafe { res::get_file(m, name, size) };
        if !data.is_null() {
            return data;
        }
    }
    if ignored {
        return core::ptr::null_mut();
    }
    let this = core::ptr::with_exposed_provenance_mut::<MappedResource>(archive);
    // SAFETY: the game's own call, as it was made.
    unsafe { res::get_file(this, name, size) }
}

/// Stands in for `MappedResource::GetFile`.
extern "thiscall" fn get_file(
    this: *mut MappedResource,
    name: *const u8,
    size: *mut u32,
) -> *mut c_void {
    let archive = this.expose_provenance();
    let data = find(archive, name, size);
    if !data.is_null() {
        return data;
    }
    // SAFETY: the game passes the file's name, a NUL-terminated string live for the call.
    let Some(n) = (unsafe { c_str(name.expose_provenance() as u32) }) else {
        return data;
    };
    match n.to_bytes() {
        [b't' | b's', b'\\', ..] => find(archive, name.wrapping_add(2), size),
        _ => data,
    }
}

/// Stands in for `MappedResource::ListFiles`.
extern "thiscall" fn list_files(
    this: *mut MappedResource,
    out: *mut StringArray,
    pattern: *const u8,
) {
    let (mods, ignored) = sources(this.expose_provenance());
    let mut temp = core::mem::MaybeUninit::<StringArray>::uninit();
    let temp = temp.as_mut_ptr();
    // SAFETY: `temp` is writable storage of the array's size, holding no array yet.
    unsafe { res::array_construct(temp) };
    let from = |archive: *mut MappedResource| {
        // SAFETY: `archive` is a mod archive or the game's, `temp` a constructed array, `pattern` the game's.
        unsafe { res::list_files(archive, temp, pattern) };
        // SAFETY: `out` is the caller's constructed array, distinct from `temp`.
        unsafe { res::array_append(out, temp) };
    };
    for m in mods {
        from(core::ptr::with_exposed_provenance_mut(m));
    }
    if !ignored {
        from(this);
    }
    // SAFETY: `temp` was constructed above and is not used afterwards.
    unsafe { res::array_destroy(temp) };
}

const FILE_FIND_MASK: [(usize, [u8; 5]); 4] = [
    (
        eix_sites::TEXT_SOURCES_LIST_KEYS__53,
        [0xE8, 0x28, 0x5E, 0x19, 0x00],
    ),
    (eix_sites::CAI_MAP_LOAD__113, [0xE8, 0xE8, 0x05, 0x0C, 0x00]),
    (
        eix_sites::C_SOUND_MANAGER_LIST_WAVE_FILES__5B,
        [0xE8, 0x50, 0xD9, 0xFB, 0xFF],
    ),
    (
        eix_sites::TERRAIN_LOAD_MAP__71,
        [0xE8, 0x9A, 0xE1, 0xFA, 0xFF],
    ),
];

const FILE_FIND: [(usize, [u8; 5]); 13] = [
    (
        eix_sites::LEVER_RECORD_TABLE_LOAD__E4,
        [0xE8, 0xC7, 0xE6, 0x20, 0x00],
    ),
    (
        eix_sites::LOAD_LIGHTNING_NOISE__116,
        [0xE8, 0x25, 0x90, 0x20, 0x00],
    ),
    (
        eix_sites::SKILL_DATABASE_LOAD__AD,
        [0xE8, 0x6E, 0x18, 0x15, 0x00],
    ),
    (
        eix_sites::UNIT_DATABASE_LOAD__AD,
        [0xE8, 0x8E, 0x78, 0x13, 0x00],
    ),
    (eix_sites::CAI_MAP_LOAD__15C, [0xE8, 0x1F, 0xD6, 0x0B, 0x00]),
    (eix_sites::CAI_MAP_LOAD__754, [0xE8, 0x27, 0xD0, 0x0B, 0x00]),
    (eix_sites::CAI_MAP_LOAD__CBA, [0xE8, 0xC1, 0xCA, 0x0B, 0x00]),
    (
        eix_sites::AI_EVENT_TABLE_SETUP__1A2,
        [0xE8, 0x89, 0x86, 0x0A, 0x00],
    ),
    (
        eix_sites::SPELL_DATABASE_LOAD__AD,
        [0xE8, 0x3E, 0xD5, 0xFE, 0xFF],
    ),
    (
        eix_sites::ITEM_DATABASE_LOAD__AD,
        [0xE8, 0xEE, 0xB2, 0xFC, 0xFF],
    ),
    (
        eix_sites::QUEST_DATABASE_LOAD__EA,
        [0xE8, 0xB1, 0x36, 0xFC, 0xFF],
    ),
    (
        eix_sites::PRINTS_DATABASE_LOAD__E7,
        [0xE8, 0x24, 0x14, 0xFA, 0xFF],
    ),
    (
        eix_sites::ACK_DATABASE_LOAD__EE,
        [0xE8, 0x2D, 0xA4, 0xF9, 0xFF],
    ),
];

/// The archive vtable's `GetFile` slot.
const GET_FILE_SLOT: usize = eix_sites::RDATA_7432F4;

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let _ = CONFIG.set(setup.res.clone());
    for (key, paths) in &setup.res.overrides {
        log::info(
            "res",
            &format!("{} mod archive(s) override", paths.len()),
            &String::from_utf8_lossy(key),
        );
    }
    for key in &setup.res.ignored {
        log::info("res", "game archive hidden", &String::from_utf8_lossy(key));
    }
    let push = |imm: u32| {
        let mut b = [0x68, 0, 0, 0, 0];
        b[1..].copy_from_slice(&imm.to_le_bytes());
        b
    };
    let context =
        |label: &str, site, original: &[u8], mode, handler| -> Result<Labelled, BuildError> {
            Ok(Labelled {
                label: label.to_owned(),
                write: hook::context_call(site, original, mode, handler, 0)?,
            })
        };
    let mut out = vec![
        context(
            "ResOpen",
            eix_sites::MAPPED_RESOURCE_OPEN_RESOURCE__8,
            &push(eix_sites::TEXT_7312A0 as u32),
            Mode::Before,
            on_open,
        )?,
        context(
            "ResClose",
            eix_sites::C_MAPPED_RESOURCE_C_MAPPED_RESOURCE__2,
            &push(eix_sites::TEXT_731210 as u32),
            Mode::Before,
            on_close,
        )?,
        context(
            "ResInit1",
            eix_sites::MATERIAL_MANAGER_CREATE_HOOK__8,
            &push(eix_sites::TEXT_732D36 as u32),
            Mode::Before,
            on_materials,
        )?,
        context(
            "ResInit2",
            eix_sites::C_FIGURE_MANAGER_CREATE_WITH_ARCHIVES__34,
            &push(eix_sites::DATA_7AF45C as u32),
            Mode::Before,
            on_figures,
        )?,
        context(
            "ResInit3(Figure)",
            eix_sites::C_LINK_FIGURE_LOADER_CONSTRUCT__92,
            &push(eix_sites::DATA_78EC24 as u32),
            Mode::Before,
            on_linked_figures,
        )?,
        // The manager's ESI is read before the replaced `mov esi, 1` runs.
        context(
            "ResInit4(Texture)",
            eix_sites::TEXTURE_MANAGER_CREATE_HOOK__EF,
            &[0xBE, 1, 0, 0, 0],
            Mode::After,
            on_textures,
        )?,
    ];
    for (i, (site, original)) in FILE_FIND_MASK.iter().enumerate() {
        let (write, _) = hook::redirect_call(*site, *original, list_files as *const () as usize)?;
        out.push(Labelled {
            label: format!("ResFileFindMask #{}", i + 1),
            write,
        });
    }
    for (i, (site, original)) in FILE_FIND.iter().enumerate() {
        let (write, _) = hook::redirect_call(*site, *original, get_file as *const () as usize)?;
        out.push(Labelled {
            label: format!("ResFileFind #{:X}", i + 1),
            write,
        });
    }
    out.push(Labelled {
        label: "ResFileFind #E".to_owned(),
        write: SiteWrite::pointer(GET_FILE_SLOT, GET_FILE, get_file as *const () as usize),
    });
    Ok(out)
}
