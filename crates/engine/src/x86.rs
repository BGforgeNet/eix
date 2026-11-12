//! A length decoder for the 32-bit x86 instructions found at hook sites, and relocation of copied instructions.
//!
//! Supported forms (32-bit address size; one prefix at most, in front of a form below that is not a branch: FS, 64, for
//! the SEH frame's `mov fs:[0], r32` in function epilogues, or the operand size, 66, which makes an immediate 16-bit):
//! - one byte: `inc`/`dec`/`push`/`pop` r32 (40-5F), `pushad`/`popad` (60, 61), `nop` and `xchg eax, r32`
//!   (90-97), `cwde`/`cdq` (98, 99), `pushfd`/`popfd`/`sahf`/`lahf` (9C-9F), `ret` (C3), `leave` (C9), `int3` (CC),
//!   the flag setters `cmc`/`clc`/`stc`/`cld`/`std` (F5, F8, F9, FC, FD);
//! - immediates: `push imm8`/`imm32` (6A, 68), `mov r8, imm8` (B0-B7), `mov r32, imm32` (B8-BF), `ret imm16` (C2),
//!   ALU on AL/EAX with an immediate (04 0C 14 1C 24 2C 34 3C A8; 05 0D 15 1D 25 2D 35 3D A9), `mov` with a 32-bit
//!   absolute address (A0-A3);
//! - ModR/M (any addressing form, SIB and displacements included): ALU r/m (00-03, 08-0B, ... 38-3B), `test`
//!   (84, 85), `xchg` (86, 87), `mov` (88-8B), `lea` (8D), `pop r/m` (8F), shifts (C0, C1, D1, D3), `mov r/m, imm`
//!   (C6, C7), `imul` (69, 6B), group 1 (80, 81, 83), `test`/`not`/`neg`/`mul`/`div` (F6, F7), x87 (D8-DF), `inc`/
//!   `dec`/`call`/`jmp`/`push` r/m (FE, FF), and on 0F: `setcc` (90-9F), `imul` (AF), `movzx`/`movsx` (B6, B7, BE,
//!   BF);
//! - relative branches with a 32-bit displacement: `call` (E8), `jmp` (E9), `jcc` (0F 80-8F), which relocation
//!   re-targets.
//!
//! 32-bit code has no instruction-pointer-relative addressing, so every other supported form runs unchanged at any
//! address. `decode` refuses short branches (`jcc rel8`, `jmp rel8`, `loop`, `jecxz`); `decode_in_place` accepts them.
//! Everything else is refused by both.

use core::fmt;

/// One decoded instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Insn {
    /// Its length in bytes.
    pub len: usize,
    /// Where its 32-bit branch displacement starts, for `E8`, `E9` and `0F 8x`; the displacement is its last 4 bytes.
    pub rel32: Option<usize>,
    /// Where its 32-bit memory displacement starts (an absolute address, or a table's base next to a register).
    pub disp32: Option<usize>,
    /// Where its 32-bit immediate starts, `mov eax, [moffs]`'s address included.
    pub imm32: Option<usize>,
}

/// Why code could not be decoded or relocated. `offset` counts from the start of the copied code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeError {
    /// An opcode outside the supported forms.
    Unsupported {
        /// Offset of the instruction.
        offset: usize,
        /// Its first opcode byte (the second for 0F).
        opcode: u8,
        /// Whether it is a two-byte `0F` opcode.
        two_byte: bool,
    },
    /// A branch with an 8-bit displacement, which a copy cannot reach its target with.
    ShortBranch {
        /// Offset of the instruction.
        offset: usize,
    },
    /// The last instruction runs past the end of the code.
    Truncated {
        /// Offset of the instruction.
        offset: usize,
    },
    /// A branch lands inside the copied code, which the hook overwrites.
    IntoCopied {
        /// Offset of the instruction.
        offset: usize,
        /// The branch target.
        target: usize,
    },
}

impl fmt::Display for CodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported {
                offset,
                opcode,
                two_byte: false,
            } => write!(f, "+{offset}: unsupported opcode {opcode:02X}"),
            Self::Unsupported {
                offset,
                opcode,
                two_byte: true,
            } => write!(f, "+{offset}: unsupported opcode 0F {opcode:02X}"),
            Self::ShortBranch { offset } => {
                write!(f, "+{offset}: short branch cannot be relocated")
            }
            Self::Truncated { offset } => {
                write!(f, "+{offset}: instruction runs past the copied bytes")
            }
            Self::IntoCopied { offset, target } => {
                write!(
                    f,
                    "+{offset}: branch to {target:#x} lands inside the copied bytes"
                )
            }
        }
    }
}

