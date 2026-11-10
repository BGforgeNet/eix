//! BriefingText: a briefing dialogue reopens its own text before each phrase and reads on where the last phrase
//! ended. `CBriefingScreen::StartPhrases` 0x608210 opens `briefing <topic>` in the text manager the interface shares
//! ([0x7AF788]), and each `CBriefingScreen::NextPhrase` 0x604C50 reads the next phrase at that manager's cursor, frames
//! later. Code that runs in between reads its own texts through the same manager: a guest joining the host's game
//! makes `ServerOnJoin` 0x41ADB0 fetch "lmp_player_connected" through `CInterface3D::MessageText` 0x5F8C90, whose
//! `CTextManager::GetString` 0x4E0020 leaves no text open but its length, and the next phrase's
//! `CTextManager::PeekWord` reads address 0 (0x4DF7F7).
#![expect(
    unsafe_code,
    reason = "reading and reopening the game's text manager between the dialogue's phrases"
)]

use std::ffi::CString;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use eix_engine::game::text::{self, TEXTS, TextManager};
use eix_engine::game::ui::BriefingScreen;
use eix_engine::hook::{self, Context, Mode};

use crate::groups::{BuildError, Labelled};
use crate::log;
use crate::setup::Setup;

/// `NextPhrase`'s first instructions, `push -1; push 0x72AE46`: the handler runs before them, so the return address
/// is stack word 0 and the screen is in ECX.
const ENTRY: usize = eix_sites::C_BRIEFING_SCREEN_NEXT_PHRASE;
const ENTRY_BYTES: [u8; 7] = [0x6A, 0xFF, 0x68, 0x46, 0xAE, 0x72, 0x00];

/// `add esp, 0x88` before `NextPhrase`'s only `ret 4` (0x605DCF); every path of the function passes it.
const EXIT: usize = eix_sites::C_BRIEFING_SCREEN_NEXT_PHRASE__1179;
const EXIT_BYTES: [u8; 6] = [0x81, 0xC4, 0x88, 0x00, 0x00, 0x00];

/// The return address of `StartPhrases`' call of `NextPhrase` (0x608404): that first phrase follows the opening of
/// the text in the same call, so it reads as the game left it.
const FIRST_PHRASE_RETURN: u32 = eix_sites::C_BRIEFING_SCREEN_START_PHRASES__1F9 as u32;

/// The prefix `StartPhrases` puts before the topic (the string at 0x7906E4).
const KEY_PREFIX: &[u8] = b"briefing ";

/// Where the last phrase left the dialogue: its screen and text, the cursor's offset in it, and the manager's state
/// then, to tell whether something else opened a text since.
#[derive(Clone)]
struct Place {
    screen: usize,
    key: CString,
    offset: u32,
    text: usize,
    cursor: usize,
    length: u32,
}

static PLACE: Mutex<Option<Place>> = Mutex::new(None);

/// The screen of the phrase being read, from the entry hook to the exit hook (the game has one thread).
static SCREEN: AtomicUsize = AtomicUsize::new(0);

/// The key `StartPhrases` opens for `topic`, or none when the topic holds a NUL.
fn text_key(topic: &[u8]) -> Option<CString> {
    CString::new([KEY_PREFIX, topic].concat()).ok()
}

/// The offset to read on from in a reopened text of `length` bytes: past its end only if it shrank, then its end.
fn resume_offset(offset: u32, length: u32) -> u32 {
    offset.min(length)
}

/// The key of `screen`'s dialogue.
///
/// # Safety
/// `screen` must be the live briefing screen `NextPhrase` runs for.
unsafe fn screen_key(screen: usize) -> Option<CString> {
    // SAFETY: the caller guarantees a live screen.
    let screen = unsafe { &*core::ptr::with_exposed_provenance::<BriefingScreen>(screen) };
    // SAFETY: the topic of a live screen is a constructed CString.
    text_key(unsafe { screen.topic.as_bytes() })
}

/// The manager's open text, cursor and length, as addresses.
///
/// # Safety
/// `texts` must be the live text manager, on the game's thread.
unsafe fn state(texts: *const TextManager) -> (usize, usize, u32) {
    // SAFETY: the caller guarantees the manager.
    let m = unsafe { &*texts };
    (m.text.addr(), m.cursor.addr(), m.length)
}

fn place() -> std::sync::MutexGuard<'static, Option<Place>> {
    PLACE.lock().unwrap_or_else(PoisonError::into_inner)
}

