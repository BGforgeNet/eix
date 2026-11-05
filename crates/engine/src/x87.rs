//! Floating point as game.exe and addon.dll compute it: on the x87 unit, under the control word in effect.
//!
//! The game runs its x87 unit at a precision its renderer sets (not necessarily the 53 bits of SSE2 doubles), and
//! each x87 operation rounds to that precision. A formula whose result feeds game state therefore runs here one x87
//! instruction per operation, its intermediates kept as full 80-bit register values ([`Ext`]) and rounded to `f32`
//! or `f64` only where the original stores them.
#![expect(unsafe_code, reason = "x87 instructions through inline assembly")]

use core::arch::asm;

/// An x87 register's value, stored without rounding (`fstp tbyte`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ext([u8; 10]);

/// Runs one x87 instruction on `a` and the operand, `a` loaded as ST0: `$op` takes `qword ptr [{x}]`.
macro_rules! binary {
    ($name:ident, $op:literal, $doc:literal) => {
        #[doc = $doc]
        #[must_use]
        pub fn $name(self, x: f64) -> Self {
            let mut out = Ext([0; 10]);
            // SAFETY: the three pointers address live values of their sizes; the x87 stack is empty on entry (all
            // eight registers are declared clobbered) and the block pops what it loads.
            unsafe {
                asm!(
                    "fld tbyte ptr [{a}]",
                    concat!($op, " qword ptr [{x}]"),
                    "fstp tbyte ptr [{out}]",
                    a = in(reg) &self.0,
                    x = in(reg) &x,
                    out = in(reg) &mut out.0,
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                    out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                    options(nostack),
                );
            }
            out
        }
    };
}

impl Ext {
    /// `x` loaded into a register (`fld qword`): exact, as is every `f32`, `i32` or `u32` converted to `f64`.
    pub fn new(x: f64) -> Self {
        let mut out = Ext([0; 10]);
        // SAFETY: as in `binary!`.
        unsafe {
            asm!(
                "fld qword ptr [{x}]",
                "fstp tbyte ptr [{out}]",
                x = in(reg) &x,
                out = in(reg) &mut out.0,
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                options(nostack),
            );
        }
        out
    }

    /// An unsigned word loaded as MSVC loads one: `fild` of it as signed, then `fadd` of 2^32 when that is negative,
    /// which rounds under the control word.
    pub fn from_u32(v: u32) -> Self {
        let signed = Self::new(f64::from(v as i32));
        if (v as i32) < 0 {
            signed.fadd(4_294_967_296.0)
        } else {
            signed
        }
    }

    binary!(fadd, "fadd", "`self + x`.");
    binary!(fsub, "fsub", "`self - x`.");
    binary!(fmul, "fmul", "`self * x`.");
    binary!(fdiv, "fdiv", "`self / x`.");
    binary!(fdivr, "fdivr", "`x / self`.");

    /// The value stored as an `f32` (`fstp dword`), rounded under the control word.
    pub fn to_f32(self) -> f32 {
        let mut out = 0f32;
        // SAFETY: as in `binary!`.
        unsafe {
            asm!(
                "fld tbyte ptr [{a}]",
                "fstp dword ptr [{out}]",
                a = in(reg) &self.0,
                out = in(reg) &mut out,
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                options(nostack),
            );
        }
        out
    }

    /// The value stored as an `f64` (`fstp qword`), as a varargs call or `_ftol2` takes it (then
    /// [`crate::float::ftol`]).
    pub fn to_f64(self) -> f64 {
        let mut out = 0f64;
        // SAFETY: as in `binary!`.
        unsafe {
            asm!(
                "fld tbyte ptr [{a}]",
                "fstp qword ptr [{out}]",
                a = in(reg) &self.0,
                out = in(reg) &mut out,
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                options(nostack),
            );
        }
        out
    }

    /// The tag FNSAVE records for a register holding this value: valid, zero or special (NaN, infinity, denormal,
    /// unsupported encodings).
    fn tag(self) -> u16 {
        let exponent = u16::from_le_bytes([self.0[8], self.0[9]]) & 0x7FFF;
        let mut m = [0u8; 8];
        m.copy_from_slice(&self.0[..8]);
        let mantissa = u64::from_le_bytes(m);
        match (exponent, mantissa) {
            (0, 0) => TAG_ZERO,
            (0 | 0x7FFF, _) => TAG_SPECIAL,
            // An exponent without the integer bit is an unnormal, which the unit treats as unsupported.
            (_, m) if m >> 63 == 0 => TAG_SPECIAL,
            _ => TAG_VALID,
        }
    }
}

