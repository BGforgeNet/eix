//! HeapPadding: how much larger the engine fix makes each of game.exe's heap blocks (see
//! `docs/engine.md`).

/// The padding of every block.
pub const MIN_PADDING: usize = 64;
/// Blocks of this size and more get a quarter of their size as padding, if that is more.
pub const LARGE_BLOCK: usize = 1024;
/// The most padding a block gets.
pub const MAX_PADDING: usize = 512;

/// The bytes added after a block of `size` bytes: 64, or for a block of 1 KB and more a quarter of its size up to
/// 512. The game's figure objects overrun their 656-byte blocks by 12 bytes; pointer arrays of 1 KB and more are
/// reported to overrun by 130 and more, which a flat 64 would leave.
pub fn padding(size: usize) -> usize {
    if size >= LARGE_BLOCK {
        (size / 4).clamp(MIN_PADDING, MAX_PADDING)
    } else {
        MIN_PADDING
    }
}

/// The size asked of the heap for a block of `size` bytes: `size` plus its padding, or `size` itself where the sum
/// does not fit (no heap has that much to give, so the request fails as the game's own would).
pub fn padded(size: usize) -> usize {
    size.checked_add(padding(size)).unwrap_or(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_blocks_get_64_bytes() {
        assert_eq!(padding(0), 64);
        assert_eq!(padding(16), 64);
        assert_eq!(padding(1023), 64);
        assert_eq!(padded(0), 64);
        assert_eq!(padded(1008), 1072);
    }

    #[test]
    fn blocks_of_1_kb_and_more_get_a_quarter_up_to_512() {
        assert_eq!(padding(1024), 256);
        assert_eq!(padding(1027), 256);
        assert_eq!(padding(1028), 257);
        assert_eq!(padding(2047), 511);
        assert_eq!(padding(2048), 512);
        assert_eq!(padding(1 << 20), 512);
        assert_eq!(padded(1024), 1280);
        assert_eq!(padded(4096), 4608);
    }

    #[test]
    fn the_measured_overruns_stay_inside_the_padding() {
        // A figure object: 0x284 bytes rounded up to 16 by the C runtime, a pointer written at +0x298.
        let figure = 0x284usize.next_multiple_of(16);
        assert_eq!(figure, 656);
        assert!(0x298 + 4 <= padded(figure));
        // Pointer arrays of 1 KB and more can overrun by 130 bytes or more.
        for size in (1024..=8192).step_by(16) {
            assert!(padding(size) >= 160, "{size}");
        }
    }

    #[test]
    fn a_larger_block_asks_for_more() {
        let mut last = 0;
        for size in 0..=4096 {
            assert!(padded(size) > last, "{size}");
            last = padded(size);
        }
    }

    #[test]
    fn a_size_that_cannot_be_padded_is_passed_as_it_is() {
        assert_eq!(padded(usize::MAX), usize::MAX);
        assert_eq!(padded(usize::MAX - 511), usize::MAX - 511);
        assert_eq!(padded(usize::MAX - 512), usize::MAX);
    }
}
