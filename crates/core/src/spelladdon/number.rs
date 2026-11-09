//! The text-to-number conversions SpellAddon 20.6 applies to its string keys: the CRT's `atof` and `atoi` as the
//! module's VS2012 debug CRT has them, and a digit-string parser with decimal commas.

/// The CRT's `isspace` in the "C" locale.
fn is_space(c: u8) -> bool {
    c == b' ' || (0x09..=0x0D).contains(&c)
}

/// The text up to its first NUL.
fn c_str(s: &[u8]) -> &[u8] {
    s.iter().position(|&c| c == 0).map_or(s, |n| &s[..n])
}

/// `atof` in the "C" locale: leading white space, a sign, digits with one optional `.`, then an optional exponent
/// (`e`, `E`, `d` or `D`, a sign, digits). Anything else ends the number; no digit at all gives 0.
///
/// A decimal comma ends the number, so `0,5` is 0. The CRT predates C99's `inf` and `nan`, which read as 0.
pub fn atof(s: &[u8]) -> f64 {
    let s = c_str(s);
    let mut i = s.iter().take_while(|&&c| is_space(c)).count();
    let mut text = String::new();
    if let Some(&sign @ (b'+' | b'-')) = s.get(i) {
        text.push(char::from(sign));
        i += 1;
    }
    let int_digits = s[i..].iter().take_while(|c| c.is_ascii_digit()).count();
    text.push('0');
    text.extend(s[i..i + int_digits].iter().map(|&c| char::from(c)));
    i += int_digits;
    let mut frac_digits = 0;
    if s.get(i) == Some(&b'.') {
        frac_digits = s[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
        text.push('.');
        text.extend(s[i + 1..i + 1 + frac_digits].iter().map(|&c| char::from(c)));
        i += 1 + frac_digits;
    }
    if int_digits + frac_digits == 0 {
        return 0.0;
    }
    if let Some(b'e' | b'E' | b'd' | b'D') = s.get(i) {
        let mut j = i + 1;
        let mut exponent = String::from("e");
        if let Some(&sign @ (b'+' | b'-')) = s.get(j) {
            exponent.push(char::from(sign));
            j += 1;
        }
        let digits = s[j..].iter().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 {
            // Past this many digits the value is 0 or infinite either way; the cut keeps the text short.
            let kept = &s[j..j + digits.min(12)];
            exponent.extend(kept.iter().map(|&c| char::from(c)));
            text.push_str(&exponent);
        }
    }
    // The text is a well-formed decimal by construction.
    text.parse().unwrap_or(0.0)
}

/// `atoi`, which this CRT implements as `strtol(s, NULL, 10)`: leading white space, a sign, decimal digits; a value
/// out of range saturates at `i32::MIN` or `i32::MAX`.
pub fn atoi(s: &[u8]) -> i32 {
    let s = c_str(s);
    let mut i = s.iter().take_while(|&&c| is_space(c)).count();
    let negative = match s.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let mut value: i64 = 0;
    for &c in s[i..].iter().take_while(|c| c.is_ascii_digit()) {
        value = (value * 10 + i64::from(c - b'0')).min(i64::from(u32::MAX) + 1);
    }
    let value = if negative { -value } else { value };
    i32::try_from(value).unwrap_or(if negative { i32::MIN } else { i32::MAX })
}

/// The module's digit-string parser over the window `buf[0..=20]`, read right to left: every digit adds
/// `digit * 10^k` (k counting the digits met so far), every digit met before the first `.` or `,` multiplies the
/// divisor by 10, and anything else is skipped, the string's NUL and the stack filler after it included. The result
/// is the sum divided by the divisor, negated when the text starts with `-` after spaces (the original skips the sign
/// too, so `-2.5` was 2.5).
///
/// Without a point the divisor is 1: `150` and `150.0` are 150, `-2.5` is -2.5 and `0,5` is 0.5.
pub fn digits(buf: &[u8]) -> f64 {
    let negative = buf.iter().find(|&&c| c != b' ') == Some(&b'-');
    let value = unsigned_digits(buf);
    if negative { -value } else { value }
}

fn unsigned_digits(buf: &[u8]) -> f64 {
    const WINDOW: usize = 21;
    let mut sum = 0.0f64;
    let mut scale: i64 = 1;
    let mut divisor = 1.0f64;
    let mut seen_point = false;
    for &c in buf.iter().take(WINDOW).rev() {
        if c.is_ascii_digit() {
            let term = i64::from(c - b'0').wrapping_mul(scale);
            // The 64-bit product goes to the FPU as an unsigned value: its low 63 bits, plus 2^63 for the top bit.
            let high = if term < 0 {
                9_223_372_036_854_775_808.0
            } else {
                0.0
            };
            sum += (term & i64::MAX) as f64 + high;
            scale = scale.wrapping_mul(10);
            if !seen_point {
                divisor *= 10.0;
            }
        } else if c == b'.' || c == b',' {
            seen_point = true;
        }
    }
    sum / if seen_point { divisor } else { 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atof_reads_the_c_locale_prefix() {
        assert_eq!(atof(b"1.25"), 1.25);
        assert_eq!(atof(b"  -3.5xyz"), -3.5);
        assert_eq!(atof(b"+.5"), 0.5);
        assert_eq!(atof(b"7."), 7.0);
        assert_eq!(atof(b"2e3"), 2000.0);
        assert_eq!(atof(b"2D-1"), 0.2);
        assert_eq!(atof(b"2e"), 2.0);
        assert_eq!(atof(b"1e999"), f64::INFINITY);
        assert_eq!(atof(b"0,5"), 0.0);
        assert_eq!(atof(b"0,006666666666"), 0.0);
        assert_eq!(atof(b"inf"), 0.0);
        assert_eq!(atof(b""), 0.0);
        assert_eq!(atof(b"-"), 0.0);
        assert_eq!(atof(b"1.5\0" as &[u8]), 1.5);
        assert_eq!(atof(b"4.5\x009"), 4.5);
        assert_eq!(atof(b"119999999999999900.0") as f32, 1.2e17);
    }

    #[test]
    fn atoi_keeps_the_integer_part_and_saturates() {
        assert_eq!(atoi(b"0.5"), 0);
        assert_eq!(atoi(b"175.0"), 175);
        assert_eq!(atoi(b" -12abc"), -12);
        assert_eq!(atoi(b"+7"), 7);
        assert_eq!(atoi(b"x"), 0);
        assert_eq!(atoi(b"0x10"), 0);
        assert_eq!(atoi(b"99999999999"), i32::MAX);
        assert_eq!(atoi(b"-99999999999"), i32::MIN);
        assert_eq!(atoi(b"2147483647"), i32::MAX);
        assert_eq!(atoi(b"-2147483648"), i32::MIN);
    }

    /// A 32-byte INI buffer as the module has it: stack filler, then the string and its NUL.
    fn buffer(text: &[u8]) -> Vec<u8> {
        let mut b = vec![0xCC; 0x20];
        b[..text.len()].copy_from_slice(text);
        b[text.len()] = 0;
        b
    }

    #[test]
    fn the_digit_parser_divides_by_the_digits_right_of_the_point() {
        assert_eq!(digits(&buffer(b"1.1")), 1.1);
        assert_eq!(digits(&buffer(b"150")), 150.0);
        assert_eq!(digits(&buffer(b"150.0")), 150.0);
        assert_eq!(digits(&buffer(b"0,5")), 0.5);
        assert_eq!(digits(&buffer(b"-2.5")), -2.5);
        assert_eq!(digits(&buffer(b" -0,5")), -0.5);
        assert_eq!(digits(&buffer(b"2-5")), 25.0);
        assert_eq!(digits(&buffer(b"0.00666666")), 0.00666666);
        assert_eq!(digits(&buffer(b"0,006666666666")), 0.006666666666);
        assert_eq!(digits(&buffer(b"1.0245")), 1.0245);
        assert_eq!(digits(&buffer(b"")), 0.0);
    }

    #[test]
    fn the_digit_parser_reads_21_bytes_whatever_the_string() {
        // A digit after the NUL counts as one more digit right of the point: 1, 5, 7 make 157 over 100.
        let mut b = buffer(b"1.5");
        b[10] = b'7';
        assert_eq!(digits(&b), 1.57);
        // Bytes from index 21 on do not count; the 20th power of ten wraps and goes to the FPU as unsigned.
        assert_eq!(digits(&buffer(b"12.000000000000000000999")), 12.0);
        // A point seen before any digit (the rightmost character) leaves the divisor at 1.
        assert_eq!(digits(&buffer(b"25.")), 25.0);
    }
}