const TAG_VALID: u16 = 0;
const TAG_ZERO: u16 = 1;
const TAG_SPECIAL: u16 = 2;
const TAG_EMPTY: u16 = 3;

/// The x87 state as FNSAVE stores it in 32-bit protected mode (108 bytes): the environment, then ST(0) to ST(7) in
/// stack order. Only an FPU hook's stub makes one ([`crate::hook::fpu_hook`]); a handler reads and changes the game's
/// stack through it, and the stub restores it with FRSTOR.
#[repr(C)]
#[derive(Debug)]
#[non_exhaustive]
pub struct FpuSave {
    control: u16,
    _reserved2: u16,
    status: u16,
    _reserved6: u16,
    tags: u16,
    _reserved10: u16,
    /// Instruction and operand pointers, the last opcode: restored as saved.
    _pointers: [u32; 4],
    regs: [[u8; 10]; 8],
}

const _: () = {
    assert!(core::mem::offset_of!(FpuSave, control) == 0);
    assert!(core::mem::offset_of!(FpuSave, status) == 4);
    assert!(core::mem::offset_of!(FpuSave, tags) == 8);
    assert!(core::mem::offset_of!(FpuSave, regs) == 28);
    assert!(size_of::<FpuSave>() == 108);
};

impl FpuSave {
    /// The game's control word (precision, rounding, exception masks).
    pub fn control_word(&self) -> u16 {
        self.control
    }

    /// The game's status word.
    pub fn status_word(&self) -> u16 {
        self.status
    }

    fn top(&self) -> usize {
        usize::from((self.status >> 11) & 7)
    }

    fn set_top(&mut self, top: usize) {
        self.status = (self.status & !(7 << 11)) | (((top & 7) as u16) << 11);
    }

    /// The tag of physical register `r`.
    fn tag(&self, r: usize) -> u16 {
        (self.tags >> (2 * (r & 7))) & 3
    }

    fn set_tag(&mut self, r: usize, tag: u16) {
        let shift = 2 * (r & 7);
        self.tags = (self.tags & !(3 << shift)) | (tag << shift);
    }

    /// The physical register that holds ST(`i`).
    fn physical(&self, i: usize) -> usize {
        (self.top() + i) & 7
    }

    /// How many values the game has on the stack: ST(0) up to the first empty register.
    pub fn depth(&self) -> usize {
        (0..8)
            .take_while(|&i| self.tag(self.physical(i)) != TAG_EMPTY)
            .count()
    }

    /// ST(`i`), or `None` when that register is empty or `i` is past ST(7).
    pub fn st(&self, i: usize) -> Option<Ext> {
        (i < 8 && self.tag(self.physical(i)) != TAG_EMPTY).then(|| Ext(self.regs[i]))
    }

    /// Replaces ST(`i`); false (nothing changed) when that register is empty or `i` is past ST(7).
    pub fn set_st(&mut self, i: usize, value: Ext) -> bool {
        if self.st(i).is_none() {
            return false;
        }
        self.regs[i] = value.0;
        self.set_tag(self.physical(i), value.tag());
        true
    }

    /// Pushes `value`, as `fld` does; false (nothing changed) when the stack is full. The condition codes stay.
    pub fn push(&mut self, value: Ext) -> bool {
        let top = (self.top() + 7) & 7;
        if self.tag(top) != TAG_EMPTY {
            return false;
        }
        self.regs.copy_within(0..7, 1);
        self.regs[0] = value.0;
        self.set_top(top);
        self.set_tag(top, value.tag());
        true
    }

    /// Pops ST(0), as `fstp st(0)` does; `None` (nothing changed) when the stack is empty.
    pub fn pop(&mut self) -> Option<Ext> {
        let value = self.st(0)?;
        let top = self.top();
        self.regs.copy_within(1..8, 0);
        self.regs[7] = [0; 10];
        self.set_tag(top, TAG_EMPTY);
        self.set_top(top + 1);
        Some(value)
    }
}

