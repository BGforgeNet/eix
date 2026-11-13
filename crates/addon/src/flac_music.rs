//! FLAC music: a track of the `Stream` folder plays from `<name>.flac` when there is one, in the mod's `Stream\`
//! folder first, then in the folder the game picked (core's [`eix_core::flac`]). eix.ini's `[Music] Flac` = 0
//! leaves both calls alone.
//!
//! The game streams music through the decoder Miles finds for `.mp3` (mp3dec.asi), calling the five functions of its
//! table itself (`CSoundManager::ReadStreamVanilla` 0x6BB3A0): open, process, seek, close and attribute, all
//! stdcall. Two calls are redirected:
//! - `CResourceStream::OpenFile` 0x6744C0 at 0x6BB65B gets the `.flac` path when the file is there; the track keeps
//!   its `.mp3` name inside the game, so music.reg and streamsn.reg's markers are unchanged.
//! - `SoundOpenStream` 0x6BAAC0 at 0x6B6B62 sets up the decoder; after it, the table it keeps (the stream's +0xC)
//!   gets our five functions. Ours open a stream that starts with `fLaC` themselves and hand every other stream, and
//!   every call for one, to the originals.
#![expect(
    unsafe_code,
    reason = "redirects game.exe's stream calls and stands in for the Miles decoder's functions"
)]

use core::ffi::{CStr, c_char, c_void};
use std::ffi::CString;
use std::io::Read;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use eix_core::eix_ini::Music;
use eix_core::flac::{self, Decoder, MAGIC};
use eix_engine::hook;
use eix_sites::Site;

use crate::game::BeforeWinMain;
use crate::groups::{Labelled, Tally, apply_built};
use crate::log;
use crate::setup::path;

/// The call of `CResourceStream::OpenFile` in `ReadStreamVanilla`: `ecx` the stream, the path and a size out.
const OPEN_FILE_CALL: Site = eix_sites::C_SOUND_MANAGER_READ_STREAM_VANILLA__2BB;
const OPEN_FILE_BYTES: [u8; 5] = [0xE8, 0x60, 0x8E, 0xFB, 0xFF];
/// The call of `SoundOpenStream` in `CSoundManager::Open`: `ecx` the stream record, the driver.
const OPEN_STREAM_CALL: Site = eix_sites::C_SOUND_MANAGER_OPEN__132;
const OPEN_STREAM_BYTES: [u8; 5] = [0xE8, 0x59, 0x3F, 0x00, 0x00];
/// Where the stream record keeps the decoder's table (`SoundOpenStream` stores it at 0x6BABEF).
const TABLE_AT: usize = 0xC;
/// The table's attribute tokens the game asks for: output sample rate, sample width and channels (read at
/// 0x6BB70A, 0x6BB720 and 0x6BB712).
const RATE_TOKEN_AT: usize = 0x2C / 4;
const WIDTH_TOKEN_AT: usize = 0x30 / 4;
const CHANNELS_TOKEN_AT: usize = 0x34 / 4;

type OpenFile = unsafe extern "thiscall" fn(*mut c_void, *const c_char, *mut c_void) -> i32;
type OpenStream = unsafe extern "thiscall" fn(*mut c_void, u32) -> i32;
/// The game's data source, `SoundStreamRead` 0x6BA950: `(user, dest, bytes, offset)`, an offset of -1 reading on.
type Fetch = unsafe extern "system" fn(u32, *mut u8, i32, i32) -> i32;
type AsiOpen = unsafe extern "system" fn(u32, Fetch, u32) -> u32;
type AsiProcess = unsafe extern "system" fn(u32, *mut u8, i32) -> i32;
type AsiSeek = unsafe extern "system" fn(u32, i32) -> i32;
type AsiClose = unsafe extern "system" fn(u32) -> i32;
type AsiAttribute = unsafe extern "system" fn(u32, u32) -> i32;

static OPEN_FILE: AtomicUsize = AtomicUsize::new(0);
static OPEN_STREAM: AtomicUsize = AtomicUsize::new(0);
/// The decoder's own functions, in table order, and the three tokens.
static ORIGINAL: [AtomicUsize; 5] = [const { AtomicUsize::new(0) }; 5];
static TOKENS: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];
/// The mod's `Stream\` folder, ending in `\`; empty without a mod.
static MOD_STREAM: Mutex<Vec<u8>> = Mutex::new(Vec::new());
/// The `.flac` path last given to `OpenFile`, kept until the next open.
static PATH: Mutex<Option<CString>> = Mutex::new(None);
/// The handles of the streams we decode.
static OURS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
/// Decoding failures logged, of which the first [`MAX_WARNINGS`] go to addon.log.
static WARNINGS: AtomicU32 = AtomicU32::new(0);
const MAX_WARNINGS: u32 = 16;

