//! Number conversions as the originals' compiled code does them.

/// A float to an integer, truncated toward zero, as the originals' conversion (`cvttsd2si`, MSVC's `_ftol2_sse`)
/// does: NaN and values outside the `i32` range give 0x80000000, where Rust's `as` saturates.
///
/// A copy of `eix_engine::float::ftol` for platform-free code (engine has no std and builds for Windows only,
/// so neither crate can use the other's); engine's tests pin the two to the same results.
pub fn trunc_to_i32(x: f64) -> i32 {
    // -2^31 - 1 < x < 2^31 truncates into range; the bounds are exact in a double.
    if x > -2_147_483_649.0 && x < 2_147_483_648.0 {
        x as i32
    } else {
        i32::MIN
    }
}

/// A float to an integer as x87 `fistp` does it under the default rounding (to nearest, ties to even): NaN and values
/// that round outside the `i32` range give 0x80000000.
pub fn round_to_i32(x: f64) -> i32 {
    // Adding and taking away 1.5 * 2^52 leaves an integer, rounded to nearest with ties to even, for |x| < 2^51,
    // without the library call `round_ties_even` is on targets before SSE4.1; larger magnitudes and NaN are out of
    // range anyway.
    const SHIFT: f64 = 6_755_399_441_055_744.0;
    if x.abs() < 2_251_799_813_685_248.0 {
        let r = (x + SHIFT) - SHIFT;
        if (-2_147_483_648.0..=2_147_483_647.0).contains(&r) {
            return r as i32;
        }
    }
    i32::MIN
}

/// A float to an integer as the addon's option reader does it (its own C runtime's x87 `_ftol2`):
/// truncated toward zero through 64 bits, of which the low 32 are kept, so a magnitude from 2^31 up wraps. NaN, the
/// infinities and magnitudes from 2^63 up give 0 (the x87 "integer indefinite" has a zero low word). Differs from
/// [`trunc_to_i32`], the conversion of game code and Lua, which gives 0x80000000 outside the `i32` range.
pub fn trunc_to_i32_wrapping(x: f32) -> i32 {
    // 2^63 is exact in an f32; `as i64` truncates toward zero inside the range.
    if x.is_finite() && x.abs() < 9_223_372_036_854_775_808.0 {
        // Keeping the low 32 bits is the conversion's result.
        (x as i64) as i32
    } else {
        0
    }
}

/// The C runtime's `atoi`: leading blanks, an optional sign, then decimal digits up to the first other byte; the
/// value wraps on overflow, as MSVC's accumulation does.
pub fn atoi(text: &[u8]) -> i32 {
    let mut rest = text
        .iter()
        .copied()
        .skip_while(|&c| c == b' ' || (b'\t'..=b'\r').contains(&c))
        .peekable();
    let negative = match rest.peek() {
        Some(b'-') => {
            rest.next();
            true
        }
        Some(b'+') => {
            rest.next();
            false
        }
        _ => false,
    };
    let mut value: i32 = 0;
    for c in rest {
        if !c.is_ascii_digit() {
            break;
        }
        value = value.wrapping_mul(10).wrapping_add(i32::from(c - b'0'));
    }
    if negative {
        value.wrapping_neg()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_range_and_nan_give_the_indefinite_integer() {
        assert_eq!(trunc_to_i32(12.9), 12);
        assert_eq!(trunc_to_i32(-12.9), -12);
        assert_eq!(trunc_to_i32(2_147_483_647.9), i32::MAX);
        assert_eq!(trunc_to_i32(-2_147_483_648.9), i32::MIN);
        assert_eq!(trunc_to_i32(2_147_483_648.0), i32::MIN);
        assert_eq!(trunc_to_i32(-2_147_483_649.0), i32::MIN);
        assert_eq!(trunc_to_i32(f64::NAN), i32::MIN);
        assert_eq!(trunc_to_i32(f64::INFINITY), i32::MIN);
    }

    #[test]
    fn fistp_rounds_ties_to_even_and_gives_the_indefinite_integer_outside() {
        assert_eq!(round_to_i32(0.5), 0);
        assert_eq!(round_to_i32(1.5), 2);
        assert_eq!(round_to_i32(254.5), 254);
        assert_eq!(round_to_i32(-2.5), -2);
        assert_eq!(round_to_i32(2_147_483_647.4), i32::MAX);
        assert_eq!(round_to_i32(2_147_483_647.5), i32::MIN);
        assert_eq!(round_to_i32(-2_147_483_648.5), i32::MIN);
        assert_eq!(round_to_i32(f64::NAN), i32::MIN);
    }

    #[test]
    fn the_option_readers_conversion_keeps_the_low_word() {
        assert_eq!(trunc_to_i32_wrapping(12.9), 12);
        assert_eq!(trunc_to_i32_wrapping(-12.9), -12);
        assert_eq!(trunc_to_i32_wrapping(-0.5), 0);
        // 3e9 is 3_000_000_000 exactly in an f32 (a multiple of 128): its low word read as signed.
        assert_eq!(trunc_to_i32_wrapping(3.0e9), -1_294_967_296);
        assert_eq!(trunc_to_i32_wrapping(-3.0e9), 1_294_967_296);
        // 2^32 has a zero low word; 2^40 + 2^20 keeps its 2^20.
        assert_eq!(trunc_to_i32_wrapping(4_294_967_296.0), 0);
        assert_eq!(
            trunc_to_i32_wrapping(2f32.powi(40) + 2f32.powi(20)),
            1 << 20
        );
        // From 2^63, and for NaN and the infinities, the x87 result has a zero low word.
        assert_eq!(trunc_to_i32_wrapping(9_223_372_036_854_775_808.0), 0);
        assert_eq!(trunc_to_i32_wrapping(f32::MAX), 0);
        assert_eq!(trunc_to_i32_wrapping(f32::NAN), 0);
        assert_eq!(trunc_to_i32_wrapping(f32::INFINITY), 0);
        assert_eq!(trunc_to_i32_wrapping(f32::NEG_INFINITY), 0);
    }

    #[test]
    fn atoi_reads_a_leading_number() {
        assert_eq!(atoi(b"2"), 2);
        assert_eq!(atoi(b"  -17x"), -17);
        assert_eq!(atoi(b"+5"), 5);
        assert_eq!(atoi(b"abc"), 0);
        assert_eq!(atoi(b""), 0);
        assert_eq!(atoi(b"4294967297"), 1);
    }
}