/// Runs one x87 instruction on `self` (ST0) and another register value `x` (ST1): `$op` takes `st(0), st(1)`.
macro_rules! binary_ext {
    ($name:ident, $op:literal, $doc:literal) => {
        #[doc = $doc]
        #[must_use]
        pub fn $name(self, x: Ext) -> Self {
            let mut out = Ext([0; 10]);
            // SAFETY: as in `binary!`; the block loads two registers and pops both.
            unsafe {
                asm!(
                    "fld tbyte ptr [{x}]",
                    "fld tbyte ptr [{a}]",
                    concat!($op, " st(0), st(1)"),
                    "fstp tbyte ptr [{out}]",
                    "fstp st(0)",
                    a = in(reg) &self.0,
                    x = in(reg) &x.0,
                    out = in(reg) &mut out.0,
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                    out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                    options(nostack),
                );
            }
            out
        }
    };
}

/// Runs one x87 instruction without an operand on `self` as ST0.
macro_rules! unary {
    ($name:ident, $op:literal, $doc:literal) => {
        #[doc = $doc]
        #[must_use]
        pub fn $name(self) -> Self {
            let mut out = Ext([0; 10]);
            // SAFETY: as in `binary!`.
            unsafe {
                asm!(
                    "fld tbyte ptr [{a}]",
                    $op,
                    "fstp tbyte ptr [{out}]",
                    a = in(reg) &self.0,
                    out = in(reg) &mut out.0,
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                    out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                    options(nostack),
                );
            }
            out
        }
    };
}

/// The x87 status word's condition bits as `fnstsw ax` puts them in AH: C0 (0x01), C2 (0x04), C3 (0x40).
pub const C0: u16 = 0x0100;
/// See [`C0`].
pub const C2: u16 = 0x0400;
/// See [`C0`].
pub const C3: u16 = 0x4000;

impl Ext {
    binary_ext!(fadd_ext, "fadd", "`self + x`, both register values.");
    binary_ext!(fsub_ext, "fsub", "`self - x`, both register values.");
    binary_ext!(fmul_ext, "fmul", "`self * x`, both register values.");
    binary_ext!(fdiv_ext, "fdiv", "`self / x`, both register values.");
    unary!(fsqrt, "fsqrt", "The square root (`fsqrt`).");
    unary!(fchs, "fchs", "The negation (`fchs`).");
    unary!(fabs, "fabs", "The absolute value (`fabs`).");

    /// The value stored as an int by `fistp dword`: rounded under the control word's rounding mode, and 0x80000000
    /// for a value outside the int range.
    pub fn fistp(self) -> i32 {
        let mut out = 0i32;
        // SAFETY: as in `binary!`.
        unsafe {
            asm!(
                "fld tbyte ptr [{a}]",
                "fistp dword ptr [{out}]",
                a = in(reg) &self.0,
                out = in(reg) &mut out,
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                options(nostack),
            );
        }
        out
    }

    /// The status word after `fcom` of `self` with `x`: [`C0`] set when `self < x`, [`C3`] when equal, all three
    /// when unordered.
    pub fn fcom(self, x: f64) -> u16 {
        let mut out = 0u16;
        // SAFETY: as in `binary!`.
        unsafe {
            asm!(
                "fld tbyte ptr [{a}]",
                "fcomp qword ptr [{x}]",
                "fnstsw word ptr [{out}]",
                a = in(reg) &self.0,
                x = in(reg) &x,
                out = in(reg) &mut out,
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                options(nostack),
            );
        }
        out & (C0 | C2 | C3)
    }

    /// [`fcom`](Self::fcom) with another register value: `fcom st(1)` with `self` in ST0 and `x` in ST1.
    pub fn fcom_ext(self, x: Ext) -> u16 {
        let mut out = 0u16;
        // SAFETY: as in `binary!`; the block loads two registers and pops both.
        unsafe {
            asm!(
                "fld tbyte ptr [{x}]",
                "fld tbyte ptr [{a}]",
                "fcomp st(1)",
                "fnstsw word ptr [{out}]",
                "fstp st(0)",
                a = in(reg) &self.0,
                x = in(reg) &x.0,
                out = in(reg) &mut out,
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                options(nostack),
            );
        }
        out & (C0 | C2 | C3)
    }
}

