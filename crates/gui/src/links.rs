use anyhow::Result;
use ghostty_vt::terminal::Point;

pub fn at_terminal(tab: &crate::Tab, point: Point) -> Result<Option<String>> {
    let reference = tab.terminal.grid_ref(point)?;
    let mut uri = vec![0; 8192];
    match reference.hyperlink_uri(&mut uri) {
        Ok(length) if length > 0 => {
            return Ok(std::str::from_utf8(&uri[..length])
                .ok()
                .filter(|url| safe_url(url))
                .map(str::to_owned));
        }
        // shortcut: links over 8 KiB are ignored; increase the bound if needed.
        Err(ghostty_vt::Error::OutOfSpace { .. }) => return Ok(None),
        Err(error) => return Err(error.into()),
        _ => {}
    }
    let Point::Viewport(coordinate) = point else {
        return Ok(None);
    };
    let mut row = String::new();
    let mut index = None;
    let mut previous = 0;
    for cell in tab
        .cells
        .iter()
        .filter(|cell| cell.y == coordinate.y as usize)
    {
        let start = row.len();
        if cell.x == usize::from(coordinate.x) {
            index = Some(if cell.text.is_empty() && !cell.narrow {
                previous
            } else {
                start
            });
        }
        if cell.text.is_empty() {
            if cell.narrow {
                row.push(' ');
            }
        } else {
            row.push_str(&cell.text);
        }
        previous = start;
    }
    Ok(index.and_then(|index| at(&row, index)).map(str::to_owned))
}

pub fn hovered_cells(tab: &crate::Tab, point: Point) -> Result<std::ops::Range<usize>> {
    let Point::Viewport(coordinate) = point else {
        return Ok(0..0);
    };
    let Some(url) = at_terminal(tab, point)? else {
        return Ok(0..0);
    };
    let Some(index) = tab
        .cells
        .iter()
        .position(|cell| cell.x == usize::from(coordinate.x) && cell.y == coordinate.y as usize)
    else {
        return Ok(0..0);
    };
    let matches = |index: usize| -> Result<bool> {
        let cell = &tab.cells[index];
        Ok(at_terminal(
            tab,
            Point::Viewport(ghostty_vt::terminal::PointCoordinate {
                x: cell.x as u16,
                y: cell.y as u32,
            }),
        )?
        .as_deref()
            == Some(url.as_str()))
    };
    let mut start = index;
    let mut end = index + 1;
    while start > 0 && matches(start - 1)? {
        start -= 1;
    }
    while end < tab.cells.len() && matches(end)? {
        end += 1;
    }
    Ok(start..end)
}

pub fn safe_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https")
        && !rest.split(['/', '?', '#']).next().unwrap_or("").is_empty()
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
        && !url.contains('\\')
}

pub fn at(text: &str, index: usize) -> Option<&str> {
    // shortcut: plain URLs are detected within a visible row; use OSC 8 for wrapped links.
    for (start, _) in text.match_indices("http") {
        let candidate = text[start..]
            .split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\''))
            .next()?;
        let mut url = candidate.trim_end_matches(['.', ',', ';', '!', ':']);
        for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
            while url.ends_with(close) && url.matches(close).count() > url.matches(open).count() {
                url = &url[..url.len() - close.len_utf8()];
            }
        }
        if index >= start && index < start + url.len() && safe_url(url) {
            return Some(url);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_links_and_unsafe_schemes() -> Result<()> {
        use ghostty_vt::{Terminal, TerminalOptions, terminal::PointCoordinate};
        let mut tab = crate::Tab::new(Terminal::new(TerminalOptions::default())?)?;
        let point = |x| Point::Viewport(PointCoordinate { x, y: 0 });
        tab.terminal.vt_write("你好 https://example.com".as_bytes());
        tab.snapshot()?;
        assert_eq!(
            at_terminal(&tab, point(10))?.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            tab.cells[hovered_cells(&tab, point(10))?]
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>(),
            "https://example.com"
        );
        tab.terminal
            .vt_write(b"\r\x1b[2K\x1b]8;;https://example.org\x1b\\label\x1b]8;;\x1b\\");
        tab.snapshot()?;
        assert_eq!(
            at_terminal(&tab, point(2))?.as_deref(),
            Some("https://example.org")
        );
        tab.terminal.vt_write(
            b"\r\x1b[2K\x1b]8;;file:///etc/passwd\x1b\\https://example.org\x1b]8;;\x1b\\",
        );
        tab.snapshot()?;
        assert_eq!(at_terminal(&tab, point(2))?, None);
        assert_eq!(hovered_cells(&tab, point(2))?, 0..0);
        Ok(())
    }
    #[test]
    fn hover_includes_wrapped_osc8_label() -> Result<()> {
        use ghostty_vt::{Terminal, TerminalOptions, terminal::PointCoordinate};
        let mut tab = crate::Tab::new(Terminal::new(TerminalOptions {
            cols: 10,
            rows: 3,
            ..Default::default()
        })?)?;
        tab.terminal
            .vt_write(b"\x1b]8;;https://example.org\x1b\\wrapped link text\x1b]8;;\x1b\\");
        tab.snapshot()?;
        let range = hovered_cells(&tab, Point::Viewport(PointCoordinate { x: 2, y: 1 }))?;
        assert_eq!(
            tab.cells[range]
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>(),
            "wrapped link text"
        );
        Ok(())
    }
    #[test]
    fn urls_and_punctuation() {
        let text = "你好 (https://example.com/a_(b)). next http://localhost:3000/x?q=1";
        assert_eq!(
            at(text, text.find("example").unwrap()),
            Some("https://example.com/a_(b)")
        );
        assert_eq!(
            at(text, text.find("localhost").unwrap()),
            Some("http://localhost:3000/x?q=1")
        );
        assert_eq!(at(text, 0), None);
        assert_eq!(at(text, text.len()), None);
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://",
            "https:///path",
            "https://host\ncommand",
            "https://host\\path",
        ] {
            assert!(!safe_url(url), "{url}");
        }
        assert!(safe_url("HTTPS://example.com"));
    }
}
