use eix_engine::patch;
use eix_sites::{Build, Site};

#[test]
fn camp_click_calls_keep_their_targets_in_en106() {
    // This binary selects one build for its whole process.
    eix_sites::select(Build::En106);
    let writes = spelladdon::writes();
    let start = eix_sites::ACTION_COLUMN_MOUSE_UP_VERIFY__43;
    let mut ru = [0; 29];
    let mut en = [0; 29];
    let mut covered = [false; 29];
    for (name, write) in writes {
        if name != "[gameplay] camp=2 (additional sites)"
            || !(start.ru()..start.ru() + ru.len()).contains(&write.site().ru())
        {
            continue;
        }
        let offset = write.site().since(start);
        let end = offset + write.size();
        ru[offset..end].copy_from_slice(write.new_bytes());
        en[offset..end].copy_from_slice(&patch::as_built(
            write.site(),
            write.kind(),
            write.new_bytes(),
        ));
        covered[offset..end].fill(true);
    }
    assert!(covered.into_iter().all(|byte| byte));
    for call in [6, 19] {
        assert_eq!(ru[call], 0xE8);
        assert_eq!(en[call], 0xE8);
        let field = call + 1..call + 5;
        let target = |address: usize, code: &[u8]| {
            (address as u32 + call as u32 + 5)
                .wrapping_add(u32::from_le_bytes(code[field.clone()].try_into().unwrap()))
                as usize
        };
        let wanted = Site::new(target(start.ru(), &ru)).in_build(Build::En106);
        assert_eq!(
            target(start.in_build(Build::En106), &en),
            wanted,
            "call at +{call}"
        );
    }
}
