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
    tab.terminal
        .vt_write(b"\r\x1b[2K\x1b]8;;file:///etc/passwd\x1b\\https://example.org\x1b]8;;\x1b\\");
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
