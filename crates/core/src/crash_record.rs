//! The text record the crash handler writes to addon.log before it stops the other threads: where the exception
//! happened, what it accessed, the registers, and the return addresses found on the crashing thread's stack.

use std::fmt;

/// The register names of the registers line, in its order.
pub const REGISTERS: [&str; 10] = [
    "eax", "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp", "eip", "eflags",
];

/// The most return addresses the record lists.
pub const MAX_RETURNS: usize = 16;

/// An address, with the module it lies in and its offset there, and what of ours put code there, when known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The address.
    pub address: u32,
    /// The module's file name and the address's offset from the module's base.
    pub module: Option<(String, u32)>,
    /// The patch group, engine fix or SpellAddon feature whose site or stub holds the address.
    pub owner: Option<String>,
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:08X}", self.address)?;
        let module = self
            .module
            .as_ref()
            .map(|(name, offset)| format!("{name}+0x{offset:X}"));
        let parts: Vec<String> = module.into_iter().chain(self.owner.clone()).collect();
        if !parts.is_empty() {
            write!(f, " ({})", parts.join(", "))?;
        }
        Ok(())
    }
}

/// Whether the 7 bytes that end where a stack value points end in a call instruction: a direct `call rel32`, or an
/// indirect `call` (`FF /2`) of any addressing form. A value that passes is a likely return address; a stale value
/// left on the stack by an earlier call passes too, so the record calls them "found on the stack".
pub fn follows_call(before: [u8; 7]) -> bool {
    if before[2] == 0xE8 {
        return true;
    }
    (2..=7).any(|len| {
        let p = 7 - len;
        let modrm = before[p + 1];
        before[p] == 0xFF
            && (modrm >> 3) & 7 == 2
            && indirect_call_len(modrm, before.get(p + 2).copied()) == len
    })
}

/// The length of an `FF /2` call with this ModRM byte and, where one follows, this SIB byte.
fn indirect_call_len(modrm: u8, sib: Option<u8>) -> usize {
    let rm = modrm & 7;
    match modrm >> 6 {
        3 => 2,
        0 if rm == 5 => 6,
        0 if rm == 4 => match sib {
            Some(s) if s & 7 == 5 => 7,
            _ => 3,
        },
        0 => 2,
        1 if rm == 4 => 4,
        1 => 3,
        _ if rm == 4 => 7,
        _ => 6,
    }
}

/// The record's lines: the exception and where, the access for an access violation or an in-page error
/// (`ExceptionInformation` 0 and 1: the kind and the address), the registers in [`REGISTERS`]' order, and the return
/// addresses found on the stack, innermost first.
pub fn lines(
    code: u32,
    at: &Place,
    access: Option<(u32, u32)>,
    registers: Option<[u32; 10]>,
    returns: &[Place],
) -> Vec<String> {
    let mut out = vec![format!("unhandled exception 0x{code:08X} at {at}")];
    if let Some((kind, target)) = access
        && matches!(code, 0xC000_0005 | 0xC000_0006)
    {
        let what = match kind {
            0 => "reading",
            1 => "writing",
            8 => "executing",
            _ => "accessing",
        };
        out.push(format!("access violation: {what} 0x{target:08X}"));
    }
    if let Some(values) = registers {
        let pairs: Vec<String> = REGISTERS
            .iter()
            .zip(values)
            .map(|(name, v)| format!("{name}={v:08X}"))
            .collect();
        out.push(format!("registers: {}", pairs.join(" ")));
    }
    if !returns.is_empty() {
        let places: Vec<String> = returns
            .iter()
            .take(MAX_RETURNS)
            .map(Place::to_string)
            .collect();
        out.push(format!(
            "return addresses found on the stack: {}",
            places.join(", ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `bytes`, the instruction that ends at the return address, right-aligned in the 7 bytes before it.
    fn before(bytes: &[u8]) -> [u8; 7] {
        let mut b = [0x90; 7];
        b[7 - bytes.len()..].copy_from_slice(bytes);
        b
    }

    #[test]
    fn every_call_form_is_found() {
        for call in [
            &[0xE8, 0x10, 0x20, 0x30, 0x40][..],         // call rel32
            &[0xFF, 0xD0],                               // call eax
            &[0xFF, 0x11],                               // call [ecx]
            &[0xFF, 0x50, 0x08],                         // call [eax+8]
            &[0xFF, 0x14, 0x24],                         // call [esp]
            &[0xFF, 0x54, 0x24, 0x08],                   // call [esp+8]
            &[0xFF, 0x15, 0x60, 0xB3, 0x73, 0x00],       // call [0x73B360]
            &[0xFF, 0x90, 0x00, 0x01, 0x00, 0x00],       // call [eax+0x100]
            &[0xFF, 0x94, 0x24, 0x00, 0x01, 0x00, 0x00], // call [esp+0x100]
            &[0xFF, 0x14, 0x85, 0x00, 0x10, 0x40, 0x00], // call [eax*4+0x401000]
        ] {
            assert!(follows_call(before(call)), "{call:02X?}");
        }
    }

    #[test]
    fn what_is_no_call_is_not_found() {
        for other in [
            &[0x8B, 0x45, 0x08][..],   // mov eax, [ebp+8]
            &[0xFF, 0xE0],             // jmp eax (FF /4)
            &[0xFF, 0x25, 0, 0, 0, 0], // jmp [disp32]
            &[0xFF, 0x50],             // a call [eax+disp8] cut short
            &[0x90],
        ] {
            assert!(!follows_call(before(other)), "{other:02X?}");
        }
    }

    #[test]
    fn the_record_names_the_place_the_access_the_registers_and_the_returns() {
        let at = Place {
            address: 0x004B_9538,
            module: Some(("game.exe".to_owned(), 0xB_9538)),
            owner: Some("WideScreen's site 0x4B9530+8".to_owned()),
        };
        let returns = [
            Place {
                address: 0x0045_73AE,
                module: Some(("game.exe".to_owned(), 0x5_73AE)),
                owner: None,
            },
            Place {
                address: 0x7000_0010,
                module: None,
                owner: Some("a stub of QuestSort, +0x10".to_owned()),
            },
            Place {
                address: 0x7100_0000,
                module: None,
                owner: None,
            },
        ];
        let registers = [1, 2, 3, 4, 5, 6, 7, 8, 0x004B_9538, 0x246];
        assert_eq!(
            lines(0xC000_0005, &at, Some((0, 0x3C)), Some(registers), &returns),
            [
                "unhandled exception 0xC0000005 at 0x004B9538 (game.exe+0xB9538, WideScreen's site 0x4B9530+8)",
                "access violation: reading 0x0000003C",
                "registers: eax=00000001 ebx=00000002 ecx=00000003 edx=00000004 esi=00000005 edi=00000006 \
                 ebp=00000007 esp=00000008 eip=004B9538 eflags=00000246",
                "return addresses found on the stack: 0x004573AE (game.exe+0x573AE), 0x70000010 (a stub of \
                 QuestSort, +0x10), 0x71000000",
            ]
        );
    }

    #[test]
    fn other_exceptions_have_no_access_line_and_parts_left_out_stay_out() {
        let at = Place {
            address: 0x10,
            module: None,
            owner: None,
        };
        assert_eq!(
            lines(0xC000_0094, &at, Some((0, 0)), None, &[]),
            ["unhandled exception 0xC0000094 at 0x00000010"]
        );
    }
}
