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
mod tests {
    use super::*;
    use ghostty_vt::terminal::ClipboardContent;

    fn write(contents: Vec<ClipboardContent>) -> ClipboardWrite {
        ClipboardWrite {
            location: ClipboardLocation::Standard,
            contents,
            name: String::new(),
            granted: false,
            can_remember: false,
        }
    }
    fn content(mime: &str, data: &[u8]) -> ClipboardContent {
        ClipboardContent {
            mime: mime.into(),
            data: data.to_vec(),
        }
    }
    #[test]
    fn text_clear_invalid_binary_and_selection() {
        let value = write(vec![
            content("text/html", b"<b>hello</b>"),
            content("text/plain;charset=utf-8", "hello 🦀".as_bytes()),
        ]);
        assert_eq!(item(value).unwrap().text().as_deref(), Some("hello 🦀"));
        assert_eq!(item(write(vec![])).unwrap().text().as_deref(), Some(""));
        assert!(item(write(vec![content("text/plain", &[255])])).is_none());
        assert!(item(write(vec![content("image/png", b"not text")])).is_none());
        let mut primary = write(vec![]);
        primary.location = ClipboardLocation::Primary;
        assert!(item(primary).is_none());
        assert!(
            item(write(vec![content(
                "text/plain",
                &vec![b'x'; MAX_BYTES + 1]
            )]))
            .is_none()
        );
    }
}