/// The x87 status word as it is now (`fnstsw`): in a hook handler, the game's, with the TOP of its stack.
pub fn status_word() -> u16 {
    let mut out = 0u16;
    // SAFETY: `fnstsw` stores the status word into a live 2-byte buffer and changes no x87 state.
    unsafe {
        asm!(
            "fnstsw word ptr [{out}]",
            out = in(reg) &mut out,
            options(nostack, preserves_flags),
        );
    }
    out
}

/// The x87 control word as it is now (`fnstcw`): its precision field (bits 8-9: 0 is 24 bits, 2 is 53, 3 is 64) and
/// rounding field (bits 10-11: 0 is to nearest) decide what each x87 operation gives.
pub fn control_word() -> u16 {
    let mut out = 0u16;
    // SAFETY: `fnstcw` stores the control word into a live 2-byte buffer and changes no x87 state.
    unsafe {
        asm!(
            "fnstcw word ptr [{out}]",
            out = in(reg) &mut out,
            options(nostack, preserves_flags),
        );
    }
    out
}

/// Runs `f` with the x87 precision field set to `pc` (0: 24 bits, 2: 53, 3: 64), then restores the control word.
pub fn with_precision<T>(pc: u16, f: impl FnOnce() -> T) -> T {
    let old = control_word();
    let new = (old & !0x0300) | ((pc & 3) << 8);
    // SAFETY: loads a control word that differs from the current one only in its precision field.
    unsafe { asm!("fldcw word ptr [{p}]", p = in(reg) &new, options(nostack)) };
    let out = f();
    // SAFETY: restores the control word.
    unsafe { asm!("fldcw word ptr [{p}]", p = in(reg) &old, options(nostack)) };
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty unit as FNINIT leaves it, with TOP at `top`.
    fn empty(top: usize) -> FpuSave {
        let mut s = FpuSave {
            control: 0x037F,
            _reserved2: 0,
            status: 0,
            _reserved6: 0,
            tags: 0xFFFF,
            _reserved10: 0,
            _pointers: [0; 4],
            regs: [[0; 10]; 8],
        };
        s.set_top(top);
        s
    }

    #[test]
    fn tags_follow_the_value() {
        assert_eq!(Ext::new(1.0).0, [0, 0, 0, 0, 0, 0, 0, 0x80, 0xFF, 0x3F]);
        assert_eq!(Ext::new(1.0).tag(), TAG_VALID);
        assert_eq!(Ext::new(0.0).tag(), TAG_ZERO);
        assert_eq!(Ext::new(-0.0).tag(), TAG_ZERO);
        assert_eq!(Ext::new(f64::INFINITY).tag(), TAG_SPECIAL);
        assert_eq!(Ext::new(f64::NAN).tag(), TAG_SPECIAL);
        assert_eq!(Ext([1, 0, 0, 0, 0, 0, 0, 0, 0, 0]).tag(), TAG_SPECIAL);
        assert_eq!(Ext([0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0x3F]).tag(), TAG_SPECIAL);
    }

    #[test]
    fn push_and_pop_move_top_and_tags() {
        let one = Ext::new(1.0);
        let two = Ext::new(2.0);
        let zero = Ext::new(0.0);
        let mut s = empty(0);
        assert_eq!((s.depth(), s.st(0)), (0, None));
        assert!(s.push(one));
        assert!(s.push(two));
        // TOP moved down twice; physical registers 7 and 6 hold 1.0 and 2.0.
        assert_eq!((s.top(), s.tags), (6, 0x0FFF));
        assert_eq!(
            (s.depth(), s.st(0), s.st(1), s.st(2)),
            (2, Some(two), Some(one), None)
        );
        assert!(s.set_st(1, zero));
        assert_eq!(s.tags, 0x4FFF);
        assert!(!s.set_st(2, one));
        assert_eq!(s.pop(), Some(two));
        assert_eq!((s.top(), s.depth(), s.st(0)), (7, 1, Some(zero)));
        assert_eq!(s.pop(), Some(zero));
        assert_eq!((s.pop(), s.top(), s.tags), (None, 0, 0xFFFF));
    }

    #[test]
    fn a_full_stack_refuses_a_push() {
        let mut s = empty(3);
        for i in 0..8 {
            assert!(s.push(Ext::new(f64::from(i))), "{i}");
        }
        assert_eq!(s.depth(), 8);
        assert!(!s.push(Ext::new(8.0)));
        assert_eq!(s.st(0), Some(Ext::new(7.0)));
        assert_eq!(s.st(7), Some(Ext::new(0.0)));
        assert_eq!(s.st(8), None);
    }

    #[test]
    fn operations_round_under_the_control_word() {
        let third = || Ext::new(1.0).fdiv(3.0).to_f64();
        assert_eq!(with_precision(2, third), 1.0 / 3.0);
        assert_eq!(with_precision(0, third), f64::from(1.0f32 / 3.0));
        // A chain keeps its intermediate unrounded: 2^60 + 1 - 2^60 is exact only with 64 bits.
        let chain = || {
            Ext::new(2f64.powi(60))
                .fadd(1.0)
                .fsub(2f64.powi(60))
                .to_f64()
        };
        assert_eq!(with_precision(3, chain), 1.0);
        assert_eq!(with_precision(2, chain), 0.0);
    }

    #[test]
    fn each_operation_does_what_it_says() {
        let v = with_precision(2, || {
            [
                Ext::new(6.0).fadd(2.0).to_f64(),
                Ext::new(6.0).fsub(2.0).to_f64(),
                Ext::new(6.0).fmul(2.0).to_f64(),
                Ext::new(6.0).fdiv(2.0).to_f64(),
                Ext::new(6.0).fdivr(2.0).to_f64(),
            ]
        });
        assert_eq!(v, [8.0, 4.0, 12.0, 3.0, 2.0 / 6.0]);
        assert_eq!(Ext::new(0.1).to_f32(), 0.1f32);
        assert_eq!(Ext::from_u32(7).to_f64(), 7.0);
        assert_eq!(
            with_precision(2, || Ext::from_u32(u32::MAX).to_f64()),
            4_294_967_295.0
        );
        // With 24 bits the fix-up rounds: 2^32 - 1 becomes 2^32.
        assert_eq!(
            with_precision(0, || Ext::from_u32(u32::MAX).to_f64()),
            4_294_967_296.0
        );
    }

    #[test]
    fn register_operands_unary_operations_and_stores() {
        let (a, b) = (Ext::new(6.0), Ext::new(2.0));
        let v = with_precision(2, || {
            [
                a.fadd_ext(b).to_f64(),
                a.fsub_ext(b).to_f64(),
                a.fmul_ext(b).to_f64(),
                a.fdiv_ext(b).to_f64(),
                Ext::new(9.0).fsqrt().to_f64(),
                a.fchs().to_f64(),
                a.fchs().fabs().to_f64(),
            ]
        });
        assert_eq!(v, [8.0, 4.0, 12.0, 3.0, 3.0, -6.0, 6.0]);
        // Round to nearest, ties to even.
        assert_eq!(Ext::new(2.5).fistp(), 2);
        assert_eq!(Ext::new(3.5).fistp(), 4);
        assert_eq!(Ext::new(-2.6).fistp(), -3);
        assert_eq!(Ext::new(3e9).fistp(), i32::MIN);
        assert_eq!(Ext::new(1.0).fcom(2.0), C0);
        assert_eq!(Ext::new(2.0).fcom(2.0), C3);
        assert_eq!(Ext::new(3.0).fcom(2.0), 0);
        assert_eq!(Ext::new(f64::NAN).fcom(2.0), C0 | C2 | C3);
        let third = Ext::new(1.0).fdiv(3.0);
        assert_eq!(third.fcom_ext(Ext::new(2.0)), C0);
        assert_eq!(third.fcom_ext(third), C3);
        assert_eq!(Ext::new(2.0).fcom_ext(third), 0);
    }

    #[test]
    fn the_status_word_shows_the_stack_top() {
        let top = |sw: u16| (sw >> 11) & 7;
        let before = status_word();
        let mut during = 0u16;
        // SAFETY: the stack is empty on entry (all eight registers are declared clobbered); the block reads the
        // status word with one value loaded and pops it again.
        unsafe {
            asm!(
                "fld1",
                "fnstsw word ptr [{out}]",
                "fstp st(0)",
                out = in(reg) &mut during,
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                options(nostack),
            );
        }
        assert_eq!(top(during), (top(before) + 7) & 7);
        assert_eq!(top(status_word()), top(before));
    }
}
