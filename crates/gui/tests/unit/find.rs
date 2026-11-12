use super::*;
use ghostty_vt::{TerminalOptions, terminal::ScrollViewport};
#[test]
fn edits_unicode_and_composition_without_splitting_characters() {
    let mut find = Find::default();
    find.replace(None, "a😀é");
    assert_eq!(find.utf16_range(find.selection.clone()), 4..4);
    find.edit("backspace", false);
    assert_eq!(find.query, "a😀");
    find.replace(Some(1..3), "界");
    assert_eq!(find.query, "a界");
    let marked = find.replace(None, "に");
    find.marked = Some(marked);
    find.replace(None, "日本");
    assert_eq!(find.query, "a界日本");
    find.selection = 0..find.query.len();
    find.replace(None, "x\ny\t");
    assert_eq!(find.query, "xy");
    find.replace(None, &"é".repeat(5000));
    assert!(find.query.len() <= 4096);
}
#[test]
fn shift_arrows_extend_and_shrink_unicode_selection() {
    let mut find = Find::default();
    find.replace(None, "a😀é");
    find.edit("left", true);
    find.edit("left", true);
    assert_eq!(&find.query[find.selection.clone()], "😀é");
    find.edit("right", true);
    assert_eq!(&find.query[find.selection.clone()], "é");
    find.edit("right", true);
    assert!(find.selection.is_empty());
}

#[test]
fn highlights_wrapped_and_wide_matches() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions {
        cols: 5,
        rows: 3,
        ..Default::default()
    })?;
    terminal.vt_write("abcdneedle\r\n界".as_bytes());
    let mut search = Search::new(&terminal)?;
    while advance(&mut search, &terminal, "needle")? {}
    assert_eq!(search.total_matches()?, 1);
    let rects = highlights(&search, &terminal)?;
    assert_eq!(rects.iter().map(|h| h.width).sum::<usize>(), 6);
    while advance(&mut search, &terminal, "界")? {}
    assert_eq!(
        highlights(&search, &terminal)?
            .iter()
            .map(|h| h.width)
            .sum::<usize>(),
        2
    );
    while advance(&mut search, &terminal, "")? {}
    assert_eq!(search.total_matches()?, 0);
    assert!(highlights(&search, &terminal)?.is_empty());
    Ok(())
}

#[test]
fn searches_scrollback_navigates_and_highlights_without_changing_selection() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions {
        cols: 12,
        rows: 3,
        ..Default::default()
    })?;
    terminal.vt_write(b"Needle\r\na\r\nb\r\nc\r\nneedle\r\nend");
    let mut search = Search::new(&terminal)?;
    while advance(&mut search, &terminal, "needle")? {}
    assert_eq!(search.total_matches()?, 2);
    assert!(search.select_next(&terminal)?);
    search.feed(&terminal)?;
    assert!(!highlights(&search, &terminal)?.is_empty());
    assert!(search.select_next(&terminal)?);
    search.feed(&terminal)?;
    assert!(highlights(&search, &terminal)?.iter().any(|h| h.selected));
    assert!(search.select_prev(&terminal)?);
    assert!(terminal.selection()?.is_none());
    terminal.scroll_viewport(ScrollViewport::Bottom);
    terminal.vt_write(b"\r\nNEEDLE");
    while advance(&mut search, &terminal, "needle")? {}
    assert_eq!(search.total_matches()?, 3);
    while advance(&mut search, &terminal, "missing")? {}
    assert_eq!(search.total_matches()?, 0);
    Ok(())
}
