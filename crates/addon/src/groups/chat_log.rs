//! ChatLog (`[DEV] ChatHtmlFile`): every chat line the game server relays (`ServerOnSay` 0x41B1C0) is appended to
//! an HTML file, a path relative to the game's working folder. A new file starts with the page head; each line is
//! `<font class=cN>name</font>: text<BR>`, N being the speaker's colour class. At 0x41B25B the server has
//! pushed the class, the name and the text for its own format.
#![expect(
    unsafe_code,
    reason = "reading the chat line's C strings the game passes"
)]

use std::io::Write;
use std::sync::OnceLock;

use eix_core::cli::crt_text;
use eix_engine::hook::{self, Context, Mode};

use super::{BuildError, Labelled, Setup};
use crate::setup::path;

/// The page head written into a new file (charset windows-1251, the game's code page). The original names the
/// classes `.1` to `.7`, which CSS does not accept as class selectors, so its colours never show.
const HEAD: &[u8] = b"<html><head><title>Chat log</title><meta http-equiv=\"Content-Type\" content=\"text/html; \
charset=windows-1251\"><style type=\"text/css\"><!--body{background-color:#000000;color:#FFFFFF}.c1{color:#0000FF}\
.c2{color:#00FF00}.c3{color:#00FFFF}.c4{color:#FF0000}.c5{color:#FF00FF}.c6{color:#FFFF00}.c7{color:#FFFFFF}-->\
</style></head>\n";

static FILE: OnceLock<std::path::PathBuf> = OnceLock::new();

/// A C string the game passed, or nothing for null.
///
/// # Safety
/// `address` must be null or a NUL-terminated string that lives for the call.
unsafe fn c_text(address: u32) -> Vec<u8> {
    let p = core::ptr::with_exposed_provenance::<core::ffi::c_char>(address as usize);
    if p.is_null() {
        return Vec::new();
    }
    // SAFETY: the caller guarantees a live NUL-terminated string.
    unsafe { core::ffi::CStr::from_ptr(p) }.to_bytes().to_vec()
}

/// The line as the original formats it, `\n` written as `\r\n` (its file is opened in text mode), with `&`, `<` and
/// `>` of the name and the text escaped (the original writes them raw, so a message could hold markup).
fn line(class: u32, name: &[u8], text: &[u8]) -> Vec<u8> {
    let mut out = format!("<font class=c{class}>").into_bytes();
    escape_into(&mut out, name);
    out.extend_from_slice(b"</font>: ");
    escape_into(&mut out, text);
    out.extend_from_slice(b"<BR>");
    crt_text(&out)
}

fn escape_into(out: &mut Vec<u8>, text: &[u8]) {
    for &c in text {
        match c {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            _ => out.push(c),
        }
    }
}

extern "C" fn handler(_: usize, ctx: &mut Context) {
    let Some(file) = FILE.get() else {
        return;
    };
    // The copied `push 0x783D88` ran first: the server's three words follow it.
    let (Some(class), Some(name), Some(text)) =
        (ctx.stack_word(1), ctx.stack_word(2), ctx.stack_word(3))
    else {
        return;
    };
    // SAFETY: at 0x41B25B the server pushed the speaker's name and the message, both its own live C strings.
    let name = unsafe { c_text(name) };
    // SAFETY: as above.
    let text = unsafe { c_text(text) };
    let new = !file.is_file();
    // The chat log is a convenience file; a line that cannot be written is dropped, as the original drops it.
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
    else {
        return;
    };
    if new {
        let _ = f.write_all(&crt_text(HEAD));
    }
    let _ = f.write_all(&line(class, &name, &text));
}

pub(super) fn build(setup: &Setup) -> Result<Vec<Labelled>, BuildError> {
    let _ = FILE.set(path(&setup.options.config.dev.chat_html_file));
    Ok(vec![Labelled {
        label: "Chat log".to_owned(),
        // push 0x783D88
        write: hook::context_call(
            eix_sites::SERVER_ON_SAY__9B,
            &[0x68, 0x88, 0x3D, 0x78, 0x00],
            Mode::Before,
            handler,
            0,
        )?,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_html_with_the_text_escaped() {
        assert_eq!(
            line(3, b"Hero", b"hi"),
            b"<font class=c3>Hero</font>: hi<BR>"
        );
        assert_eq!(
            line(1, b"<b>", b"x & </font>"),
            b"<font class=c1>&lt;b&gt;</font>: x &amp; &lt;/font&gt;<BR>"
        );
        let head = String::from_utf8_lossy(HEAD);
        for class in 1..=7 {
            assert!(head.contains(&format!(".c{class}{{color:#")), "{class}");
        }
        assert!(!head.contains(")}"));
        assert!(crt_text(HEAD).ends_with(b"</head>\r\n"));
        assert_eq!(crt_text(HEAD).len(), HEAD.len() + 1);
    }
}