fn warn(what: &str) {
    if WARNINGS.fetch_add(1, Ordering::Relaxed) < MAX_WARNINGS {
        log::warn("flac", what, "");
    }
}

/// The game's data source as `io::Read`: the first read starts at the file's start, later ones read on.
struct Source {
    user: u32,
    fetch: Fetch,
    from_start: bool,
}

impl Read for Source {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let offset = if core::mem::take(&mut self.from_start) {
            0
        } else {
            -1
        };
        let len = i32::try_from(buf.len()).unwrap_or(i32::MAX);
        // SAFETY: the game's own source with its user value, into a buffer of `len` bytes or more.
        let n = unsafe { (self.fetch)(self.user, buf.as_mut_ptr(), len, offset) };
        Ok(usize::try_from(n).unwrap_or(0))
    }
}

/// A stream we decode; without a decoder (a damaged file) it plays silence.
struct Stream {
    user: u32,
    fetch: Fetch,
    decoder: Option<Decoder<Source>>,
}

impl Stream {
    fn decoder(user: u32, fetch: Fetch) -> Option<Decoder<Source>> {
        let source = Source {
            user,
            fetch,
            from_start: true,
        };
        Decoder::new(source)
            .inspect_err(|e| {
                warn(&format!(
                    "a FLAC stream does not decode: {e}; it plays silence"
                ))
            })
            .ok()
    }
}

/// Logs the gain a track's tag sets, or the tag that is no gain. A loop opens the track again and logs nothing.
fn log_gain(decoder: &Decoder<Source>) {
    if let Some((tag, text)) = decoder.invalid_gain() {
        warn(&format!(
            "the {tag} tag \"{text}\" is not a number of decibels from -100 to 100; the track plays at its own level"
        ));
    } else if let Some(db) = decoder.gain_db() {
        log::info("flac", "gain", &format!("{db:+.2} dB"));
    }
}

/// The stream behind `handle` when it is one of ours.
fn ours(handle: u32) -> Option<*mut Stream> {
    let ours = OURS.lock().ok()?;
    ours.contains(&handle)
        .then(|| core::ptr::with_exposed_provenance_mut(handle as usize))
}

/// The decoder's own function in table slot `slot`, as `F`.
///
/// # Safety
/// `F` must be the type of slot `slot`, and the table must have been wrapped.
unsafe fn original<F: Copy>(slot: usize) -> F {
    // SAFETY: the caller guarantees the type; the slot was stored before the table got our functions.
    unsafe { hook::as_fn_at::<F>(ORIGINAL[slot].load(Ordering::Relaxed)) }
}

extern "system" fn open(user: u32, fetch: Fetch, total: u32) -> u32 {
    let mut magic = [0u8; 4];
    // SAFETY: the game's source, as it passed it, into four bytes from the file's start.
    let n = unsafe { fetch(user, magic.as_mut_ptr(), 4, 0) };
    if n == 4 && magic == MAGIC {
        let decoder = Stream::decoder(user, fetch);
        if let Some(decoder) = &decoder {
            log_gain(decoder);
        }
        let stream = Box::new(Stream {
            user,
            fetch,
            decoder,
        });
        let raw = Box::into_raw(stream);
        // A 32-bit process: an address fits the decoder handle the game keeps.
        let Ok(handle) = u32::try_from(raw.expose_provenance()) else {
            // SAFETY: just made by `Box::into_raw`, handed out to no one.
            drop(unsafe { Box::from_raw(raw) });
            return 0;
        };
        if let Ok(mut ours) = OURS.lock() {
            ours.push(handle);
        }
        return handle;
    }
    // Not FLAC: back to the file's start for the decoder of the game's choice.
    // SAFETY: as above; a read of nothing only moves the position.
    unsafe { fetch(user, magic.as_mut_ptr(), 0, 0) };
    // SAFETY: slot 0 is the decoder's open.
    let open = unsafe { original::<AsiOpen>(0) };
    // SAFETY: the game's own arguments.
    unsafe { open(user, fetch, total) }
}

