//! WideScreen's layout: the 800x600 interface kept in the middle of a mode wider than 4:3, computed once from
//! `VideoResolution` as the original does before WinMain.

use crate::num::trunc_to_i32;

/// The layout for a wide mode. Layout units are the 800x600 interface's; pixels are the mode's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// The mode's width in pixels.
    pub width: i32,
    /// The mode's height in pixels.
    pub height: i32,
    /// The width of the 4:3 part in the middle, `height * 800 / 600` pixels.
    pub width43: i32,
    /// The pixels on either side of the 4:3 part.
    pub margin: i32,
    /// The margin in layout units, rounded up: the panels move this far towards the screen's edges.
    pub extra: i32,
}

/// What `VideoResolution` gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Not a wide mode (at least 800x600 and wider than 4:3): the group stays off.
    NotWide(String),
    /// A value the original refuses with an error: the group fails.
    Invalid(String),
    /// A wide mode.
    Wide(Layout),
}

/// `VideoResolution` read as `sscanf("%dx%d")` does: each number not read is 0, which the range check refuses.
pub fn scan(text: &str) -> (i32, i32) {
    let (w, rest) = scan_int(text);
    let h = rest
        .and_then(|r| r.strip_prefix('x'))
        .and_then(|r| scan_int(r).0);
    (w.unwrap_or(0), h.unwrap_or(0))
}

/// A `%d`: leading white space, a sign, digits (wrapping as the CRT's conversion does); the rest after it.
fn scan_int(text: &str) -> (Option<i32>, Option<&str>) {
    let t = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0B', '\x0C']);
    let (negative, t) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return (None, None);
    }
    let value = t[..digits].bytes().fold(0i32, |v, d| {
        v.wrapping_mul(10).wrapping_add(i32::from(d - b'0'))
    });
    let value = if negative {
        value.wrapping_neg()
    } else {
        value
    };
    (Some(value), Some(&t[digits..]))
}

impl Mode {
    /// The mode for `VideoResolution` (`None` when the value is missing or not a string).
    pub fn from_resolution(resolution: Option<&str>) -> Self {
        let text = resolution.unwrap_or("");
        let (w, h) = scan(text);
        if !(640..=65535).contains(&w) || !(480..=65535).contains(&h) || w / h < 1 {
            return Mode::Invalid(format!("Invalid resolution: {text}"));
        }
        if w < 800 || h < 600 || 3 * w <= 4 * h {
            return Mode::NotWide(format!(
                "VideoResolution {w}x{h} is not wider than 4:3 and at least 800x600"
            ));
        }
        let width43 = h * 800 / 600;
        let margin = (w - width43) / 2;
        // At WinMain the FPU computes in double precision; the quotient is rounded to a float before its ceiling.
        let ratio = (f64::from(margin) * 800.0 / f64::from(width43)) as f32;
        Mode::Wide(Layout {
            width: w,
            height: h,
            width43,
            margin,
            extra: trunc_to_i32(f64::from(ratio).ceil()),
        })
    }
}

/// The layout's width in layout units.
const LAYOUT_WIDTH: i32 = 800;
/// Its height.
const LAYOUT_HEIGHT: i32 = 600;

/// `a * b / divisor` on the 64-bit product, truncated toward zero, as `imul` then `idiv` do; a quotient outside the
/// `i32` range (where `idiv` faults) keeps its low 32 bits.
fn mul_div(a: i32, b: i32, divisor: i32) -> i32 {
    let q = i64::from(a) * i64::from(b) / i64::from(divisor);
    // Only the low 32 bits exist in EAX; no coordinate the game passes comes near the range.
    q as i32
}

impl Layout {
    /// A layout x in screen pixels: scaled by the 4:3 part's width, and moved past the left margin when `margin` (the
    /// scaling entries called with a non-zero ECX; the margin is left out for sizes and offsets).
    pub fn x_to_screen(&self, x: i32, margin: bool) -> i32 {
        let m = if margin { self.margin } else { 0 };
        mul_div(self.width43, x, LAYOUT_WIDTH).wrapping_add(m)
    }

    /// A layout y in screen pixels, scaled by the mode's height.
    pub fn y_to_screen(&self, y: i32) -> i32 {
        mul_div(self.height, y, LAYOUT_HEIGHT)
    }

    /// A screen x in layout units, the left margin taken off first when `margin`.
    pub fn x_from_screen(&self, x: i32, margin: bool) -> i32 {
        let m = if margin { self.margin } else { 0 };
        mul_div(x.wrapping_sub(m), LAYOUT_WIDTH, self.width43)
    }

    /// A screen y in layout units.
    pub fn y_from_screen(&self, y: i32) -> i32 {
        mul_div(y, LAYOUT_HEIGHT, self.height)
    }

    /// A drawn text's extent in pixels as layout units: by the 4:3 part's width and the mode's height.
    pub fn text_extent(&self, cx: i32, cy: i32) -> (i32, i32) {
        (
            mul_div(cx, LAYOUT_WIDTH, self.width43),
            mul_div(cy, LAYOUT_HEIGHT, self.height),
        )
    }

    /// The projection's x scale, `width43 / width`, divided in double precision and stored as a float (the FPU's
    /// precision before WinMain).
    pub fn axis_scale(&self) -> f32 {
        (f64::from(self.width43) / f64::from(self.width)) as f32
    }

    /// The interface fonts' scale, `width * 0.0075 / height`: the float 0.0075 widened to a double, then multiplied
    /// and divided in double precision and stored as a float.
    pub fn font_scale(&self) -> f32 {
        (f64::from(self.width) * f64::from(0.0075f32) / f64::from(self.height)) as f32
    }