impl core::error::Error for CodeError {}

/// What follows an opcode.
#[derive(Clone, Copy)]
enum Operands {
    None,
    Imm(usize),
    ModRm(usize),
    Rel32,
}

fn one_byte(op: u8, modrm: Option<u8>) -> Option<Operands> {
    use Operands::{Imm, ModRm, None, Rel32};
    let reg = modrm.map(|m| (m >> 3) & 7);
    Some(match op {
        0x40..=0x5F
        | 0x60
        | 0x61
        | 0x90..=0x99
        | 0x9C..=0x9F
        | 0xC3
        | 0xC9
        | 0xCC
        | 0xF5
        | 0xF8
        | 0xF9
        | 0xFC
        | 0xFD => None,
        0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C | 0xA8 | 0x6A | 0xB0..=0xB7 => Imm(1),
        0x05
        | 0x0D
        | 0x15
        | 0x1D
        | 0x25
        | 0x2D
        | 0x35
        | 0x3D
        | 0xA9
        | 0x68
        | 0xB8..=0xBF
        | 0xA0..=0xA3 => Imm(4),
        0xC2 => Imm(2),
        0x00..=0x03
        | 0x08..=0x0B
        | 0x10..=0x13
        | 0x18..=0x1B
        | 0x20..=0x23
        | 0x28..=0x2B
        | 0x30..=0x33
        | 0x38..=0x3B
        | 0x84..=0x8B
        | 0x8D
        | 0x8F
        | 0xD1
        | 0xD3
        | 0xD8..=0xDF
        | 0xFE
        | 0xFF => ModRm(0),
        0x6B | 0x80 | 0x83 | 0xC0 | 0xC1 | 0xC6 => ModRm(1),
        0x69 | 0x81 | 0xC7 => ModRm(4),
        // Only `test` (/0) carries an immediate in groups F6 and F7; a missing ModR/M byte is reported as truncated.
        0xF6 => ModRm(if reg == Some(0) { 1 } else { 0 }),
        0xF7 => ModRm(if reg == Some(0) { 4 } else { 0 }),
        0xE8 | 0xE9 => Rel32,
        _ => return Option::None,
    })
}

fn two_byte(op: u8) -> Option<Operands> {
    Some(match op {
        0x80..=0x8F => Operands::Rel32,
        0x90..=0x9F | 0xAF | 0xB6 | 0xB7 | 0xBE | 0xBF => Operands::ModRm(0),
        _ => return None,
    })
}

/// Length of a ModR/M byte with its SIB byte and displacement, and where a 32-bit displacement starts in it.
fn modrm_len(modrm: u8, sib: Option<u8>) -> Option<(usize, Option<usize>)> {
    let md = modrm >> 6;
    let rm = modrm & 7;
    if md == 3 {
        return Some((1, None));
    }
    let mut len = 1;
    let mut disp32 = None;
    if rm == 4 {
        len += 1;
        if md == 0 && sib? & 7 == 5 {
            disp32 = Some(len);
            len += 4;
        }
    } else if md == 0 && rm == 5 {
        disp32 = Some(len);
        len += 4;
    }
    if md == 2 {
        disp32 = Some(len);
    }
    Some((
        len + match md {
            1 => 1,
            2 => 4,
            _ => 0,
        },
        disp32,
    ))
}

/// Decodes the instruction at the start of `code`; `offset` only labels errors.
pub fn decode(code: &[u8], offset: usize) -> Result<Insn, CodeError> {
    decode_with(code, offset, false)
}

/// The whole instructions `code` starts with ([`decode_in_place`]), each with its offset: up to the first that does
/// not decode or runs past the end (bytes after a write complete it, or data follows the code).
pub fn whole_insns(code: &[u8]) -> impl Iterator<Item = (usize, Insn)> + '_ {
    let mut at = 0;
    core::iter::from_fn(move || {
        let insn = decode_in_place(code.get(at..)?, at).ok()?;
        let start = at;
        at += insn.len;
        (at <= code.len()).then_some((start, insn))
    })
}

/// [`decode`], which also takes a short branch (as an instruction with no address field): for code that stays where
/// it is relative to its targets.
pub fn decode_in_place(code: &[u8], offset: usize) -> Result<Insn, CodeError> {
    decode_with(code, offset, true)
}

