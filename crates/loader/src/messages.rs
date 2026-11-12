//! The loader's error box texts: the original loader's wording, with our own title.
#![forbid(unsafe_code)]

use alloc::format;
use alloc::string::String;

use eix_engine::exe::ExeId;
use eix_engine::win::narrow;

/// The title of every error box.
pub(crate) const TITLE: &str = concat!("eix loader ", env!("CARGO_PKG_VERSION"), " error");

/// game.exe is not a build the loader knows, followed by what it is.
pub(crate) fn invalid_version(id: &ExeId) -> String {
    format!(
        "Invalid version of game.exe!\n\nImage base {:#x}, entry point RVA {:#x}, image size {:#x}, time stamp \
         {:#010x}.",
        id.base, id.entry_rva, id.size_of_image, id.time_date_stamp
    )
}

/// No candidate path names an existing file: the list of those tried, one per line.
pub(crate) fn not_found(tried: &[impl AsRef<[u16]>]) -> String {
    let mut text = String::from("Unable to find addon.dll. Tried to find in these paths:\n");
    for path in tried {
        text.push_str("- ");
        text.push_str(&narrow(path.as_ref()));
        text.push('\n');
    }
    text
}

/// `LoadLibraryW` failed with the system error `code`.
pub(crate) fn load_failed(code: u32, path: &[u16]) -> String {
    format!(
        "Unable to load addon.dll. Error code: {code}. Addon path: {}",
        narrow(path)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn title_names_the_version() {
        assert_eq!(
            TITLE,
            format!("eix loader {} error", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn lists_every_path_tried() {
        let tried = [
            w("C:\\EIStarter\\Mods\\inv\\addon.dll"),
            w("C:\\EIStarter\\addon.dll"),
        ];
        assert_eq!(
            not_found(&tried),
            "Unable to find addon.dll. Tried to find in these paths:\n\
             - C:\\EIStarter\\Mods\\inv\\addon.dll\n\
             - C:\\EIStarter\\addon.dll\n"
        );
        let none: [Vec<u16>; 0] = [];
        assert_eq!(
            not_found(&none),
            "Unable to find addon.dll. Tried to find in these paths:\n"
        );
    }

    #[test]
    fn load_failure_names_code_and_path() {
        assert_eq!(
            load_failed(193, &w("C:\\EIStarter\\addon.dll")),
            "Unable to load addon.dll. Error code: 193. Addon path: C:\\EIStarter\\addon.dll"
        );
    }

    #[test]
    fn invalid_version_names_the_header_fields() {
        let id = ExeId {
            base: eix_sites::IMAGE_BASE.ru(),
            entry_rva: 0x2E_C1E6,
            size_of_image: 0x3D_7000,
            time_date_stamp: 0x3ABF_0DB3,
        };
        assert_eq!(
            invalid_version(&id),
            "Invalid version of game.exe!\n\nImage base 0x400000, entry point RVA 0x2ec1e6, image size 0x3d7000, \
             time stamp 0x3abf0db3."
        );
    }
}
