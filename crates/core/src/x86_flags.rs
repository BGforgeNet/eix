//! The arithmetic flags of x86 instructions, for hooks that stand in for a compare whose flags the game's next
//! branch reads.

/// Carry, parity, adjust, zero, sign and overflow: the bits a `cmp` sets in EFLAGS.
pub const ARITHMETIC: u32 = 0x08D5;

/// `eflags` with its arithmetic flags replaced by those of `cmp a, b` (32-bit operands); the other bits are kept.
pub fn cmp(eflags: u32, a: u32, b: u32) -> u32 {
    let r = a.wrapping_sub(b);
    let mut f = 0;
    if a < b {
        f |= 0x0001; // CF
    }
    if (r as u8).count_ones().is_multiple_of(2) {
        f |= 0x0004; // PF: even parity of the low byte
    }
    if (a ^ b ^ r) & 0x10 != 0 {
        f |= 0x0010; // AF
    }
    if r == 0 {
        f |= 0x0040; // ZF
    }
    if r & 0x8000_0000 != 0 {
        f |= 0x0080; // SF
    }
    if (a ^ b) & (a ^ r) & 0x8000_0000 != 0 {
        f |= 0x0800; // OF
    }
    (eflags & !ARITHMETIC) | f
}

#[cfg(test)]
mod tests {
    use super::*;

    const CF: u32 = 0x1;
    const ZF: u32 = 0x40;
    const SF: u32 = 0x80;
    const OF: u32 = 0x800;

    /// `jge`: taken when SF equals OF.
    fn ge(f: u32) -> bool {
        (f & SF != 0) == (f & OF != 0)
    }

    #[test]
    fn signed_and_unsigned_orders() {
        for (a, b) in [
            (0i32, 0i32),
            (5, 100),
            (100, 5),
            (-1, 1),
            (1, -1),
            (i32::MIN, 1),
            (i32::MAX, -1),
        ] {
            let f = cmp(0, a as u32, b as u32);
            assert_eq!(ge(f), a >= b, "{a} >= {b}");
            assert_eq!(f & ZF != 0, a == b);
            assert_eq!(f & CF != 0, (a as u32) < (b as u32));
        }
    }

    #[test]
    fn other_bits_are_kept() {
        // IF (0x200), the reserved bit 1 and DF (0x400) stay; the old arithmetic bits go.
        let f = cmp(0x0602 | ARITHMETIC, 7, 7);
        assert_eq!(f & !ARITHMETIC, 0x0602);
        // 7 - 7 = 0: ZF and PF set, nothing else.
        assert_eq!(f & ARITHMETIC, 0x44);
        // 0x10 - 1 = 0xF: a borrow out of bit 4 (AF), four bits set (PF).
        assert_eq!(cmp(0, 0x10, 1) & ARITHMETIC, 0x14);
    }
}
