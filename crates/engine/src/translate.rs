//! Another build than RU 1.06: the code names RU 1.06 addresses, in constants and inside the bytes it expects and
//! writes; these turn such bytes into the running build's. Code bytes are decoded, and each branch target and 32-bit
//! address field is translated with [`eix_sites::here`]; a data site's 4-byte value is translated when it is an
//! address in the image.
#![expect(unsafe_code, reason = "reading the bytes a site holds")]

use alloc::vec::Vec;

use eix_sites::{Site, in_image};

use crate::x86;

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    let field: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(field))
}

fn put(bytes: &mut [u8], at: usize, value: u32) {
    if let Some(field) = bytes.get_mut(at..at + 4) {
        field.copy_from_slice(&value.to_le_bytes());
    }
}

/// Whether `site` lies in game.exe's code.
fn in_code(site: usize) -> bool {
    (eix_sites::TEXT_SECTION.ru()..eix_sites::RDATA_SECTION.ru()).contains(&site)
}

/// RU 1.06 address `ru` in the running build.
fn here(ru: usize) -> usize {
    Site::new(ru).here()
}

/// `bytes`, code written for RU address `site`, moved to `map(site)`; `None` when they do not decode as whole
/// instructions.
fn rebase(site: usize, bytes: &[u8], map: impl Fn(usize) -> usize) -> Option<Vec<u8>> {
    let to = map(site);
    let mut out = bytes.to_vec();
    let mut at = 0;
    while at < bytes.len() {
        let insn = x86::decode_in_place(&bytes[at..], at).ok()?;
        let end = at + insn.len;
        if let Some(r) = insn.rel32 {
            let rel = word(bytes, at + r)?;
            let target = ((site + end) as u32).wrapping_add(rel) as usize;
            put(
                &mut out,
                at + r,
                (map(target) as u32).wrapping_sub((to + end) as u32),
            );
        }
        for field in [insn.disp32, insn.imm32].into_iter().flatten() {
            let value = word(bytes, at + field)? as usize;
            if in_image(value) {
                put(&mut out, at + field, map(value) as u32);
            }
        }
        at = end;
    }
    Some(out)
}

fn translated_with(site: usize, bytes: &[u8], map: impl Fn(usize) -> usize) -> Vec<u8> {
    if in_code(site) {
        return rebase(site, bytes, map).unwrap_or_else(|| bytes.to_vec());
    }
    match word(bytes, 0) {
        Some(value) if bytes.len() == 4 && in_image(value as usize) => {
            (map(value as usize) as u32).to_le_bytes().to_vec()
        }
        _ => bytes.to_vec(),
    }
}

/// `bytes` written for RU address `site`, as the running build would hold them: code rebased, a data address
/// translated; `bytes` unchanged where neither applies.
pub(crate) fn translated(site: Site, bytes: &[u8]) -> Vec<u8> {
    translated_with(site.ru(), bytes, here)
}

fn equivalent_with(site: usize, bytes: &[u8], found: &[u8], map: impl Fn(usize) -> usize) -> bool {
    if bytes.len() != found.len() {
        return false;
    }
    let to = map(site);
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == found[i] {
            i += 1;
            continue;
        }
        let covered = (i.saturating_sub(3)..=i).find(|&w| {
            let (Some(ru), Some(now)) = (word(bytes, w), word(found, w)) else {
                return false;
            };
            let absolute = in_image(ru as usize) && map(ru as usize) as u32 == now;
            // A branch out of the image (into a stub) keeps its target and changes its displacement.
            let target = ((site + w + 4) as u32).wrapping_add(ru) as usize;
            let relative = (map(target) as u32).wrapping_sub((to + w + 4) as u32) == now;
            absolute || relative
        });
        match covered {
            Some(w) => i = w + 4,
            None => return false,
        }
    }
    true
}

/// The bytes a 4-byte field can reach on either side of a site.
const REACH: usize = 3;

/// Whether `found` (read at `here(site)`) is `bytes` (written for RU address `site`) translated: equal but for 4-byte
/// fields that hold an address or a branch displacement whose translation the found field is. Needs no decoding, so
/// it also holds for part of an instruction; a site that cuts a field (the low byte of an address operand) is judged
/// with the bytes around it, which the site does not cover and so must be the same in both.
///
/// # Safety
/// `here(site)`, with [`REACH`] bytes on either side, must be mapped, readable memory that no other thread writes
/// meanwhile, when `site` lies in game.exe's image.
pub(crate) unsafe fn matches(site: Site, bytes: &[u8], found: &[u8]) -> bool {
    let site = site.ru();
    if found == bytes || equivalent_with(site, bytes, found, here) {
        return true;
    }
    if !in_image(site) || bytes.len() != found.len() {
        return false;
    }
    let at = here(site);
    // SAFETY: the caller's guarantee.
    let read = |from: usize| unsafe { core::slice::from_raw_parts(from as *const u8, REACH) };
    let (before, after) = (read(at - REACH), read(at + bytes.len()));
    let ru = [before, bytes, after].concat();
    let now = [before, found, after].concat();
    equivalent_with(site - REACH, &ru, &now, here)
}