fn decode_with(code: &[u8], offset: usize, short_ok: bool) -> Result<Insn, CodeError> {
    let first = *code.first().ok_or(CodeError::Truncated { offset })?;
    if matches!(first, 0x64 | 0x66) {
        // The segment changes no address a relocation touches, the operand size only how long an immediate is; a
        // prefixed branch is left out, as nothing needs one (and 66 would cut the target to 16 bits).
        let unsupported = CodeError::Unsupported {
            offset,
            opcode: first,
            two_byte: false,
        };
        if matches!(
            code.get(1),
            Some(0x64 | 0x66 | 0x70..=0x7F | 0xEB | 0xE0..=0xE3)
        ) {
            return Err(unsupported);
        }
        let insn = decode_body(&code[1..], offset, short_ok, first == 0x66)?;
        if insn.rel32.is_some() {
            return Err(unsupported);
        }
        let after_prefix = |at: Option<usize>| at.map(|a| a + 1);
        return Ok(Insn {
            len: insn.len + 1,
            rel32: None,
            disp32: after_prefix(insn.disp32),
            imm32: after_prefix(insn.imm32),
        });
    }
    decode_body(code, offset, short_ok, false)
}

/// An instruction with no prefix; `word`: its operands are 16-bit (a 66 prefix came before it).
fn decode_body(code: &[u8], offset: usize, short_ok: bool, word: bool) -> Result<Insn, CodeError> {
    let truncated = CodeError::Truncated { offset };
    let first = *code.first().ok_or(truncated.clone())?;
    if matches!(first, 0x70..=0x7F | 0xEB | 0xE0..=0xE3) {
        if short_ok && code.len() >= 2 {
            return Ok(Insn {
                len: 2,
                rel32: None,
                disp32: None,
                imm32: None,
            });
        }
        return Err(CodeError::ShortBranch { offset });
    }
    let (opcode_len, operands) = if first == 0x0F {
        let op = *code.get(1).ok_or(truncated.clone())?;
        let operands = two_byte(op).ok_or(CodeError::Unsupported {
            offset,
            opcode: op,
            two_byte: true,
        })?;
        (2, operands)
    } else {
        let operands = one_byte(first, code.get(1).copied()).ok_or(CodeError::Unsupported {
            offset,
            opcode: first,
            two_byte: false,
        })?;
        // A 32-bit immediate is 16-bit then; `mov` with an absolute address (A0-A3) keeps its 32-bit address.
        let operands = match operands {
            Operands::Imm(4) if word && !matches!(first, 0xA0..=0xA3) => Operands::Imm(2),
            Operands::ModRm(4) if word => Operands::ModRm(2),
            o @ (Operands::None | Operands::Imm(_) | Operands::ModRm(_) | Operands::Rel32) => o,
        };
        (1, operands)
    };
    let insn = match operands {
        Operands::None => Insn {
            len: opcode_len,
            rel32: None,
            disp32: None,
            imm32: None,
        },
        Operands::Imm(n) => Insn {
            len: opcode_len + n,
            rel32: None,
            disp32: None,
            imm32: (n == 4).then_some(opcode_len),
        },
        Operands::Rel32 => Insn {
            len: opcode_len + 4,
            rel32: Some(opcode_len),
            disp32: None,
            imm32: None,
        },
        Operands::ModRm(imm) => {
            let modrm = *code.get(opcode_len).ok_or(truncated.clone())?;
            let (len, disp32) =
                modrm_len(modrm, code.get(opcode_len + 1).copied()).ok_or(truncated.clone())?;
            Insn {
                len: opcode_len + len + imm,
                rel32: None,
                disp32: disp32.map(|d| opcode_len + d),
                imm32: (imm == 4).then_some(opcode_len + len),
            }
        }
    };
    if insn.len > code.len() {
        return Err(truncated);
    }
    Ok(insn)
}

/// Splits `code` into whole supported instructions.
pub fn instructions(code: &[u8]) -> Result<alloc::vec::Vec<Insn>, CodeError> {
    let mut out = alloc::vec::Vec::new();
    let mut at = 0;
    while at < code.len() {
        let insn = decode(&code[at..], at)?;
        out.push(insn);
        at += insn.len;
    }
    Ok(out)
}

