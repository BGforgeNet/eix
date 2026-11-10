//! The logic of eix's engine fixes that needs no game memory (see `docs/engine.md`).

use std::sync::OnceLock;

/// LowercaseTable: what the C runtime's `_tolower_lk` returned for each byte under one locale, `L` being the locale
/// state the call reads.
#[derive(Debug)]
pub struct LowerTable<L> {
    locale: L,
    lower: [i32; 256],
}

impl<L: Copy + Eq> LowerTable<L> {
    /// The table for `locale`, from one call of `original` per byte.
    pub fn build(locale: L, mut original: impl FnMut(i32) -> i32) -> Self {
        let mut lower = [0; 256];
        for (byte, slot) in (0..=u8::MAX).zip(&mut lower) {
            *slot = original(i32::from(byte));
        }
        Self { locale, lower }
    }

    /// The locale it was built under.
    pub fn locale(&self) -> L {
        self.locale
    }

    /// `original(c)` as built, for a byte `c` under the table's own locale.
    pub fn get(&self, locale: L, c: i32) -> Option<i32> {
        if locale != self.locale {
            return None;
        }
        self.lower.get(usize::try_from(c).ok()?).copied()
    }
}

/// `_tolower_lk(c)` under the set locale `locale`: from `table`, which the first call builds under its locale, or
/// from `original` for what the table does not hold (not a byte, or another locale than the table's).
pub fn lower<L: Copy + Eq>(
    table: &OnceLock<LowerTable<L>>,
    locale: L,
    c: i32,
    original: impl Fn(i32) -> i32,
) -> i32 {
    if !(0..=255).contains(&c) {
        return original(c);
    }
    table
        .get_or_init(|| LowerTable::build(locale, &original))
        .get(locale, c)
        .unwrap_or_else(|| original(c))
}

/// A display mode: its size and bits per pixel. The engine's `ddraw::Mode` has the same shape (the engine builds
/// without std, so it cannot use this one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayMode {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bits per pixel.
    pub bits: u32,
}

/// FullScreenFallback: the mode to set when the display refuses `wanted`: the smallest of `offered` at `wanted`'s
/// depth that holds it whole (by area, then the narrower), `wanted` itself left out; none when no mode is that large.
pub fn fallback_mode(wanted: DisplayMode, offered: &[DisplayMode]) -> Option<DisplayMode> {
    offered
        .iter()
        .filter(|m| {
            m.bits == wanted.bits
                && m.width >= wanted.width
                && m.height >= wanted.height
                && **m != wanted
        })
        .min_by_key(|m| (u64::from(m.width) * u64::from(m.height), m.width))
        .copied()
}

/// The most tick steps SmoothCamera holds back: one tick, so the camera never trails the game by more.
const CAMERA_MAX_PENDING: f32 = 1.0;

/// SmoothCamera: of the tick steps held back (`pending`, in ticks), what a frame update of `step` takes along, and
/// what stays held back: as much as the frame's own step, and anything past one tick.
pub fn camera_release(pending: f32, step: f32) -> (f32, f32) {
    let mut release = pending.min(step.max(0.0));
    let left = pending - release;
    if left > CAMERA_MAX_PENDING {
        release += left - CAMERA_MAX_PENDING;
        return (release, CAMERA_MAX_PENDING);
    }
    (release, left)
}

/// UnitShadowResolution: the side of a unit's shadow texture for the setting's `value`, `None` for a value the game's
/// renderer cannot take. 64 is the game's own.
pub fn unit_shadow_size(value: u32) -> Option<u32> {
    matches!(value, 64 | 128 | 256).then_some(value)
}

/// The window messages of the focus fixes.
const WM_DESTROY: u32 = 0x0002;
const WM_MOVE: u32 = 0x0003;
const WM_SIZE: u32 = 0x0005;
const WM_ACTIVATE: u32 = 0x0006;
const WM_SETFOCUS: u32 = 0x0007;
/// `WM_SIZE`'s `wParam` for a minimised window.
const SIZE_MINIMIZED: u32 = 1;

/// Whether `message` is `WM_ACTIVATE` for a window that loses the activation to another one (`WA_INACTIVE`, and not
/// the minimising that the game handles itself): the game's window procedure then pauses.
pub fn is_focus_loss(message: u32, wparam: u32) -> bool {
    message == WM_ACTIVATE && wparam == 0
}

/// What ClipCursorToWindow does for a window message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorClip {
    /// Confine the cursor to the client area.
    Set,
    /// Confine it if the window is the foreground window, else release it: the area moved or changed size.
    Refresh,
    /// Let the cursor go.
    Release,
    /// Nothing changes.
    Keep,
}

