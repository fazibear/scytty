use ghostty_vt::terminal::{ClipboardLocation, ClipboardWrite};
use gpui::ClipboardItem;
use scytty_common::clipboard::MAX_BYTES;

pub fn item(write: ClipboardWrite) -> Option<ClipboardItem> {
    // GPUI exposes the system clipboard, not X11's primary/selection clipboards.
    if write.location != ClipboardLocation::Standard
        || write.contents.iter().fold(0usize, |size, content| {
            size.saturating_add(content.data.len())
        }) > MAX_BYTES
    {
        return None;
    }
    if write.contents.is_empty() {
        return Some(ClipboardItem::new_string(String::new()));
    }
    let content = write.contents.into_iter().find(|content| {
        content
            .mime
            .split(';')
            .next()
            .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/plain"))
            || matches!(content.mime.as_str(), "UTF8_STRING" | "TEXT")
    })?;
    Some(ClipboardItem::new_string(
        String::from_utf8(content.data).ok()?,
    ))
}

#[cfg(test)]
#[path = "../tests/unit/clipboard.rs"]
mod tests;