extern "system" fn process(handle: u32, buffer: *mut u8, bytes: i32) -> i32 {
    let Some(stream) = ours(handle) else {
        // SAFETY: slot 1 is the decoder's process.
        let process = unsafe { original::<AsiProcess>(1) };
        // SAFETY: the game's own arguments.
        return unsafe { process(handle, buffer, bytes) };
    };
    // SAFETY: a live stream of ours, used by the game's stream thread alone.
    let stream = unsafe { &mut *stream };
    let Some(decoder) = stream.decoder.as_mut() else {
        return 0;
    };
    let len = usize::try_from(bytes).unwrap_or(0);
    // SAFETY: the game's buffer of `bytes` bytes.
    let out = unsafe { core::slice::from_raw_parts_mut(buffer, len) };
    match decoder.read(out) {
        Ok(n) => i32::try_from(n).unwrap_or(0),
        Err(e) => {
            warn(&format!(
                "a FLAC stream stops decoding: {e}; the rest plays silence"
            ));
            stream.decoder = None;
            0
        }
    }
}

extern "system" fn seek(handle: u32, offset: i32) -> i32 {
    let Some(stream) = ours(handle) else {
        // SAFETY: slot 2 is the decoder's seek.
        let seek = unsafe { original::<AsiSeek>(2) };
        // SAFETY: the game's own arguments.
        return unsafe { seek(handle, offset) };
    };
    // SAFETY: as in `process`.
    let stream = unsafe { &mut *stream };
    // The game seeks to 0 to loop a track, and to -1 (go on from where the source is) after opening one.
    if offset == 0 {
        stream.decoder = Stream::decoder(stream.user, stream.fetch);
    }
    0
}

extern "system" fn close(handle: u32) -> i32 {
    let Some(stream) = ours(handle) else {
        // SAFETY: slot 3 is the decoder's close.
        let close = unsafe { original::<AsiClose>(3) };
        // SAFETY: the game's own argument.
        return unsafe { close(handle) };
    };
    if let Ok(mut ours) = OURS.lock() {
        ours.retain(|&h| h != handle);
    }
    // SAFETY: a stream of ours, made by `Box::into_raw` in `open`, now out of the list.
    drop(unsafe { Box::from_raw(stream) });
    0
}

extern "system" fn attribute(handle: u32, token: u32) -> i32 {
    let Some(stream) = ours(handle) else {
        // SAFETY: slot 4 is the decoder's attribute.
        let attribute = unsafe { original::<AsiAttribute>(4) };
        // SAFETY: the game's own arguments.
        return unsafe { attribute(handle, token) };
    };
    // SAFETY: as in `process`.
    let stream = unsafe { &*stream };
    // A stream without a decoder plays silence at the game's own rate.
    let format = stream.decoder.as_ref().map_or(
        flac::Format {
            sample_rate: 44100,
            channels: 2,
        },
        Decoder::format,
    );
    let value = match token {
        t if t == TOKENS[0].load(Ordering::Relaxed) => format.sample_rate,
        t if t == TOKENS[1].load(Ordering::Relaxed) => 16,
        t if t == TOKENS[2].load(Ordering::Relaxed) => format.channels,
        _ => 0,
    };
    i32::try_from(value).unwrap_or(0)
}

/// `CResourceStream::OpenFile` with the track's `.flac` path when there is one.
extern "thiscall" fn open_file(this: *mut c_void, file: *const c_char, size: *mut c_void) -> i32 {
    // SAFETY: OPEN_FILE holds the function, of this type, once `install` ran.
    let original = unsafe { hook::as_fn_at::<OpenFile>(OPEN_FILE.load(Ordering::Relaxed)) };
    if file.is_null() {
        // SAFETY: the game's own call.
        return unsafe { original(this, file, size) };
    }
    // SAFETY: the joined path the game passes, a NUL-terminated string.
    let bytes = unsafe { CStr::from_ptr(file) }.to_bytes();
    let mod_stream = MOD_STREAM.lock().map(|m| m.clone()).unwrap_or_default();
    let found = flac::candidates(bytes, &mod_stream)
        .into_iter()
        .find(|c| path(c).is_file())
        .and_then(|c| CString::new(c).ok());
    let Some(found) = found else {
        // SAFETY: as above.
        return unsafe { original(this, file, size) };
    };
    log::info(
        "flac",
        "plays",
        &eix_engine::win::ansi_text(found.as_bytes()),
    );
    let Ok(mut kept) = PATH.lock() else {
        // SAFETY: as above.
        return unsafe { original(this, file, size) };
    };
    let flac_path = kept.insert(found).as_ptr();
    // SAFETY: the game's call with our path, which stays until the next open.
    unsafe { original(this, flac_path, size) }
}

