use super::*;
use ghostty_vt::{TerminalOptions, terminal::ScrollViewport};
#[test]
fn edits_unicode_and_composition_without_splitting_characters() {
    let mut find = Find::default();
    find.replace(None, "a😀é");
    assert_eq!(
        crate::unicode::utf16_range(&find.query, find.selection.clone()),
        4..4
    );
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
fn edits_complete_graphemes_and_bounds_pastes_without_splitting_them() {
    for cluster in ["e\u{301}", "👩🏽‍💻", "🇵🇱", "1️⃣", "क्ष"] {
        let mut find = Find::default();
        find.replace(None, &format!("a{cluster}z"));
        find.edit("left", false);
        find.edit("left", true);
        assert_eq!(&find.query[find.selection.clone()], cluster);
        find.edit("backspace", false);
        assert_eq!(find.query, "az");
        find.replace(None, cluster);
        find.edit("left", false);
        find.edit("delete", false);
        assert_eq!(find.query, "az");
        find.selection = 1..1;
        find.replace(None, cluster);
        find.edit("backspace", false);
        assert_eq!(find.query, "az");
        let mut find = Find::default();
        find.replace(None, &"x".repeat(4095));
        find.replace(None, cluster);
        assert_eq!(find.query.len(), 4095);
    }
}

#[test]
fn keyboard_deletion_at_an_ime_caret_inside_a_grapheme_removes_the_whole_cluster() {
    for key in ["backspace", "delete"] {
        let mut find = Find::default();
        find.replace(None, "e\u{301}x");
        find.selection = 1..1;
        find.edit(key, false);
        assert_eq!(find.query, "x");
    }
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
fn unicode_search_highlights_grapheme_cell_spans_without_normalizing_text() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    terminal.vt_write("é e\u{301} 界 👩🏽‍💻 🇵🇱 क्ष".as_bytes());
    let mut search = Search::new(&terminal)?;
    for (needle, width) in [
        ("é", 1),
        ("e\u{301}", 1),
        ("界", 2),
        ("👩🏽‍💻", 2),
        ("🇵🇱", 2),
        ("क्ष", 2),
    ] {
        while advance(&mut search, &terminal, needle)? {}
        assert_eq!(search.total_matches(), 1, "{needle:?}");
        assert_eq!(
            highlights(&search, &terminal)?
                .iter()
                .map(|h| h.width)
                .sum::<usize>(),
            width,
            "{needle:?}"
        );
    }
    Ok(())
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
    assert_eq!(search.total_matches(), 1);
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
    assert_eq!(search.total_matches(), 0);
    assert!(highlights(&search, &terminal)?.is_empty());
    Ok(())
}

#[test]
fn full_case_folding_maps_expansions_back_to_original_cells() -> Result<()> {
    for (text, needle, count) in [
        ("Straße STRASSE straße", "STRASSE", 3),
        ("Σ σ ς", "σ", 3),
        ("ÉCOLE école", "école", 2),
        ("ﬃ FFI", "ffi", 2),
        ("K K k", "k", 3),
        ("İ I ı i", "i", 2),
        ("İ I", "i\u{307}", 1),
        ("sß", "ss", 1),
        ("ß", "s", 0),
        ("e\u{301}", "é", 0),
    ] {
        let mut terminal = Terminal::new(TerminalOptions::default())?;
        terminal.vt_write(text.as_bytes());
        let mut search = Search::new(&terminal)?;
        while advance(&mut search, &terminal, needle)? {}
        assert_eq!(search.total_matches(), count, "{text:?} / {needle:?}");
    }
    let mut terminal = Terminal::new(TerminalOptions {
        cols: 4,
        rows: 3,
        ..Default::default()
    })?;
    terminal.vt_write("Straße".as_bytes());
    let mut search = Search::new(&terminal)?;
    while advance(&mut search, &terminal, "STRASSE")? {}
    assert_eq!(search.total_matches(), 1);
    assert_eq!(
        highlights(&search, &terminal)?
            .iter()
            .map(|h| h.width)
            .sum::<usize>(),
        6
    );
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
    assert_eq!(search.total_matches(), 2);
    assert!(search.select_next(&mut terminal)?);
    assert!(!highlights(&search, &terminal)?.is_empty());
    assert!(search.select_next(&mut terminal)?);
    assert!(highlights(&search, &terminal)?.iter().any(|h| h.selected));
    assert!(search.select_prev(&mut terminal)?);
    assert!(terminal.selection()?.is_none());
    terminal.scroll_viewport(ScrollViewport::Bottom);
    terminal.vt_write(b"\r\nNEEDLE");
    search.feed(&terminal)?;
    while advance(&mut search, &terminal, "needle")? {}
    assert_eq!(search.total_matches(), 3);
    while advance(&mut search, &terminal, "missing")? {}
    assert_eq!(search.total_matches(), 0);
    Ok(())
}
