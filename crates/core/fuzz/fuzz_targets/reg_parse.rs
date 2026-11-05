#![no_main]

use eix_core::reg::Registry;
use libfuzzer_sys::fuzz_target;

// Any input parses or errors without a panic; what parses writes and parses back to the same registry.
fuzz_target!(|data: &[u8]| {
    if let Ok(reg) = Registry::parse(data) {
        if let Ok(bytes) = reg.to_bytes() {
            let again = Registry::parse(&bytes).expect("written files parse");
            assert_eq!(again.to_bytes().ok().as_deref(), Some(bytes.as_slice()));
        }
    }
});
