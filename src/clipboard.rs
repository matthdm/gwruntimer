//! The OS clipboard for the game's imgui panel, so Ctrl+V works in the token box.
//!
//! imgui is built here without its Win32 functions (imgui-sys defines `IMGUI_DISABLE_WIN32_FUNCTIONS`) and
//! imgui-rs installs a clipboard that always answers "empty", so pasting did nothing. This reads the real
//! clipboard (text only) when imgui asks for it. Copying out of the panel is not supported: `set` does nothing.

/// Pasted text is cut at this many characters: a token is about 50, and nothing here needs more.
pub const MAX_PASTE_CHARS: usize = 4096;

/// Text from the UTF-16 data of a clipboard (CF_UNICODETEXT): up to the first NUL, at most `MAX_PASTE_CHARS`
/// characters, bad surrogates replaced rather than failing.
pub fn text_from_utf16(units: &[u16]) -> String {
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end]).chars().take(MAX_PASTE_CHARS).collect()
}

/// imgui's clipboard, backed by the Windows clipboard. Install it once with `Context::set_clipboard_backend`.
pub struct WindowsClipboard;

impl hudhook::imgui::ClipboardBackend for WindowsClipboard {
    fn get(&mut self) -> Option<String> {
        read_text().filter(|t| !t.is_empty())
    }

    fn set(&mut self, _value: &str) {}
}

/// The clipboard's text (CF_UNICODETEXT), or None if it holds no text or can't be opened right now.
#[cfg(windows)]
fn read_text() -> Option<String> {
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    const CF_UNICODETEXT: u32 = 13;
    unsafe {
        OpenClipboard(None).ok()?;
        let text = (|| {
            let handle = HGLOBAL(GetClipboardData(CF_UNICODETEXT).ok()?.0);
            let bytes = GlobalSize(handle);
            let ptr = GlobalLock(handle) as *const u16;
            if ptr.is_null() {
                return None;
            }
            // The block is what the owner allocated: read no more than its size, then cut at the first NUL.
            let text = text_from_utf16(std::slice::from_raw_parts(ptr, bytes / 2));
            let _ = GlobalUnlock(handle);
            Some(text)
        })();
        let _ = CloseClipboard();
        text
    }
}

#[cfg(not(windows))]
fn read_text() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn stops_at_the_first_nul() {
        let mut data = utf16("tips_trk_abc");
        data.extend([0, utf16("junk")[0]]);
        assert_eq!(text_from_utf16(&data), "tips_trk_abc");
    }

    #[test]
    fn handles_text_without_a_nul_and_non_ascii() {
        assert_eq!(text_from_utf16(&utf16("héllo ✓")), "héllo ✓");
        assert_eq!(text_from_utf16(&[]), "");
    }

    #[test]
    fn a_broken_surrogate_is_replaced_not_a_failure() {
        assert_eq!(text_from_utf16(&[0x61, 0xD800, 0x62]), "a\u{FFFD}b");
    }

    #[test]
    fn very_long_text_is_cut() {
        let long = vec![0x61u16; MAX_PASTE_CHARS * 3];
        assert_eq!(text_from_utf16(&long).chars().count(), MAX_PASTE_CHARS);
    }
}
