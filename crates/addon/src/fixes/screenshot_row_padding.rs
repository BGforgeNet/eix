//! ScreenshotRowPadding: `CDib::AllocateBits` 0x6D6AB0 allocates the image size `CDib::Create` 0x6D4BA0 computed
//! (`[esi+0x20]`, rows rounded up to 4 bytes), which `CDib::Write` 0x6D5AC0 writes to the file. It allocated
//! bytes per pixel x planes x width x height, so a capture whose rows are not a multiple of 4 bytes (24 bits at
//! 1366 pixels) had the writer read past the block.

use eix_sites::Site;

use crate::groups::{BuildError, Labelled, bytes};
use crate::setup::Setup;

/// The size computation before `push eax; call operator new` (0x6D6AD4).
const SIZE: Site = eix_sites::C_DIB_ALLOCATE_BITS__A;
/// `xor eax, eax; xor edx, edx; mov ax, [ecx+0xE]; mov dx, [ecx+0xC]; shr eax, 3; imul eax, edx;
/// imul eax, [ecx+8]; imul eax, [ecx+4]`.
const SIZE_BYTES: [u8; 26] = [
    0x33, 0xC0, 0x33, 0xD2, 0x66, 0x8B, 0x41, 0x0E, 0x66, 0x8B, 0x51, 0x0C, 0xC1, 0xE8, 0x03, 0x0F,
    0xAF, 0xC2, 0x0F, 0xAF, 0x41, 0x08, 0x0F, 0xAF, 0x41, 0x04,
];
/// `mov eax, [esi+0x20]`, then NOPs: the size `CDib::Write` writes (0x6D5B3A).
const PADDED_SIZE: [u8; 3] = [0x8B, 0x46, 0x20];

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let mut new = [0x90; SIZE_BYTES.len()];
    new[..PADDED_SIZE.len()].copy_from_slice(&PADDED_SIZE);
    Ok(vec![bytes(
        "ScreenshotRowPadding #0",
        SIZE,
        &SIZE_BYTES,
        &new,
    )?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_size_is_loaded_and_the_rest_is_nops() {
        let w = build(&Setup::default()).unwrap();
        assert_eq!(
            w[0].write.site().offset(w[0].write.size()),
            eix_sites::C_DIB_ALLOCATE_BITS__24
        );
        assert_eq!(&w[0].write.new_bytes()[..3], &PADDED_SIZE);
        assert!(w[0].write.new_bytes()[3..].iter().all(|&b| b == 0x90));
    }
}
