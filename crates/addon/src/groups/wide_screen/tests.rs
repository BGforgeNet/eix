use super::*;

/// The 1056x768 mode.
pub(super) fn layout_1056x768() -> Layout {
    match Mode::from_resolution(Some("1056x768")) {
        Mode::Wide(l) => l,
        other @ (Mode::NotWide(_) | Mode::Invalid(_)) => panic!("{other:?}"),
    }
}

#[test]
fn the_1056x768_mode_moves_the_panels_by_13() {
    assert_eq!(layout_1056x768().extra, 13);
}

/// The WideScreen sites of the 1056x768 mode: number, address, length, and the bytes
/// written where they do not depend on where our code lies.
const SITES: [(usize, Site, usize, Option<&[u8]>); 100] = [
    (0, eix_sites::C_UI_REGION_SET_RECT, 5, None),
    (1, eix_sites::C_UI_MESH_SET_TRIANGLE__4A7, 5, None),
    (2, eix_sites::C_UI_MESH_SET_GRID_VANILLA__60D, 5, None),
    (3, eix_sites::C_UI_MESH_SET_RECTANGLE__4F1, 5, None),
    (4, eix_sites::C_UI_MESH_SET_FRAME__5FE, 5, None),
    (5, eix_sites::C_UI_REGION_ADD_POINT, 5, None),
    (6, eix_sites::C_UI_MESH_PLACE_BODY, 5, None),
    (7, eix_sites::C_INTERFACE_3D_SCALE_RECT_TO_SCREEN, 5, None),
    (8, eix_sites::C_INTERFACE_3D_SCALE_POINT_TO_SCREEN, 5, None),
    (
        9,
        eix_sites::C_INTERFACE_3D_SCALE_POINT_TO_SCREEN__40,
        5,
        None,
    ),
    (
        10,
        eix_sites::C_UI_EDIT_FIELD_DRAW_CONTENTS_VANILLA__23F,
        6,
        Some(&[0x90, 0x90, 0x90, 0x90, 0x31, 0xC9]),
    ),
    (
        11,
        eix_sites::C_UI_EDIT_FIELD_DRAW_CONTENTS_VANILLA__264,
        6,
        Some(&[0x90, 0x90, 0x90, 0x90, 0x31, 0xC9]),
    ),
    (
        12,
        eix_sites::C_MP_CONNECT_SCREEN_DRAW_LIST_VANILLA__38E,
        6,
        Some(&[0x90, 0x90, 0x90, 0x90, 0x31, 0xC9]),
    ),
    (
        13,
        eix_sites::C_MP_CONNECT_SCREEN_DRAW_LIST_VANILLA__3BE,
        6,
        Some(&[0x90, 0x90, 0x90, 0x90, 0x31, 0xC9]),
    ),
    (
        14,
        eix_sites::C_SAVE_LOAD_WINDOW_REFRESH_LIST_VANILLA__257,
        6,
        Some(&[0x90, 0x90, 0x90, 0x90, 0x31, 0xC9]),
    ),
    (
        15,
        eix_sites::C_SAVE_LOAD_WINDOW_REFRESH_LIST_VANILLA__268,
        6,
        Some(&[0x90, 0x90, 0x90, 0x90, 0x31, 0xC9]),
    ),
    (
        16,
        eix_sites::C_CHAT_OVERLAY_DRAW_TYPED__139,
        6,
        Some(&[0x90, 0x90, 0x90, 0x90, 0x31, 0xC9]),
    ),
    (
        17,
        eix_sites::C_CHAT_OVERLAY_DRAW_TYPED__16C,
        6,
        Some(&[0x90, 0x90, 0x90, 0x90, 0x31, 0xC9]),
    ),
    (
        18,
        eix_sites::C_UI_TOOLTIP_ON_MOUSE_MOVE__1FE,
        6,
        Some(&[0x90, 0x90, 0x90, 0x90, 0x31, 0xC9]),
    ),
    (
        19,
        eix_sites::C_UI_TOOLTIP_ON_MOUSE_MOVE__243,
        4,
        Some(&[0x2D, 0x03, 0x00, 0x00]),
    ),
    (
        20,
        eix_sites::C_UI_TOOLTIP_ON_MOUSE_MOVE__24A,
        4,
        Some(&[0xD3, 0xFC, 0xFF, 0xFF]),
    ),
    (21, eix_sites::C_HUD_MAP_SETUP_VANILLA, 5, None),
    (22, eix_sites::C_HUD_MAP_SETUP_VANILLA__9FF, 5, None),
    (23, eix_sites::C_HUD_MAP_LAYOUT, 5, None),
    (24, eix_sites::C_HUD_MAP_LAYOUT__1D9, 5, None),
    (
        25,
        eix_sites::C_HUD_MAP_SETUP_VANILLA__31,
        4,
        Some(&[0x74, 0x02, 0x00, 0x00]),
    ),
    (
        26,
        eix_sites::C_HUD_MAP_SETUP_VANILLA__41,
        4,
        Some(&[0x2D, 0x03, 0x00, 0x00]),
    ),
    (
        27,
        eix_sites::C_HUD_MAP_LAYOUT__1CC,
        4,
        Some(&[0xC9, 0x02, 0x00, 0x00]),
    ),
    (
        28,
        eix_sites::C_HUD_MAP_UPDATE_VANILLA__1E3,
        4,
        Some(&[0x00, 0x80, 0x35, 0x44]),
    ),
    (
        29,
        eix_sites::C_HUD_MAP_UPDATE_VANILLA__23F,
        4,
        Some(&[0x00, 0xC0, 0x57, 0x44]),
    ),
    (
        30,
        eix_sites::MAP_MOUSE_UP_VERIFY__DD,
        4,
        Some(&[0x37, 0xFD, 0xFF, 0xFF]),
    ),
    (
        31,
        eix_sites::MAP_MOUSE_UP_VERIFY__222,
        4,
        Some(&[0x37, 0xFD, 0xFF, 0xFF]),
    ),
    (
        32,
        eix_sites::C_HUD_UNIT_PANEL_DRAW_VANILLA__1AC,
        6,
        Some(&[0x90, 0x90, 0x90, 0x90, 0x31, 0xC9]),
    ),
    (
        33,
        eix_sites::C_HUD_UNIT_PANEL_DRAW_VANILLA__1EE,
        6,
        Some(&[0x90, 0x90, 0x90, 0x90, 0x31, 0xC9]),
    ),
    (
        34,
        eix_sites::C_HUD_UNIT_PANEL_DRAW_VANILLA__1F9,
        6,
        Some(&[0x90, 0x90, 0x90, 0x90, 0x31, 0xC9]),
    ),
    (35, eix_sites::C_HUD_UNIT_PANEL_SETUP__39, 5, None),
    (36, eix_sites::C_HUD_UNIT_PANEL_SETUP__896, 5, None),
    (37, eix_sites::C_HUD_UNIT_PANEL_LAYOUT, 5, None),
    (38, eix_sites::C_HUD_UNIT_PANEL_LAYOUT__319, 5, None),
    (39, eix_sites::C_HUD_UNIT_PANEL_DRAW_VANILLA, 5, None),
    (40, eix_sites::C_HUD_UNIT_PANEL_DRAW_VANILLA__87D, 5, None),
    (
        41,
        eix_sites::C_HUD_UNIT_PANEL_DRAW_VANILLA__368,
        4,
        Some(&[0x57, 0x00, 0x00, 0x00]),
    ),
    (
        42,
        eix_sites::C_HUD_UNIT_PANEL_UPDATE_VANILLA__13B,
        4,
        Some(&[0x00, 0x00, 0x7C, 0xC2]),
    ),
    (
        43,
        eix_sites::C_HUD_UNIT_PANEL_UPDATE_VANILLA__197,
        4,
        Some(&[0x00, 0x00, 0x94, 0x42]),
    ),
    (
        44,
        eix_sites::C_HUD_UNIT_PANEL_LAYOUT__2D1,
        4,
        Some(&[0x07, 0x00, 0x00, 0x00]),
    ),
    (
        45,
        eix_sites::C_HUD_QUICK_ITEMS_CONSTRUCT__80,
        4,
        Some(&[0x15, 0x02, 0x00, 0x00]),
    ),
    (
        46,
        eix_sites::C_HUD_QUICK_ITEMS_CONSTRUCT__94,
        4,
        Some(&[0xDD, 0x02, 0x00, 0x00]),
    ),
    (47, eix_sites::C_HUD_QUICK_ITEMS_CONSTRUCT, 5, None),
    (48, eix_sites::C_HUD_QUICK_ITEMS_CONSTRUCT__22F, 5, None),
    (
        49,
        eix_sites::G_PANEL_RIGHT,
        4,
        Some(&[0x00, 0x00, 0x31, 0x44]),
    ),
    (
        50,
        eix_sites::C_HUD_QUICK_ITEMS_ADD_ITEM__133,
        4,
        Some(&[0x00, 0x40, 0x2D, 0x44]),
    ),
    (
        51,
        eix_sites::C_HUD_QUICK_ITEMS_ADD_ITEM__127,
        4,
        Some(&[0x00, 0x40, 0x37, 0x44]),
    ),
    (52, eix_sites::C_HUD_ACTION_COLUMN_CONSTRUCT__78, 5, None),
    (53, eix_sites::C_HUD_ACTION_COLUMN_CONSTRUCT__152, 5, None),
    (
        54,
        eix_sites::C_HUD_ACTION_COLUMN_CONSTRUCT__8E,
        4,
        Some(&[0x1B, 0x00, 0x00, 0x00]),
    ),
    (
        55,
        eix_sites::C_HUD_ACTION_COLUMN_ADD_ACTION__133,
        4,
        Some(&[0x00, 0x00, 0xE0, 0x40]),
    ),
    (
        56,
        eix_sites::C_HUD_SPELL_COLUMN_CONSTRUCT__66,
        4,
        Some(&[0x05, 0x03, 0x00, 0x00]),
    ),
    (
        57,
        eix_sites::C_HUD_SPELL_COLUMN_CONSTRUCT__7A,
        4,
        Some(&[0x2D, 0x03, 0x00, 0x00]),
    ),
    (58, eix_sites::C_HUD_SPELL_COLUMN_ADD_SPELL, 5, None),
    (59, eix_sites::C_HUD_SPELL_COLUMN_ADD_SPELL__247, 5, None),
    (
        60,
        eix_sites::C_HUD_WEAPON_BAR_CONSTRUCT__7B,
        4,
        Some(&[0xE3, 0x00, 0x00, 0x00]),
    ),
    (
        61,
        eix_sites::C_HUD_WEAPON_BAR_CONSTRUCT__4C,
        4,
        Some(&[0x43, 0x00, 0x00, 0x00]),
    ),
    (
        62,
        eix_sites::C_UI_REGION_SET_RECT__14,
        8,
        Some(&[0x00, 0x00, 0x86, 0x42, 0x00, 0x00, 0xD6, 0x42]),
    ),
    (
        63,
        eix_sites::C_HUD_WEAPON_BAR_ADD_WEAPON__2C0,
        4,
        Some(&[0xF4, 0xC9, 0x5F, 0x00]),
    ),
    (
        64,
        eix_sites::C_HUD_WEAPON_BAR_ADD_WEAPON__2B1,
        4,
        Some(&[0xF8, 0xC9, 0x5F, 0x00]),
    ),
    (65, eix_sites::C_HUD_WEAPON_BAR_HIGHLIGHT, 5, None),
    (66, eix_sites::C_HUD_WEAPON_BAR_HIGHLIGHT__93, 5, None),
    (67, eix_sites::C_HUD_WEAPON_BAR_HIGHLIGHT__11C, 5, None),
    (68, eix_sites::C_HUD_MOVE_PANEL_SETUP_VANILLA, 5, None),
    (69, eix_sites::C_HUD_MOVE_PANEL_SETUP_VANILLA__3B1, 5, None),
    (
        70,
        eix_sites::C_HUD_MOVE_PANEL_SETUP_VANILLA__F,
        4,
        Some(&[0xF3, 0xFF, 0xFF, 0xFF]),
    ),
    (
        71,
        eix_sites::C_HUD_MOVE_PANEL_SETUP_VANILLA__23,
        4,
        Some(&[0x43, 0x00, 0x00, 0x00]),
    ),
    (72, eix_sites::C_HUD_MOVE_PANEL_SHOW_MODE_VANILLA, 5, None),
    (
        73,
        eix_sites::C_HUD_MOVE_PANEL_SHOW_MODE_VANILLA__175,
        5,
        None,
    ),
    (74, eix_sites::C_HUD_CLOCK_SETUP, 5, None),
    (75, eix_sites::C_HUD_CLOCK_SETUP__3B7, 5, None),
    (
        76,
        eix_sites::C_HUD_CLOCK_SETUP__11,
        4,
        Some(&[0xDD, 0x02, 0x00, 0x00]),
    ),
    (
        77,
        eix_sites::C_HUD_CLOCK_SETUP__25,
        4,
        Some(&[0x2D, 0x03, 0x00, 0x00]),
    ),
    (
        78,
        eix_sites::G_LAYOUT_WIDTH,
        4,
        Some(&[0x00, 0x40, 0x4B, 0x44]),
    ),
    (79, eix_sites::C_UI_SURFACE_RELEASE_CONTENTS__117, 9, None),
    (
        80,
        eix_sites::GLOBAL_MAP_MOUSE_MOVE__22C,
        4,
        Some(&[0xF7, 0xCF, 0xFC, 0xFF]),
    ),
    (
        81,
        eix_sites::GLOBAL_MAP_LEFT_DOWN__1D0,
        4,
        Some(&[0x13, 0xD7, 0xFC, 0xFF]),
    ),
    (
        82,
        eix_sites::C_INTERFACE_3D_DRAW_OVERLAYS__14A,
        1,
        Some(&[0x00]),
    ),
    (
        83,
        eix_sites::C_INTERFACE_3D_DRAW_OVERLAYS__14E,
        4,
        Some(&[0x00, 0x03, 0x00, 0x00]),
    ),
    (
        84,
        eix_sites::C_INTERFACE_3D_DRAW_OVERLAYS__153,
        4,
        Some(&[0x20, 0x04, 0x00, 0x00]),
    ),
    (
        85,
        eix_sites::C_INTERFACE_3D_DRAW_OVERLAYS__248,
        1,
        Some(&[0x00]),
    ),
    (
        86,
        eix_sites::C_INTERFACE_3D_DRAW_OVERLAYS__24C,
        4,
        Some(&[0x00, 0x03, 0x00, 0x00]),
    ),
    (
        87,
        eix_sites::C_INTERFACE_3D_DRAW_OVERLAYS__251,
        4,
        Some(&[0x20, 0x04, 0x00, 0x00]),
    ),
    (88, eix_sites::C_INTERFACE_3D_TEXT_EXTENT__8F, 80, None),
    (
        89,
        eix_sites::C_INTERFACE_3D_CREATE__BF,
        6,
        Some(&[0xD8, 0x0D, 0x81, 0x4F, 0x5F, 0x00]),
    ),
    (
        90,
        eix_sites::C_INTERFACE_3D_CREATE__5F1,
        4,
        Some(&[0xC2, 0xF5, 0x28, 0x3C]),
    ),
    (91, eix_sites::AXIS_SWAP_MATRIX__2A, 19, None),
    (
        92,
        eix_sites::C_UI_SURFACE_DRAW_TEXT_IN__74,
        3,
        Some(&[0x31, 0xC9, 0x90]),
    ),
    (
        93,
        eix_sites::C_INTERFACE_3D_SCALE_POINT_TO_SCREEN__7E,
        194,
        None,
    ),
    (
        94,
        eix_sites::C_CAMERA_INPUT_ON_MOUSE_MOVE__39,
        5,
        Some(&[0xE8, 0xB0, 0xCB, 0x00, 0x00]),
    ),
    (
        95,
        eix_sites::GLOBAL_MAP_MOUSE_MOVE__7C,
        5,
        Some(&[0xE8, 0x6D, 0x7D, 0xFC, 0xFF]),
    ),
    (96, eix_sites::C_CAMP_PAGES_CREATE, 5, None),
    (97, eix_sites::C_MP_HERO_SCREEN_CREATE, 7, None),
    (98, eix_sites::C_MP_HERO_SCREEN_SET_MODE, 5, None),
    (99, eix_sites::C_HELP_WINDOW_SHOW_PAGE__574, 7, None),
];

#[test]
fn every_site_has_its_address_and_length_and_its_fixed_bytes() {
    let mut setup = Setup::default();
    setup.options.settings.video_resolution = Some("1056x768".to_owned());
    let sites = build(&setup).unwrap();
    assert_eq!(sites.len(), SITES.len());
    for (l, (n, site, len, patched)) in sites.iter().zip(SITES) {
        assert_eq!(l.label, format!("WideScreen #{n}"));
        assert_eq!((l.write.site(), l.write.size()), (site, len), "#{n}");
        if let Some(patched) = patched {
            assert_eq!(l.write.new_bytes(), patched, "#{n}");
        }
    }
}

#[test]
fn a_bad_resolution_fails_the_group() {
    let mut setup = Setup::default();
    setup.options.settings.video_resolution = Some("bad".to_owned());
    let error = build(&setup).map(|_| ()).unwrap_err();
    assert_eq!(
        format!("{error:?}"),
        "BuildError(\"Invalid resolution: bad\")"
    );
}
