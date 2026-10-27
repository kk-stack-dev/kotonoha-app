//! Plain-text clipboard access. Restoring the previous clipboard restores its text only.

pub fn set_text(text: &str) -> Result<(), arboard::Error> {
    arboard::Clipboard::new()?.set_text(text)
}

pub fn get_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}
