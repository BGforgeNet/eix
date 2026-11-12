//! Another build than RU 1.06: the code names RU 1.06 addresses, in constants and inside the bytes it expects and
//! writes; these turn such bytes into the running build's. Code bytes are decoded, and each branch target and 32-bit
//! address field is translated with [`eix_sites::here`]; a data site's 4-byte value is translated when it is an
//! address in the image.
#![expect(unsafe_code, reason = "reading the bytes a site holds")]

use alloc::vec::Vec;

use eix_sites::{Site, in_image};

use crate::x86;

/// What bytes written at a site are, for another build: the bytes there translate as their kind says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// What the site's section holds: code in `.text`, data elsewhere (a data site's 4-byte address translates).
    Section,
    /// Code wherever it lies: a branch the original put into `.data` is rebased as code.
    Code,
    /// Data wherever it lies: a pointer in a jump table of `.text` translates as an address.
    Data,
    /// Bytes that hold no address (text, a constant), the same in every build: nothing in them translates.
    Plain,
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    let field: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(field))
}

fn put(bytes: &mut [u8], at: usize, value: u32) {
    if let Some(field) = bytes.get_mut(at..at + 4) {
        field.copy_from_slice(&value.to_le_bytes());
    }
}

/// How bytes at a site translate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// Rebased: branch targets and address fields.
    Code,
    /// A 4-byte address.
    Data,
    /// Not at all.
    Plain,
}

/// How bytes of `kind` at `site` translate: as declared, or code in game.exe's `.text` and data elsewhere.
fn shape(site: usize, kind: Kind) -> Shape {
    match kind {
        Kind::Code => Shape::Code,
        Kind::Data => Shape::Data,
        Kind::Plain => Shape::Plain,
        Kind::Section => {
            if (eix_sites::TEXT_SECTION.ru()..eix_sites::RDATA_SECTION.ru()).contains(&site) {
                Shape::Code
            } else {
                Shape::Data
            }
        }
    }
}

/// Whether bytes of `kind` at `site` are code: declared so, or by game.exe's section.
pub(crate) fn is_code(site: usize, kind: Kind) -> bool {
    shape(site, kind) == Shape::Code
}

/// RU 1.06 address `ru` in the running build.
fn here(ru: usize) -> usize {
    Site::new(ru).here()
}

/// `bytes`, code written for RU address `site`, moved to `map(site)`, as far as they decode as whole instructions;
/// the rest is left as it is.
fn rebase(site: usize, bytes: &[u8], map: impl Fn(usize) -> usize) -> Vec<u8> {
    let to = map(site);
    let mut out = bytes.to_vec();
    for (at, insn) in x86::whole_insns(bytes) {
        let end = at + insn.len;
        if let Some(r) = insn.rel32
            && let Some(rel) = word(bytes, at + r)
        {
            let target = ((site + end) as u32).wrapping_add(rel) as usize;
            put(
                &mut out,
                at + r,
                (map(target) as u32).wrapping_sub((to + end) as u32),
            );
        }
        for field in [insn.disp32, insn.imm32].into_iter().flatten() {
            match word(bytes, at + field) {
                Some(value) if in_image(value as usize) => {
                    put(&mut out, at + field, map(value as usize) as u32);
                }
                _ => {}
            }
        }
    }
    out
}

/// Where `bytes` of `shape` hold 4-byte fields of their own (a branch displacement, an address operand or immediate,
/// a data pointer), which their translation sets; code only as far as [`rebase`] decodes it.
fn own_fields(shape: Shape, bytes: &[u8]) -> Vec<usize> {
    match shape {
        Shape::Code => x86::whole_insns(bytes)
            .flat_map(|(at, insn)| {
                [insn.rel32, insn.disp32, insn.imm32]
                    .into_iter()
                    .flatten()
                    .map(move |f| at + f)
            })
            .collect(),
        Shape::Data if bytes.len() == 4 => Vec::from([0]),
        Shape::Data | Shape::Plain => Vec::new(),
    }
}

fn translated_with(
    site: usize,
    bytes: &[u8],
    shape: Shape,
    map: impl Fn(usize) -> usize,
) -> Vec<u8> {
    match (shape, word(bytes, 0)) {
        (Shape::Code, _) => rebase(site, bytes, map),
        (Shape::Data, Some(value)) if bytes.len() == 4 && in_image(value as usize) => {
            (map(value as usize) as u32).to_le_bytes().to_vec()
        }
        _ => bytes.to_vec(),
    }
}

/// `bytes` of `kind` written for RU address `site`, as the running build would hold them: code rebased, a data
/// address translated; `bytes` unchanged where neither applies.
pub(crate) fn translated(site: Site, bytes: &[u8], kind: Kind) -> Vec<u8> {
    translated_with(site.ru(), bytes, shape(site.ru(), kind), here)
}