/// What the running build holds at RU address `site` when it is `bytes` translated, else `bytes` (a mismatch the
/// caller reports).
///
/// # Safety
/// As for [`matches`], with `here(site)..here(site) + bytes.len()` readable too.
pub(crate) unsafe fn expected(site: Site, bytes: &[u8]) -> Vec<u8> {
    // SAFETY: the caller's guarantee.
    let found = unsafe { core::slice::from_raw_parts(site.ptr::<u8>(), bytes.len()) };
    // SAFETY: as above.
    if unsafe { matches(site, bytes, found) } {
        found.to_vec()
    } else {
        bytes.to_vec()
    }
}

fn written_with(
    site: usize,
    expected: &[u8],
    found: Option<&[u8]>,
    new: &[u8],
    map: impl Fn(usize) -> usize,
) -> Vec<u8> {
    let mut out = translated_with(site, new, map);
    if let Some(found) = found.filter(|f| f.len() == new.len() && expected.len() == new.len()) {
        for (i, b) in out.iter_mut().enumerate() {
            if new[i] == expected[i] {
                *b = found[i];
            }
        }
    }
    out
}

/// `new`, to be written over `expected` at RU address `site`, for the running build: translated (code rebased, a
/// data address translated), and where `new` keeps a byte of `expected`, the byte `found` there, when what the site
/// holds [`matches`] `expected`.
pub(crate) fn written(site: Site, expected: &[u8], found: Option<&[u8]>, new: &[u8]) -> Vec<u8> {
    written_with(site.ru(), expected, found, new, here)
}

#[cfg(test)]
mod tests {
    use super::*;

    use eix_sites::{DATA_7AF784, DATA_SECTION, ENTRY, RDATA_SECTION, RSRC_SECTION, TEXT_SECTION};

    /// Low code, which a test build leaves in place, and code from its high part on, which it moves down by 0x40.
    const LOW: usize = TEXT_SECTION.ru() + 0x5_5440;
    const HIGH: usize = TEXT_SECTION.ru() + 0x1F_F000;
    /// Data from here on moves down by 0x10.
    const MOVED_DATA: usize = DATA_SECTION.ru() + 0xD000;

    fn map(a: usize) -> usize {
        if (HIGH..RDATA_SECTION.ru()).contains(&a) {
            a - 0x40
        } else if (MOVED_DATA..RSRC_SECTION.ru()).contains(&a) {
            a - 0x10
        } else {
            a
        }
    }

    #[test]
    fn rebases_branches_and_addresses() {
        // call LOW from high code; mov eax, [DATA_7AF784]; jne short
        let site = ENTRY.ru() + 0xD6;
        let rel = (LOW as u32).wrapping_sub(site as u32 + 5).to_le_bytes();
        let data = DATA_7AF784.ru32().to_le_bytes();
        let ru = [
            0xE8, rel[0], rel[1], rel[2], rel[3], 0xA1, data[0], data[1], data[2], data[3], 0x75,
            0x02,
        ];
        let en = rebase(site, &ru, map).unwrap();
        let rel_en = (LOW as u32)
            .wrapping_sub(map(site) as u32 + 5)
            .to_le_bytes();
        assert_eq!(&en[1..5], &rel_en);
        assert_eq!(en[6..10], (map(DATA_7AF784.ru()) as u32).to_le_bytes());
        assert_eq!(&en[10..], &[0x75, 0x02]);
        assert!(equivalent_with(site, &ru, &en, map));
        // Part of the instruction (the address operand alone) is equivalent too.
        assert!(equivalent_with(site + 6, &ru[6..10], &en[6..10], map));
        // A field that is not the translation is not.
        let mut wrong = en.clone();
        wrong[7] ^= 1;
        assert!(!equivalent_with(site, &ru, &wrong, map));
    }

    #[test]
    fn a_jump_into_a_stub_keeps_its_target() {
        let site = HIGH + 0x1000;
        let stub = 0x1000_0000u32;
        let ru = [
            &[0xE9][..],
            &stub.wrapping_sub(site as u32 + 5).to_le_bytes(),
        ]
        .concat();
        let en = rebase(site, &ru, map).unwrap();
        assert_eq!(
            en[1..5],
            stub.wrapping_sub(map(site) as u32 + 5).to_le_bytes()
        );
        assert!(equivalent_with(site, &ru, &en, map));
    }

    #[test]
    fn written_bytes_take_the_found_ones_they_keep() {
        // mov ecx, [DATA_7AF784] (8B 0D) whose opcode a patch keeps and whose operand the build moved.
        let site = LOW;
        let [r0, r1, r2, r3] = DATA_7AF784.ru32().to_le_bytes();
        let [e0, e1, e2, e3] = (map(DATA_7AF784.ru()) as u32).to_le_bytes();
        let expected = [0x8B, 0x0D, r0, r1, r2, r3];
        let found = [0x8B, 0x0D, e0, e1, e2, e3];
        let new = [0x8B, 0x15, r0, r1, r2, r3];
        assert!(equivalent_with(site, &expected, &found, map));
        assert_eq!(
            written_with(site, &expected, Some(&found), &new, map),
            [0x8B, 0x15, e0, e1, e2, e3]
        );
        // The operand's low byte alone needs the bytes around it.
        assert!(!equivalent_with(site + 2, &[r0], &[e0], map));
        // A data pointer is translated as a value.
        assert_eq!(
            translated_with(MOVED_DATA + 0x1000, &DATA_7AF784.ru32().to_le_bytes(), map),
            (map(DATA_7AF784.ru()) as u32).to_le_bytes()
        );
    }
}
