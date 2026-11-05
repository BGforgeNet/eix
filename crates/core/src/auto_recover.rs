//! The crash handler's backup of the hosting player's party (the registry's `AutoRecover`): the folder and file names
//! it writes to and the text it adds to the crash box. Paths are bytes in the ANSI code page, as game.exe's file calls take them.

/// The backups' folder below the mod folder.
const BACKUPS: &[u8] = b"mp\\backup\\";
/// The prefix of the second party file.
const LOOT_PREFIX: &[u8] = b"lootall-";
/// The box line's lead-in.
const LINE_LEAD: &str = "Your saves backed up here: ";

/// How many backup folders one day can hold: the names run from `-0` to `-999`.
const FOLDERS_PER_DAY: u32 = 1000;

/// The backups' folder: the mod folder (ending in `\`, empty for no mod) and `mp\backup\`.
pub fn backups_dir(mod_dir: &[u8]) -> Vec<u8> {
    [mod_dir, BACKUPS].concat()
}

/// A backup folder's name for a day and a number: `DD.MM.YYYY-n`, the day and month in two digits.
pub fn folder_name(day: u16, month: u16, year: u16, n: u32) -> String {
    format!("{day:02}.{month:02}.{year}-{n}")
}

/// The first of the day's names for which `taken` is false, n from 0 to 999; none when all are taken.
pub fn first_free_name(
    day: u16,
    month: u16,
    year: u16,
    mut taken: impl FnMut(&str) -> bool,
) -> Option<String> {
    (0..FOLDERS_PER_DAY)
        .map(|n| folder_name(day, month, year, n))
        .find(|name| !taken(name))
}

/// A backup folder's path, with the `\` the file names follow: the backups' folder `dir`, then `name`.
pub fn folder_path(dir: &[u8], name: &str) -> Vec<u8> {
    [dir, name.as_bytes(), b"\\"].concat()
}

/// The two party files of a backup folder: the party as it stands, then the party after the dead units' loot was
/// collected; both are named for `zone`, the game's current multiplayer zone name.
pub fn party_files(folder: &[u8], zone: &[u8]) -> [Vec<u8>; 2] {
    [
        [folder, zone].concat(),
        [folder, LOOT_PREFIX, zone].concat(),
    ]
}

/// The crash box's line for a backup in `folder`, or none when nothing was backed up.
pub fn box_line(folder: &str) -> String {
    if folder.is_empty() {
        String::new()
    } else {
        format!("{LINE_LEAD}{folder}\n")
    }
}

/// Where eix's bug reports go.
pub const ISSUES: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/issues");

/// The crash box's text: the exception, then `backup_line` ([`box_line`], empty for none), then where to report it
/// and the files to attach: `log`, and `dump` unless it is empty (no dump name was free).
///
/// Diverges on purpose: the original asks for the dump at its author's address, which is not where eix's crashes are
/// fixed.
pub fn crash_text(code: u32, address: usize, backup_line: &str, log: &str, dump: &str) -> String {
    let attach = if dump.is_empty() {
        log.to_owned()
    } else {
        format!("{log}\n{dump}")
    };
    format!(
        "The Game has crashed. Code: 0x{code:08X}, Address: 0x{address:08X}\n{backup_line}Please report it at \
         {ISSUES} and attach:\n{attach}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backups_folder_follows_the_mod_folder() {
        assert_eq!(
            backups_dir(b"C:\\EIStarter\\Mods\\inv\\"),
            b"C:\\EIStarter\\Mods\\inv\\mp\\backup\\"
        );
        assert_eq!(backups_dir(b""), b"mp\\backup\\");
    }

    #[test]
    fn a_folder_name_is_the_day_month_year_and_number() {
        assert_eq!(folder_name(2, 11, 2026, 0), "02.11.2026-0");
        assert_eq!(folder_name(31, 1, 2026, 12), "31.01.2026-12");
        assert_eq!(folder_name(5, 6, 987, 999), "05.06.987-999");
    }

    #[test]
    fn the_first_free_name_skips_the_taken_ones() {
        let taken = ["02.11.2026-0", "02.11.2026-1"];
        assert_eq!(
            first_free_name(2, 11, 2026, |n| taken.contains(&n)),
            Some("02.11.2026-2".to_owned())
        );
        assert_eq!(
            first_free_name(2, 11, 2026, |_| false),
            Some("02.11.2026-0".to_owned())
        );
    }

    #[test]
    fn a_day_holds_a_thousand_folders() {
        let mut asked = 0;
        let found = first_free_name(2, 11, 2026, |_| {
            asked += 1;
            true
        });
        assert_eq!(found, None);
        assert_eq!(asked, 1000);
        assert_eq!(
            first_free_name(2, 11, 2026, |n| n != "02.11.2026-999"),
            Some("02.11.2026-999".to_owned())
        );
    }

    #[test]
    fn a_folder_path_ends_in_a_backslash() {
        assert_eq!(
            folder_path(b"C:\\m\\mp\\backup\\", "02.11.2026-0"),
            b"C:\\m\\mp\\backup\\02.11.2026-0\\"
        );
    }

    #[test]
    fn the_party_files_are_named_for_the_zone() {
        let [first, second] = party_files(b"C:\\m\\mp\\backup\\02.11.2026-0\\", b"zone0");
        assert_eq!(first, b"C:\\m\\mp\\backup\\02.11.2026-0\\zone0");
        assert_eq!(second, b"C:\\m\\mp\\backup\\02.11.2026-0\\lootall-zone0");
        // The zone name is bytes of the ANSI code page, kept as they are.
        let [first, _] = party_files(b"b\\", &[0xC7, 0xEE]);
        assert_eq!(first, [b'b', b'\\', 0xC7, 0xEE]);
    }

    #[test]
    fn the_box_line_names_the_folder_or_is_empty() {
        assert_eq!(
            box_line("C:\\m\\mp\\backup\\02.11.2026-0\\"),
            "Your saves backed up here: C:\\m\\mp\\backup\\02.11.2026-0\\\n"
        );
        assert_eq!(box_line(""), "");
    }

    #[test]
    fn the_crash_text_asks_for_a_report_with_the_log_and_the_dump() {
        let log = "C:\\EIStarter\\addon.log";
        let dump = "C:\\EIStarter\\Dumps\\ei-02.11.2026-0.dmp";
        assert_eq!(
            crash_text(0xC000_0005, 0x4E_6183, "", log, dump),
            "The Game has crashed. Code: 0xC0000005, Address: 0x004E6183\nPlease report it at \
             https://github.com/BGforgeNet/eix/issues and attach:\nC:\\EIStarter\\addon.log\n\
             C:\\EIStarter\\Dumps\\ei-02.11.2026-0.dmp"
        );
        assert_eq!(
            crash_text(
                0xC000_0005,
                0x4E_6183,
                &box_line("C:\\m\\mp\\backup\\02.11.2026-0\\"),
                log,
                ""
            ),
            "The Game has crashed. Code: 0xC0000005, Address: 0x004E6183\nYour saves backed up here: \
             C:\\m\\mp\\backup\\02.11.2026-0\\\nPlease report it at https://github.com/BGforgeNet/eix/issues and \
             attach:\nC:\\EIStarter\\addon.log"
        );
    }
}