/// The 4-byte fields where `found` differs from `bytes` (written for RU address `site`) as their translation, by
/// offset, and whether every differing byte lies in one.
fn translated_fields(
    site: usize,
    bytes: &[u8],
    found: &[u8],
    shape: Shape,
    map: impl Fn(usize) -> usize,
) -> (Vec<usize>, bool) {
    let moved = translated_with(site, bytes, shape, map);
    let mut localized = bytes.to_vec();
    let mut fields = Vec::new();
    for field in own_fields(shape, bytes) {
        let range = field..field + 4;
        if found.get(range.clone()) == moved.get(range.clone()) {
            localized[range.clone()].copy_from_slice(&moved[range]);
            fields.push(field);
        }
    }
    (fields, localized == found)
}

fn equivalent_with(
    site: usize,
    bytes: &[u8],
    found: &[u8],
    shape: Shape,
    map: impl Fn(usize) -> usize,
) -> bool {
    translated_fields(site, bytes, found, shape, map).1
}

fn localized_with(
    site: usize,
    bytes: &[u8],
    found: &[u8],
    shape: Shape,
    map: impl Fn(usize) -> usize,
) -> Vec<u8> {
    let mut out = bytes.to_vec();
    if bytes.len() == found.len() {
        for w in translated_fields(site, bytes, found, shape, map).0 {
            out[w..w + 4].copy_from_slice(&found[w..w + 4]);
        }
    }
    out
}

/// `bytes` (expected at RU address `site`) with each 4-byte field that `found` holds translated taken from `found`:
/// what the running build holds where only part of the bytes match (an earlier write covers the rest), so the rest
/// is left for the caller to judge.
pub(crate) fn localized(site: Site, bytes: &[u8], found: &[u8], kind: Kind) -> Vec<u8> {
    localized_with(site.ru(), bytes, found, shape(site.ru(), kind), here)
}

/// The maximum x86 instruction length: enough context to decode a field a site cuts.
const REACH: usize = 15;

/// Whether `found` (read at `here(site)`) is `bytes` (written for RU address `site`) translated: equal but for 4-byte
/// fields the decoder identifies as addresses or branch displacements, or a declared data pointer. A site that cuts
/// an instruction is judged with the bytes around it, which it does not cover and so must be the same in both.
///
/// # Safety
/// `here(site)`, with [`REACH`] bytes on either side, must be mapped, readable memory that no other thread writes
/// meanwhile, when `site` lies in game.exe's image.
pub(crate) unsafe fn matches(site: Site, bytes: &[u8], found: &[u8], kind: Kind) -> bool {
    let site = site.ru();
    let shape = shape(site, kind);
    if found == bytes || equivalent_with(site, bytes, found, shape, here) {
        return true;
    }
    if shape != Shape::Code || !in_image(site) || bytes.len() != found.len() {
        return false;
    }
    // Whole instructions establish their own boundaries; surrounding bytes cannot turn their opcodes into operands.
    if x86::whole_insns(bytes)
        .map(|(_, insn)| insn.len)
        .sum::<usize>()
        == bytes.len()
    {
        return false;
    }
    let at = here(site);
    // SAFETY: the caller's guarantee.
    let read = |from: usize| unsafe { core::slice::from_raw_parts(from as *const u8, REACH) };
    let (before, after) = (read(at - REACH), read(at + bytes.len()));
    equivalent_around(site, bytes, found, before, after, here)
}