extern "C" fn phrase_start(_: usize, ctx: &mut Context) {
    let screen = ctx.ecx as usize;
    SCREEN.store(screen, Ordering::Relaxed);
    if ctx.stack_word(0).map(|w| eix_sites::ru(w as usize) as u32) == Some(FIRST_PHRASE_RETURN) {
        return;
    }
    // SAFETY: NextPhrase runs on the game's thread, after WinMain created the manager.
    let texts = unsafe { TEXTS.read() };
    if texts.is_null() {
        return;
    }
    // SAFETY: ECX at NextPhrase's entry is its briefing screen.
    let Some(key) = (unsafe { screen_key(screen) }) else {
        return;
    };
    // A copy, so no lock is held while the game's code runs.
    let Some(p) = place()
        .as_ref()
        .filter(|p| p.screen == screen && p.key == key)
        .cloned()
    else {
        return;
    };
    // SAFETY: `texts` is the live manager; the game's thread is the only one using it.
    unsafe { reopen(texts, &p) };
}

/// Opens `p`'s text again and puts the cursor at `p`'s offset; a text no source holds any more reads as empty, so
/// the dialogue ends there.
///
/// # Safety
/// `texts` must be the live text manager, on the game's thread.
unsafe fn reopen(texts: *mut TextManager, p: &Place) {
    // SAFETY: the caller guarantees the manager.
    let (text, cursor, length) = unsafe { state(texts) };
    let moved = (text, cursor, length) != (p.text, p.cursor, p.length);
    let key = p.key.to_string_lossy();
    // SAFETY: as above; the key is NUL-terminated.
    let opened = unsafe { text::open(texts, &p.key) };
    // SAFETY: as above; no other reference to the manager is live.
    let m = unsafe { &mut *texts };
    if opened {
        let offset = resume_offset(p.offset, m.length);
        // `offset` is at most the length Open set, so the cursor stays within the text or at its end.
        m.cursor = m.text.wrapping_add(offset as usize);
        if moved {
            log::info(
                "fixes",
                &format!(
                    "\"{key}\": the text manager held another text (text {text:#x}, cursor {cursor:#x}, length \
                     {length}); reopened at {offset}"
                ),
                "BriefingText",
            );
        }
    } else {
        // Open cleared the text and the cursor; a zero length makes every read find the end.
        m.length = 0;
        log::warn(
            "fixes",
            &format!("\"{key}\" is no longer there: the dialogue ends"),
            "BriefingText",
        );
    }
}

extern "C" fn phrase_end(_: usize, _: &mut Context) {
    let screen = SCREEN.load(Ordering::Relaxed);
    // SAFETY: as in `phrase_start`.
    let texts = unsafe { TEXTS.read() };
    let now = if texts.is_null() {
        None
    } else {
        // SAFETY: the live manager, on the game's thread.
        let (text, cursor, length) = unsafe { state(texts) };
        let offset = cursor
            .checked_sub(text)
            .and_then(|o| u32::try_from(o).ok())
            .filter(|&o| text != 0 && o <= length);
        // SAFETY: `screen` is the screen this phrase was read for, still live at its end.
        let key = offset.and_then(|_| unsafe { screen_key(screen) });
        offset.zip(key).map(|(offset, key)| Place {
            screen,
            key,
            offset,
            text,
            cursor,
            length,
        })
    };
    *place() = now;
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![
        Labelled {
            label: "BriefingText #0".to_owned(),
            write: hook::context_hook(ENTRY, &ENTRY_BYTES, Mode::After, phrase_start, 0)?,
        },
        Labelled {
            label: "BriefingText #1".to_owned(),
            write: hook::context_hook(EXIT, &EXIT_BYTES, Mode::Before, phrase_end, 0)?,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_sites_become_jumps_to_their_stubs() {
        let w = build(&Setup::default()).unwrap();
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].write.site(), ENTRY);
        assert_eq!(w[0].write.expected(), &ENTRY_BYTES);
        assert_eq!(w[0].write.new_bytes()[0], 0xE9);
        assert_eq!(w[1].write.site(), EXIT);
        assert_eq!(w[1].write.expected(), &EXIT_BYTES);
        assert_eq!(w[1].write.new_bytes()[0], 0xE9);
    }

    #[test]
    fn the_first_phrase_returns_after_the_five_byte_call() {
        assert_eq!(
            FIRST_PHRASE_RETURN,
            eix_sites::C_BRIEFING_SCREEN_START_PHRASES__1F4 as u32 + 5
        );
    }

    #[test]
    fn the_key_is_the_one_start_phrases_opens() {
        assert_eq!(text_key(b"z3q1_1").unwrap().as_bytes(), b"briefing z3q1_1");
        assert_eq!(text_key(b"").unwrap().as_bytes(), b"briefing ");
        assert!(text_key(b"a\0b").is_none());
    }

    #[test]
    fn the_offset_stays_within_the_reopened_text() {
        assert_eq!(resume_offset(120, 4000), 120);
        assert_eq!(resume_offset(4000, 4000), 4000);
        assert_eq!(resume_offset(5000, 4000), 4000);
        assert_eq!(resume_offset(0, 0), 0);
    }
}