    /// What the global map's pick picture coordinate (half a layout x) loses: half the margin.
    pub fn pick_shift(&self) -> i32 {
        self.margin / 2
    }
}

/// The zone tutorials' pages, `"<text>:<page>"`, whose frames point at the battle screen's left panels.
const LEFT_PANEL_PAGES: [&str; 9] = [
    "zt9:0", "zt12:0", "zt12:1", "zt13:0", "zt14:0", "zt14:2", "zt17:0", "zt20:0", "zt22:0",
];
/// Those whose frames point at its right panels.
const RIGHT_PANEL_PAGES: [&str; 7] = [
    "zt3:2", "zt4:0", "zt4:1", "zt7:0", "zt10:0", "zt10:1", "zt18:0",
];

/// How far a tutorial message's frames move along x: `-extra` for a page pointing at a left panel, `extra` for one
/// pointing at a right panel, else 0. `page` is `"<text>:<page>"`, compared exactly (case included).
pub fn tutorial_frame_shift(page: &[u8], extra: i32) -> i32 {
    if LEFT_PANEL_PAGES.iter().any(|p| p.as_bytes() == page) {
        -extra
    } else if RIGHT_PANEL_PAGES.iter().any(|p| p.as_bytes() == page) {
        extra
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(text: &str) -> Layout {
        match Mode::from_resolution(Some(text)) {
            Mode::Wide(l) => l,
            other @ (Mode::NotWide(_) | Mode::Invalid(_)) => panic!("{text}: {other:?}"),
        }
    }

    #[test]
    fn scanning_follows_percent_d() {
        assert_eq!(scan("1056x768"), (1056, 768));
        assert_eq!(scan(" 1280x 720 junk"), (1280, 720));
        assert_eq!(scan("1280X720"), (1280, 0));
        assert_eq!(scan("bad"), (0, 0));
        assert_eq!(scan(""), (0, 0));
        assert_eq!(scan("-5x+6"), (-5, 6));
    }

    #[test]
    fn the_1056x768_mode_gives_its_values() {
        // 1056x768: 0x320 + 13 = 0x32D at 0x65E7C3.
        assert_eq!(
            wide("1056x768"),
            Layout {
                width: 1056,
                height: 768,
                width43: 1024,
                margin: 16,
                extra: 13
            }
        );
        let l = wide("1920x1080");
        assert_eq!((l.width43, l.margin, l.extra), (1440, 240, 134));
        let l = wide("3840x2160");
        assert_eq!((l.width43, l.margin, l.extra), (2880, 480, 134));
        let l = wide("1024x600");
        assert_eq!((l.width43, l.margin, l.extra), (800, 112, 112));
    }

    #[test]
    fn scaling_keeps_the_layout_in_the_middle() {
        let l = wide("1056x768");
        // The layout's corners land on the 4:3 part's: 16 pixels in from either side.
        assert_eq!(l.x_to_screen(0, true), 16);
        assert_eq!(l.x_to_screen(800, true), 1040);
        assert_eq!(l.x_to_screen(800, false), 1024);
        assert_eq!(l.y_to_screen(600), 768);
        // The panels' edge units reach the screen's edges.
        assert_eq!(l.x_to_screen(-l.extra, true), 0);
        assert_eq!(l.x_to_screen(800 + l.extra, true), 1056);
        assert_eq!(l.x_from_screen(16, true), 0);
        assert_eq!(l.x_from_screen(1040, true), 800);
        assert_eq!(l.x_from_screen(0, true), -12);
        assert_eq!(l.x_from_screen(1024, false), 800);
        assert_eq!(l.y_from_screen(384), 300);
        assert_eq!(l.text_extent(1024, 768), (800, 600));
        assert_eq!(l.text_extent(7, 13), (5, 10));
        // Products past 32 bits divide in 64 bits.
        assert_eq!(l.x_to_screen(3_000_000, false), 3_840_000);
        assert_eq!(l.pick_shift(), 8);
    }

    #[test]
    fn the_1056x768_floats_are_exact() {
        let l = wide("1056x768");
        assert_eq!(l.axis_scale().to_bits(), 0x3F78_3E10);
        assert_eq!(l.font_scale().to_bits(), 0x3C28_F5C2);
    }

    #[test]
    fn tutorial_frames_move_with_their_panels() {
        assert_eq!(tutorial_frame_shift(b"zt9:0", 13), -13);
        assert_eq!(tutorial_frame_shift(b"zt22:0", 13), -13);
        assert_eq!(tutorial_frame_shift(b"zt3:2", 13), 13);
        assert_eq!(tutorial_frame_shift(b"zt18:0", 13), 13);
        assert_eq!(tutorial_frame_shift(b"zt9:1", 13), 0);
        assert_eq!(tutorial_frame_shift(b"ZT9:0", 13), 0);
        assert_eq!(tutorial_frame_shift(b"", 13), 0);
    }

    #[test]
    fn small_or_narrow_modes_are_not_wide_and_bad_ones_invalid() {
        for text in ["1024x768", "800x480", "1000x800", "800x600"] {
            assert!(
                matches!(Mode::from_resolution(Some(text)), Mode::NotWide(_)),
                "{text}"
            );
        }
        for text in ["bad", "799x400", "70000x768", "1056x479", "640x65535"] {
            assert!(
                matches!(Mode::from_resolution(Some(text)), Mode::Invalid(_)),
                "{text}"
            );
        }
        assert!(matches!(Mode::from_resolution(None), Mode::Invalid(_)));
    }
}