fn equivalent_around(
    site: usize,
    bytes: &[u8],
    found: &[u8],
    before: &[u8],
    after: &[u8],
    map: impl Fn(usize) -> usize,
) -> bool {
    let reach = before.len();
    let ru = [before, bytes, after].concat();
    let now = [before, found, after].concat();
    // Start at the site for an instruction cut at its end, or at a preceding instruction whose field crosses it.
    (0..=reach).rev().any(|start| {
        let Ok(insn) = x86::decode_in_place(&ru[start..], 0) else {
            return false;
        };
        let crosses = [insn.rel32, insn.disp32, insn.imm32]
            .into_iter()
            .flatten()
            .any(|f| start + f <= reach && start + f + 4 > reach);
        (start == reach || crosses)
            && equivalent_with(
                site - reach + start,
                &ru[start..],
                &now[start..],
                Shape::Code,
                &map,
            )
    })
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
    if unsafe { matches(site, bytes, found, Kind::Code) } {
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
    shape: Shape,
    map: impl Fn(usize) -> usize,
) -> Vec<u8> {
    let mut out = translated_with(site, new, shape, map);
    if let Some(found) = found.filter(|f| f.len() == new.len() && expected.len() == new.len()) {
        // A field `new` holds itself is its own, translated above, even where a byte of it equals `expected`'s.
        let own = own_fields(shape, new);
        let owned = |i: usize| own.iter().any(|&f| (f..f + 4).contains(&i));
        for (i, b) in out.iter_mut().enumerate() {
            if new[i] == expected[i] && !owned(i) {
                *b = found[i];
            }
        }
    }
    out
}

/// `new`, bytes of `kind` to be written over `expected` at RU address `site`, for the running build: translated (code
/// rebased, a data address translated), and where `new` keeps a byte of `expected` outside a field of its own, the
/// byte `found` there, when what the site holds [`matches`] `expected`.
pub(crate) fn written(
    site: Site,
    expected: &[u8],
    found: Option<&[u8]>,
    new: &[u8],
    kind: Kind,
) -> Vec<u8> {
    written_with(
        site.ru(),
        expected,
        found,
        new,
        shape(site.ru(), kind),
        here,
    )
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
    fn nop_bytes_are_not_a_relative_displacement() {
        let site = eix_sites::TERRAIN_GROUND_AND_WATER_DEPTH__3E9.ru();
        let en = |at| Site::new(at).in_build(eix_sites::Build::En106);
        let expected = [0x90; 7];
        let found = [0xD0, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90];
        for kind in [Kind::Section, Kind::Code, Kind::Data, Kind::Plain] {
            let shape = shape(site, kind);
            assert!(!equivalent_with(site, &expected, &found, shape, en));
            assert_eq!(localized_with(site, &expected, &found, shape, en), expected);
        }
    }

    #[test]
    fn a_cut_address_field_uses_its_instruction() {
        let site = LOW;
        let address = DATA_7AF784.ru32().to_le_bytes();
        let moved = (map(DATA_7AF784.ru()) as u32).to_le_bytes();
        let before = [0x8B, 0x0D];
        assert!(equivalent_around(
            site + 2,
            &address,
            &moved,
            &before,
            &[],
            map
        ));
        assert!(equivalent_around(
            site + 2,
            &address[..1],
            &moved[..1],
            &before,
            &address[1..],
            map
        ));
        // A push with an 8-bit immediate does not give the following bytes an address field.
        assert!(!equivalent_around(
            site + 2,
            &address,
            &moved,
            &[0x90, 0x6A],
            &[],
            map
        ));
    }

    #[test]
    fn an_absolute_operand_cannot_match_a_relative_translation() {
        let site = HIGH + 0x1000;
        let expected = [0xB8, 0x90, 0x90, 0x90, 0x90];
        let found = [0xB8, 0xD0, 0x90, 0x90, 0x90];
        assert!(!equivalent_with(site, &expected, &found, Shape::Code, map));
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
        let en = rebase(site, &ru, map);
        let rel_en = (LOW as u32)
            .wrapping_sub(map(site) as u32 + 5)
            .to_le_bytes();
        assert_eq!(&en[1..5], &rel_en);
        assert_eq!(en[6..10], (map(DATA_7AF784.ru()) as u32).to_le_bytes());
        assert_eq!(&en[10..], &[0x75, 0x02]);
        assert!(equivalent_with(site, &ru, &en, Shape::Code, map));
        // A declared address operand is translated as data.
        assert!(equivalent_with(
            site + 6,
            &ru[6..10],
            &en[6..10],
            Shape::Data,
            map
        ));
        assert!(!equivalent_with(site, &ru, &en, Shape::Plain, map));
        assert!(!equivalent_with(
            site + 6,
            &ru[6..10],
            &en[6..10],
            Shape::Plain,
            map
        ));
        assert_eq!(localized_with(site, &ru, &en, Shape::Plain, map), ru);
        // A field that is not the translation is not.
        let mut wrong = en.clone();
        wrong[7] ^= 1;
        assert!(!equivalent_with(site, &ru, &wrong, Shape::Code, map));
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
        let en = rebase(site, &ru, map);
        assert_eq!(
            en[1..5],
            stub.wrapping_sub(map(site) as u32 + 5).to_le_bytes()
        );
        assert!(equivalent_with(site, &ru, &en, Shape::Code, map));
    }

    #[test]
    fn code_is_rebased_as_far_as_it_decodes() {
        // jmp LOW; fld [DATA_7AF784]; then the start of a `fsub [esp + 8]` the bytes after the write complete.
        let site = HIGH + 0x1000;
        let rel = (LOW as u32).wrapping_sub(site as u32 + 5).to_le_bytes();
        let data = DATA_7AF784.ru32().to_le_bytes();
        let ru = [&[0xE9][..], &rel, &[0xD9, 0x05], &data, &[0xD8, 0x64]].concat();
        let en = rebase(site, &ru, map);
        assert_eq!(
            en[1..5],
            (LOW as u32)
                .wrapping_sub(map(site) as u32 + 5)
                .to_le_bytes()
        );
        assert_eq!(en[7..11], (map(DATA_7AF784.ru()) as u32).to_le_bytes());
        assert_eq!(en[11..], [0xD8, 0x64]);
        assert_eq!(own_fields(Shape::Code, &ru), [1, 7]);
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
        assert!(equivalent_with(site, &expected, &found, Shape::Code, map));
        assert_eq!(
            written_with(site, &expected, Some(&found), &new, Shape::Code, map),
            [0x8B, 0x15, e0, e1, e2, e3]
        );
        // The operand's low byte alone needs the bytes around it.
        assert!(!equivalent_with(site + 2, &[r0], &[e0], Shape::Code, map));
        // A data pointer is translated as a value.
        assert_eq!(
            translated_with(
                MOVED_DATA + 0x1000,
                &DATA_7AF784.ru32().to_le_bytes(),
                Shape::Data,
                map
            ),
            (map(DATA_7AF784.ru()) as u32).to_le_bytes()
        );
        // Plain bytes, though they read as an address, are not.
        assert_eq!(
            translated_with(MOVED_DATA, b"Gof\0", Shape::Plain, map),
            *b"Gof\0"
        );
    }

    #[test]
    fn bytes_matching_in_part_localize_what_translates() {
        // A call into moved high code, then two bytes an earlier write changed: the call is the build's, the rest
        // stays RU's for the caller to judge.
        let site = LOW;
        let rel = (HIGH as u32 + 0x93).wrapping_sub(site as u32 + 5);
        let [r0, r1, r2, r3] = rel.to_le_bytes();
        let [e0, e1, e2, e3] = rel.wrapping_sub(0x40).to_le_bytes();
        let expected = [0xE8, r0, r1, r2, r3, 0x33, 0xC0];
        let found = [0xE8, e0, e1, e2, e3, 0xEB, 0x10];
        assert!(!equivalent_with(site, &expected, &found, Shape::Code, map));
        assert_eq!(
            localized_with(site, &expected, &found, Shape::Code, map),
            [0xE8, e0, e1, e2, e3, 0x33, 0xC0]
        );
    }

    #[test]
    fn a_pointer_written_into_code_is_data() {
        // A jump table's entry in `.text`, pointing at high code the build moved.
        let site = LOW;
        assert!(is_code(site, Kind::Section));
        assert!(!is_code(site, Kind::Data));
        let entry = (HIGH as u32 + 0x10).to_le_bytes();
        let moved = (map(HIGH + 0x10) as u32).to_le_bytes();
        assert_eq!(
            written_with(site, &entry, Some(&moved), &entry, Shape::Data, map),
            moved
        );
    }

    #[test]
    fn a_jump_written_into_data_is_code() {
        // The original's jump to added code, put in `.data`, which the build moved: declared code, it is rebased.
        let site = MOVED_DATA + 0x1000;
        assert!(!is_code(site, Kind::Section));
        assert!(is_code(site, Kind::Code));
        let stub = 0x1000_0000u32;
        let jump = |from: usize| {
            let [r0, r1, r2, r3] = stub.wrapping_sub(from as u32 + 5).to_le_bytes();
            [0xE9, r0, r1, r2, r3]
        };
        let written = written_with(site, &[0; 5], Some(&[0; 5]), &jump(site), Shape::Code, map);
        assert_eq!(written, jump(map(site)));
    }

    #[test]
    fn a_written_field_keeps_its_own_bytes_where_they_match_the_expected_ones() {
        // The game's `call` into high code, which the build moved down, becomes a call to a stub whose displacement
        // shares its low three bytes with the game's.
        let site = LOW;
        let call = |rel: u32| {
            let [r0, r1, r2, r3] = rel.to_le_bytes();
            [0xE8, r0, r1, r2, r3]
        };
        let rel = (HIGH as u32 + 0x93).wrapping_sub(site as u32 + 5);
        let expected = call(rel);
        let found = call(rel.wrapping_sub(0x40));
        let new = call(rel.wrapping_add(0x1000_0000));
        assert!(equivalent_with(site, &expected, &found, Shape::Code, map));
        assert_eq!(
            written_with(site, &expected, Some(&found), &new, Shape::Code, map),
            new
        );
        // A data pointer that shares bytes with the one it replaces is written whole.
        let slot = MOVED_DATA + 0x1000;
        let was = (HIGH as u32 + 0x10).to_le_bytes();
        let moved = (map(HIGH + 0x10) as u32).to_le_bytes();
        let ours = (HIGH as u32 + 0x10 + 0x1000_0000).to_le_bytes();
        assert_eq!(
            written_with(slot, &was, Some(&moved), &ours, Shape::Data, map),
            ours
        );
    }
}
