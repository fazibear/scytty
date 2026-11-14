use super::*;
use ghostty_vt::TerminalOptions;

fn complete(search: &mut Search, terminal: &Terminal) -> Result<()> {
    while search.tick(terminal)? {}
    Ok(())
}

#[test]
fn live_output_keeps_history_progress_and_updates_active_matches() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions {
        cols: 16,
        rows: 3,
        ..Default::default()
    })?;
    for _ in 0..100 {
        terminal.vt_write(b"needle\r\n");
    }
    let mut search = Search::new(&terminal)?;
    search.set_needle(&terminal, "needle")?;
    for _ in 0..10 {
        assert!(search.tick(&terminal)?);
    }
    let progress = search.row;
    terminal.vt_write(b"needle");
    search.feed(&terminal)?;
    assert!(!search.dirty);
    assert_eq!(search.row, progress);
    complete(&mut search, &terminal)?;
    assert_eq!(search.total_matches(), 101);
    search.select_prev(&mut terminal)?;
    let selected = search.selected_index();
    terminal.vt_write(b"\r\nneedle");
    search.feed(&terminal)?;
    complete(&mut search, &terminal)?;
    assert_eq!(search.total_matches(), 102);
    assert_eq!(search.selected_index(), selected);
    terminal.vt_write(b"\x1b[2J\x1b[Hgone");
    search.feed(&terminal)?;
    complete(&mut search, &terminal)?;
    assert!(search.total_matches() < 102);
    Ok(())
}

#[test]
fn selected_match_survives_live_updates_without_scrollback() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions {
        cols: 24,
        rows: 4,
        ..Default::default()
    })?;
    terminal.vt_write(b"needle needle needle");
    let mut search = Search::new(&terminal)?;
    search.set_needle(&terminal, "needle")?;
    complete(&mut search, &terminal)?;
    search.select_prev(&mut terminal)?;
    assert_eq!(search.selected_index(), Some(2));
    terminal.vt_write(b"\r\nneedle");
    search.feed(&terminal)?;
    complete(&mut search, &terminal)?;
    assert_eq!(search.total_matches(), 4);
    assert_eq!(search.selected_index(), Some(2));
    Ok(())
}

#[test]
fn wrapped_matches_can_overlap_and_reindex_only_bounded_context() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions {
        cols: 4,
        rows: 3,
        ..Default::default()
    })?;
    terminal.vt_write(b"aaaaaaaaaaaaaaaaaaaa");
    let mut search = Search::new(&terminal)?;
    search.set_needle(&terminal, "aaa")?;
    complete(&mut search, &terminal)?;
    assert_eq!(search.total_matches(), 18);
    terminal.vt_write(b"aaaa");
    search.feed(&terminal)?;
    assert!(search.row > 0);
    complete(&mut search, &terminal)?;
    assert_eq!(search.total_matches(), 22);
    assert!(search.text.len() < search.folded.len());
    Ok(())
}

#[test]
fn ascii_search_keeps_native_space_wrap_and_overlap_semantics() -> Result<()> {
    for (text, query) in [
        ("needle", "needle "),
        ("needle ", "needle "),
        ("  ", " "),
        ("needle", " "),
        ("a\x1b[6GX", "a    x"),
        ("abcdneedle", "needle"),
        ("abcdefg Needle", "g needle"),
        ("abcdefg Needle", "gneedle"),
        ("needle\u{3000}", "needle\u{3000}"),
        ("s\r\ns", "ss"),
        ("aaaaaaaaaaaa", "aaa"),
    ] {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 3,
            ..Default::default()
        })?;
        terminal.vt_write(text.as_bytes());
        let mut native = ghostty_vt::search::Search::new(&terminal)?;
        native.set_needle(&terminal, Some(query))?;
        native.run(&terminal)?;
        let mut search = Search::new(&terminal)?;
        search.set_needle(&terminal, query)?;
        complete(&mut search, &terminal)?;
        assert_eq!(
            search.total_matches(),
            native.total_matches()?,
            "{text:?} / {query:?}"
        );
    }
    Ok(())
}

#[test]
fn resize_screen_switch_and_history_eviction_discard_stale_coordinates() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions {
        cols: 8,
        rows: 2,
        ..Default::default()
    })?;
    for _ in 0..40 {
        terminal.vt_write("Straße\r\n".as_bytes());
    }
    let mut search = Search::new(&terminal)?;
    search.set_needle(&terminal, "strasse")?;
    complete(&mut search, &terminal)?;
    assert_eq!(search.total_matches(), 40);
    search.select_prev(&mut terminal)?;
    terminal.resize(4, 3, 8, 16)?;
    search.feed(&terminal)?;
    assert!(search.dirty);
    complete(&mut search, &terminal)?;
    assert_eq!(search.total_matches(), 40);
    assert_eq!(search.selected_index(), Some(39));
    terminal.vt_write(b"\x1b[?1049hclean");
    search.feed(&terminal)?;
    complete(&mut search, &terminal)?;
    assert_eq!(search.total_matches(), 0);
    terminal.vt_write(b"\x1b[?1049l\x1b[3J");
    search.feed(&terminal)?;
    complete(&mut search, &terminal)?;
    assert!(search.total_matches() < 40);
    Ok(())
}