/// What ClipCursorToWindow does for `message` with `wparam`, `windowed` telling whether the game runs in a window
/// (full screen is the game's own).
pub fn cursor_clip(message: u32, wparam: u32, windowed: bool) -> CursorClip {
    if !windowed {
        return CursorClip::Release;
    }
    match message {
        // `WA_ACTIVE` or `WA_CLICKACTIVE`; the high word is set for a minimised window.
        WM_ACTIVATE if matches!(wparam, 1 | 2) => CursorClip::Set,
        WM_ACTIVATE | WM_DESTROY => CursorClip::Release,
        WM_SETFOCUS => CursorClip::Set,
        WM_SIZE if wparam == SIZE_MINIMIZED => CursorClip::Release,
        WM_SIZE | WM_MOVE => CursorClip::Refresh,
        _ => CursorClip::Keep,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    #[test]
    fn a_frame_takes_along_at_most_its_own_step() {
        assert_eq!(camera_release(1.0, 0.25), (0.25, 0.75));
        assert_eq!(camera_release(0.1, 0.25), (0.1, 0.0));
        assert_eq!(camera_release(0.0, 0.25), (0.0, 0.0));
    }

    #[test]
    fn more_than_one_tick_held_back_goes_at_once() {
        // A long run of frames shorter than their tick (or none): the camera never trails by more than one tick.
        assert_eq!(camera_release(2.5, 0.25), (1.5, 1.0));
        assert_eq!(camera_release(1.5, 0.0), (0.5, 1.0));
    }

    #[test]
    fn over_a_second_the_camera_gets_every_step() {
        // 140 frames a second against 55 ms ticks, the tick step held back and released frame by frame.
        let frame = 1000.0 / 140.0 / 55.0;
        let (mut pending, mut given, mut held, mut largest) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let mut since_tick = 0.0f32;
        for _ in 0..140 {
            since_tick += frame;
            if since_tick >= 1.0 {
                since_tick -= 1.0;
                pending += 1.0;
                held += 1.0;
            }
            let (release, left) = camera_release(pending, frame);
            pending = left;
            given += release;
            largest = largest.max(release);
        }
        // Nothing lost: what was held back went out or is still pending, at most one tick of it.
        assert!((held - given - pending).abs() < 1e-4);
        assert!(pending <= 1.0, "{pending} pending");
        // No lump: no frame took along more than its own step, where the game added a whole tick's.
        assert!(largest <= frame + 1e-6, "{largest} in one frame");
    }

    fn mode(width: u32, height: u32, bits: u32) -> DisplayMode {
        DisplayMode {
            width,
            height,
            bits,
        }
    }

    #[test]
    fn the_fallback_is_the_smallest_mode_that_holds_the_wanted_one_at_its_depth() {
        let offered = [
            mode(800, 600, 32),
            mode(1280, 1024, 32),
            mode(1024, 768, 16),
            mode(1024, 768, 32),
            mode(1920, 1080, 32),
        ];
        assert_eq!(
            fallback_mode(mode(1024, 600, 32), &offered),
            Some(mode(1024, 768, 32))
        );
        assert_eq!(
            fallback_mode(mode(1024, 600, 16), &offered),
            Some(mode(1024, 768, 16))
        );
        assert_eq!(
            fallback_mode(mode(1366, 768, 32), &offered),
            Some(mode(1920, 1080, 32))
        );
    }

    #[test]
    fn no_fallback_without_a_mode_that_large_at_that_depth() {
        let offered = [mode(1024, 768, 32), mode(1920, 1080, 16)];
        assert_eq!(fallback_mode(mode(1280, 720, 32), &offered), None);
        assert_eq!(fallback_mode(mode(1024, 600, 8), &offered), None);
        assert_eq!(fallback_mode(mode(1024, 600, 32), &[]), None);
    }

    #[test]
    fn the_refused_mode_itself_is_no_fallback() {
        // Offered but refused (a lost device, a busy display): setting it again cannot help.
        assert_eq!(
            fallback_mode(mode(1024, 768, 32), &[mode(1024, 768, 32)]),
            None
        );
    }

    #[test]
    fn of_two_modes_of_one_area_the_narrower_wins() {
        let offered = [mode(1600, 900, 32), mode(1200, 1200, 32)];
        assert_eq!(
            fallback_mode(mode(1024, 600, 32), &offered),
            Some(mode(1200, 1200, 32))
        );
    }

    /// A stand-in for a locale's lowering with an upper half (cp1251's Cyrillic block and its YO pair), counting the
    /// calls. Values above the bytes are scrambled, so that a table answer for one would show.
    fn model(calls: &AtomicU32, c: i32) -> i32 {
        calls.fetch_add(1, Ordering::SeqCst);
        match c {
            0x41..=0x5A | 0xC0..=0xDF => c + 0x20,
            0xA8 => 0xB8,
            _ if c > 0xFF => c ^ 0x5A5A,
            _ => c,
        }
    }

    #[test]
    fn every_input_gets_what_the_original_returns() {
        let calls = AtomicU32::new(0);
        let table = OnceLock::new();
        for c in -70_000..140_000 {
            assert_eq!(
                lower(&table, 1u32, c, |c| model(&calls, c)),
                model(&AtomicU32::new(0), c),
                "c = {c}"
            );
        }
        // The bytes reach the original only while the table is built, once each.
        assert_eq!(calls.load(Ordering::SeqCst), 256 + 70_000 + (140_000 - 256));
    }

    #[test]
    fn a_byte_is_answered_from_the_table_after_the_first_call() {
        let calls = AtomicU32::new(0);
        let table = OnceLock::new();
        assert_eq!(lower(&table, 7u32, 0xC1, |c| model(&calls, c)), 0xE1);
        assert_eq!(calls.load(Ordering::SeqCst), 256);
        for c in 0..=255 {
            lower(&table, 7u32, c, |c| model(&calls, c));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 256);
        assert_eq!(table.get().map(LowerTable::locale), Some(7));
    }

    #[test]
    fn another_locale_than_the_tables_goes_to_the_original() {
        let table = OnceLock::new();
        let calls = AtomicU32::new(0);
        assert_eq!(lower(&table, 1u32, b'A'.into(), |c| model(&calls, c)), 0x61);
        let other = AtomicU32::new(0);
        // Under locale 2 the original maps nothing; the table of locale 1 must not answer.
        assert_eq!(
            lower(&table, 2u32, b'A'.into(), |c| {
                other.fetch_add(1, Ordering::SeqCst);
                c
            }),
            0x41
        );
        assert_eq!(other.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn threads_racing_to_the_first_call_agree() {
        let table = OnceLock::new();
        let calls = AtomicU32::new(0);
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    for c in 0..=255 {
                        assert_eq!(
                            lower(&table, 3u32, c, |c| model(&calls, c)),
                            model(&AtomicU32::new(0), c)
                        );
                    }
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 256);
    }

    #[test]
    fn only_the_three_shadow_sizes_are_taken() {
        for size in [64, 128, 256] {
            assert_eq!(unit_shadow_size(size), Some(size));
        }
        for other in [0, 1, 32, 63, 65, 100, 192, 255, 257, 512, u32::MAX] {
            assert_eq!(unit_shadow_size(other), None, "{other}");
        }
    }

    #[test]
    fn only_a_plain_deactivation_is_a_focus_loss() {
        assert!(is_focus_loss(WM_ACTIVATE, 0));
        // WA_ACTIVE, WA_CLICKACTIVE, and the deactivation of a minimised window (high word set).
        for wparam in [1, 2, 1 << 16, (1 << 16) | 1] {
            assert!(!is_focus_loss(WM_ACTIVATE, wparam), "{wparam:#x}");
        }
        assert!(!is_focus_loss(WM_SETFOCUS, 0));
    }

    #[test]
    fn the_cursor_is_clipped_while_active_and_let_go_otherwise() {
        use CursorClip::*;
        assert_eq!(cursor_clip(WM_ACTIVATE, 1, true), Set);
        assert_eq!(cursor_clip(WM_ACTIVATE, 2, true), Set);
        assert_eq!(cursor_clip(WM_ACTIVATE, 0, true), Release);
        assert_eq!(cursor_clip(WM_ACTIVATE, 1 << 16, true), Release);
        assert_eq!(cursor_clip(WM_ACTIVATE, (1 << 16) | 1, true), Release);
        assert_eq!(cursor_clip(WM_SETFOCUS, 0, true), Set);
        assert_eq!(cursor_clip(WM_SIZE, 0, true), Refresh);
        assert_eq!(cursor_clip(WM_SIZE, SIZE_MINIMIZED, true), Release);
        assert_eq!(cursor_clip(WM_MOVE, 0, true), Refresh);
        assert_eq!(cursor_clip(WM_DESTROY, 0, true), Release);
        assert_eq!(cursor_clip(0x0200, 0, true), Keep);
        // Full screen is the game's: no message sets a clip.
        for message in [WM_ACTIVATE, WM_SETFOCUS, WM_SIZE, WM_MOVE, 0x0200] {
            assert_eq!(cursor_clip(message, 1, false), Release, "{message:#x}");
        }
    }
}