/// `code`, taken from address `from`, rewritten to run at address `to`: every 32-bit branch keeps its target.
///
/// The length is unchanged. A branch into `from + 1 .. from + code.len()` is refused: the hook overwrites those bytes.
pub fn relocate(code: &[u8], from: usize, to: usize) -> Result<alloc::vec::Vec<u8>, CodeError> {
    let mut out = code.to_vec();
    let mut at = 0;
    for insn in instructions(code)? {
        if let Some(rel_at) = insn.rel32 {
            let field = at + rel_at;
            let mut rel = [0u8; 4];
            rel.copy_from_slice(&code[field..field + 4]);
            let end = |base: usize| (base + at + insn.len) as u32;
            let target = end(from).wrapping_add(u32::from_le_bytes(rel)) as usize;
            if target > from && target < from + code.len() {
                return Err(CodeError::IntoCopied { offset: at, target });
            }
            let new = (target as u32).wrapping_sub(end(to));
            out[field..field + 4].copy_from_slice(&new.to_le_bytes());
        }
        at += insn.len;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lengths(code: &[u8]) -> Vec<usize> {
        instructions(code).unwrap().iter().map(|i| i.len).collect()
    }

    #[test]
    fn decodes_real_site_forms() {
        // push ebp; mov ebp, esp; sub esp, 0xC
        assert_eq!(lengths(&[0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x0C]), [1, 2, 3]);
        // mov eax, [esp+0x110]; mov [esp+0xDC], eax
        assert_eq!(
            lengths(&[
                0x8B, 0x84, 0x24, 0x10, 0x01, 0, 0, 0x89, 0x84, 0x24, 0xDC, 0, 0, 0
            ]),
            [7, 7]
        );
        // fld dword [0x73E124]; fmul dword [esi+0x24]; fld qword [esp+0x34]
        assert_eq!(
            lengths(&[
                0xD9, 0x05, 0x24, 0xE1, 0x73, 0x00, 0xD8, 0x4E, 0x24, 0xDD, 0x44, 0x24, 0x34
            ]),
            [6, 3, 4]
        );
        // mov dword [0x791EE0], 0x790A70; cmp dword [esi+0x25C], 1; push -1; push 0x72BCBC
        assert_eq!(
            lengths(&[
                0xC7, 0x05, 0xE0, 0x1E, 0x79, 0, 0x70, 0x0A, 0x79, 0, 0x83, 0xBE, 0x5C, 0x02, 0, 0,
                0x01, 0x6A, 0xFF, 0x68, 0xBC, 0xBC, 0x72, 0
            ]),
            [10, 7, 2, 5]
        );
        // sete cl; cmp al, 0x40; ret 4; test byte [eax], 1; not eax
        assert_eq!(
            lengths(&[
                0x0F, 0x94, 0xC1, 0x3C, 0x40, 0xC2, 0x04, 0x00, 0xF6, 0x00, 0x01, 0xF7, 0xD0
            ]),
            [3, 2, 3, 3, 2]
        );
        // mov eax, [ebx*4+0x7C0000] (SIB without base)
        assert_eq!(lengths(&[0x8B, 0x04, 0x9D, 0, 0, 0x7C, 0]), [7]);
        // mov eax, fs:[0]; mov fs:[0], esp (an SEH prologue)
        assert_eq!(
            lengths(&[0x64, 0xA1, 0, 0, 0, 0, 0x64, 0x89, 0x25, 0, 0, 0, 0]),
            [6, 7]
        );
        // mov fs:[0], ecx; add esp, 0x1C; ret 0xC (an SEH epilogue)
        assert_eq!(
            lengths(&[
                0x64, 0x89, 0x0D, 0, 0, 0, 0, 0x83, 0xC4, 0x1C, 0xC2, 0x0C, 0x00
            ]),
            [7, 3, 3]
        );
        // pushad; xchg eax, esi; lahf; cld; popad
        assert_eq!(lengths(&[0x60, 0x96, 0x9F, 0xFC, 0x61]), [1, 1, 1, 1, 1]);
    }

    #[test]
    fn the_operand_size_prefix_shortens_immediates_only() {
        // mov ax, 0x1234; mov word [0x7AF5B8], 7; cmp word [esi+4], 0x10; mov ax, [0x7AF5B8]; movzx eax, word [ecx]
        assert_eq!(
            lengths(&[
                0x66, 0xB8, 0x34, 0x12, 0x66, 0xC7, 0x05, 0xB8, 0xF5, 0x7A, 0, 7, 0, 0x66, 0x83,
                0x7E, 0x04, 0x10, 0x66, 0xA1, 0xB8, 0xF5, 0x7A, 0, 0x66, 0x0F, 0xB7, 0x01
            ]),
            [4, 9, 5, 6, 4]
        );
        // The address of `mov word [0x7AF5B8], 7` is found after the prefix; a 16-bit immediate is no address.
        assert_eq!(
            decode(&[0x66, 0xC7, 0x05, 0xB8, 0xF5, 0x7A, 0, 7, 0], 0),
            Ok(Insn {
                len: 9,
                rel32: None,
                disp32: Some(3),
                imm32: None
            })
        );
        assert_eq!(
            decode(&[0x66, 0xA1, 0xB8, 0xF5, 0x7A, 0], 0).map(|i| i.imm32),
            Ok(Some(2))
        );
    }

    #[test]
    fn a_prefix_takes_no_branch_and_no_second_prefix() {
        for prefix in [0x64, 0x66] {
            let refused = CodeError::Unsupported {
                offset: 0,
                opcode: prefix,
                two_byte: false,
            };
            assert_eq!(decode(&[prefix, 0xE8, 0, 0, 0, 0], 0), Err(refused.clone()));
            assert_eq!(
                decode_in_place(&[prefix, 0x74, 0x02], 0),
                Err(refused.clone())
            );
            assert_eq!(decode(&[prefix, 0x64, 0x90], 0), Err(refused.clone()));
            assert_eq!(decode(&[prefix, 0x66, 0x90], 0), Err(refused));
            assert_eq!(
                decode(&[prefix], 0),
                Err(CodeError::Truncated { offset: 0 })
            );
        }
    }

    #[test]
    fn refuses_what_it_cannot_copy() {
        assert_eq!(
            decode(&[0x74, 0x09], 3),
            Err(CodeError::ShortBranch { offset: 3 })
        );
        assert_eq!(
            decode(&[0xEB, 0x00], 0),
            Err(CodeError::ShortBranch { offset: 0 })
        );
        assert_eq!(
            decode(&[0xCD, 0x2E], 0),
            Err(CodeError::Unsupported {
                offset: 0,
                opcode: 0xCD,
                two_byte: false
            })
        );
        assert_eq!(
            decode(&[0x0F, 0x05], 0),
            Err(CodeError::Unsupported {
                offset: 0,
                opcode: 0x05,
                two_byte: true
            })
        );
        assert_eq!(decode(&[0xF7], 2), Err(CodeError::Truncated { offset: 2 }));
        // The site length cuts `mov eax, [esp+0x110]` in two.
        assert_eq!(
            instructions(&[0x8B, 0x84, 0x24, 0x10, 0x01]),
            Err(CodeError::Truncated { offset: 0 })
        );
    }

    #[test]
    fn relocates_rel32_branches() {
        // Sample addresses below game.exe's image, which code names only through eix-sites.
        let from = 0x0030_1000;
        let to = 0x1000_0000;
        // call 0x302000; jmp 0x300000; je 0x301100 (each relative to its end)
        let mut code = vec![0xE8];
        code.extend_from_slice(&(0x0030_2000u32 - (from as u32 + 5)).to_le_bytes());
        code.push(0xE9);
        code.extend_from_slice(&(0x0030_0000u32.wrapping_sub(from as u32 + 10)).to_le_bytes());
        code.extend_from_slice(&[0x0F, 0x84]);
        code.extend_from_slice(&(0x0030_1100u32 - (from as u32 + 16)).to_le_bytes());
        let out = relocate(&code, from, to).unwrap();
        let target = |at: usize, end: usize| {
            let rel = u32::from_le_bytes(out[at..at + 4].try_into().unwrap());
            (to as u32 + end as u32).wrapping_add(rel)
        };
        assert_eq!(out[0], 0xE8);
        assert_eq!(target(1, 5), 0x0030_2000);
        assert_eq!(out[5], 0xE9);
        assert_eq!(target(6, 10), 0x0030_0000);
        assert_eq!(out[10..12], [0x0F, 0x84]);
        assert_eq!(target(12, 16), 0x0030_1100);
        // Position-independent code comes back as it was.
        assert_eq!(
            relocate(&[0x55, 0x8B, 0xEC], from, to).unwrap(),
            [0x55, 0x8B, 0xEC]
        );
    }

    #[test]
    fn refuses_branches_into_the_copy() {
        // nop; jmp to the site's start: that is where the hook's own jump begins, so it is a valid target.
        let code = [0x90, 0xE9, 0xFA, 0xFF, 0xFF, 0xFF];
        let out = relocate(&code, 0x1000, 0x2000).unwrap();
        assert_eq!(out[2..], (0x1000u32.wrapping_sub(0x2006)).to_le_bytes());
        // A jump to the site's second byte lands inside the hook's jump.
        let into = [0xE9, 0xFC, 0xFF, 0xFF, 0xFF];
        assert_eq!(
            relocate(&into, 0x1000, 0x2000),
            Err(CodeError::IntoCopied {
                offset: 0,
                target: 0x1001
            })
        );
    }
}