/// `SoundOpenStream`, then our functions in the decoder table it set up.
extern "thiscall" fn open_stream(this: *mut c_void, driver: u32) -> i32 {
    // SAFETY: OPEN_STREAM holds the function, of this type, once `install` ran.
    let original = unsafe { hook::as_fn_at::<OpenStream>(OPEN_STREAM.load(Ordering::Relaxed)) };
    // SAFETY: the game's own call.
    let opened = unsafe { original(this, driver) };
    if opened == 0 {
        return opened;
    }
    // SAFETY: a stream record `SoundOpenStream` set up; +0xC holds the decoder's table.
    let table = unsafe {
        this.cast::<u8>()
            .wrapping_add(TABLE_AT)
            .cast::<*mut usize>()
            .read()
    };
    if table.is_null() {
        return opened;
    }
    let ours = [
        open as *const () as usize,
        process as *const () as usize,
        seek as *const () as usize,
        close as *const () as usize,
        attribute as *const () as usize,
    ];
    // SAFETY: the table Miles keeps in the decoder's sample record, heap memory, five function slots then tokens.
    let slots = unsafe { core::slice::from_raw_parts_mut(table, CHANNELS_TOKEN_AT + 1) };
    if slots[0] == ours[0] {
        return opened;
    }
    for (i, &f) in ours.iter().enumerate() {
        ORIGINAL[i].store(slots[i], Ordering::Relaxed);
        slots[i] = f;
    }
    for (i, at) in [RATE_TOKEN_AT, WIDTH_TOKEN_AT, CHANNELS_TOKEN_AT]
        .into_iter()
        .enumerate()
    {
        TOKENS[i].store(
            u32::try_from(slots[at]).unwrap_or(u32::MAX),
            Ordering::Relaxed,
        );
    }
    log::info("flac", "the music decoder takes FLAC streams too", "");
    opened
}

/// Redirects the two calls unless eix.ini's `[Music] Flac` is 0; `mod_dir` is the mod folder, ending in `\`, empty
/// without a mod.
pub(crate) fn install(token: &BeforeWinMain, music: Music, mod_dir: &[u8]) {
    if !music.flac {
        log::info("flac", "off: [Music] Flac is 0", "");
        return;
    }
    if let Ok(mut m) = MOD_STREAM.lock() {
        *m = if mod_dir.is_empty() {
            Vec::new()
        } else {
            [mod_dir, b"Stream\\"].concat()
        };
    }
    let built = [
        (
            "FlacMusic open",
            OPEN_FILE_CALL,
            OPEN_FILE_BYTES,
            open_file as *const () as usize,
            &OPEN_FILE,
        ),
        (
            "FlacMusic decoder",
            OPEN_STREAM_CALL,
            OPEN_STREAM_BYTES,
            open_stream as *const () as usize,
            &OPEN_STREAM,
        ),
    ]
    .into_iter()
    .map(|(label, site, bytes, to, old)| {
        let (write, target) = hook::redirect_call(site, bytes, to)?;
        old.store(target.here(), Ordering::Relaxed);
        Ok(Labelled {
            label: label.to_owned(),
            write,
        })
    })
    .collect::<Result<Vec<_>, hook::HookError>>()
    .map_err(Into::into);
    let mut tally = Tally::default();
    apply_built(token, "flac", "FlacMusic", built, &mut tally);
}

#[cfg(test)]
mod tests {
    use super::*;

    const CALLS: [(Site, [u8; 5], Site); 2] = [
        (
            OPEN_FILE_CALL,
            OPEN_FILE_BYTES,
            eix_sites::C_RESOURCE_STREAM_OPEN_FILE,
        ),
        (
            OPEN_STREAM_CALL,
            OPEN_STREAM_BYTES,
            eix_sites::SOUND_OPEN_STREAM,
        ),
    ];

    #[test]
    fn the_calls_go_to_the_open_and_the_decoder_setup() {
        for (site, bytes, target) in CALLS {
            let rel = i32::from_le_bytes(bytes[1..].try_into().unwrap());
            assert_eq!(
                Site::new((site.offset(5)).ru().wrapping_add_signed(rel as isize)),
                target
            );
        }
    }

    #[test]
    fn spelladdon_writes_nothing_at_the_calls() {
        for (site, bytes, _) in CALLS {
            let ours = site..site.offset(bytes.len());
            for (name, w) in spelladdon::writes() {
                assert!(
                    w.site().offset(w.size()) <= ours.start || ours.end <= w.site(),
                    "SpellAddon's {name} at {:#X}",
                    w.site()
                );
            }
        }
    }
}
