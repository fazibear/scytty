use super::*;
use crate::tab::Tab;
use ghostty_vt::{
    TerminalOptions,
    selection::{FormatOptions, Selection},
    terminal::ScrollViewport,
};

fn tab(text: &str, cols: u16, rows: u16) -> Result<Tab> {
    let mut tab = Tab::new(Terminal::new(TerminalOptions {
        cols,
        rows,
        ..Default::default()
    })?)?;
    tab.terminal.vt_write(text.as_bytes());
    tab.snapshot()?;
    Ok(tab)
}
fn row(tab: &Tab, y: usize) -> String {
    tab.bidi
        .cells
        .iter()
        .filter(|c| c.y == y && c.width > 0)
        .map(|c| c.text.as_str())
        .collect::<String>()
        .trim_end()
        .to_owned()
}

#[test]
fn mixed_runs_keep_logical_text_and_reversible_coordinates() -> Result<()> {
    let tab = tab("a אבג z", 16, 2)?;
    assert_eq!(row(&tab, 0), "a גבא z");
    for cell in &tab.cells {
        let x = tab.bidi.visual_x(cell.x, cell.y);
        assert_eq!(
            tab.bidi.logical_point(Point::Viewport(PointCoordinate {
                x: x as u16,
                y: cell.y as u32
            })),
            Point::Viewport(PointCoordinate {
                x: cell.x as u16,
                y: cell.y as u32
            })
        );
    }
    let selection = Selection::new(
        tab.terminal
            .grid_ref(Point::Viewport(PointCoordinate { x: 0, y: 0 }))?,
        tab.terminal
            .grid_ref(Point::Viewport(PointCoordinate { x: 6, y: 0 }))?,
        false,
    );
    assert_eq!(
        tab.terminal
            .format_selection_alloc(FormatOptions::new().with_selection(&selection))?
            .unwrap()
            .to_vec(),
        "a אבג z".as_bytes()
    );
    Ok(())
}

#[test]
fn numbers_brackets_combining_clusters_wide_cells_and_styles() -> Result<()> {
    let tab = tab("a (אב\u{05b0}ג 12) 界 👩🏽‍💻 z", 32, 2)?;
    assert_eq!(row(&tab, 0), "a (12 גב\u{05b0}א) 界 👩🏽‍💻 z");
    let logical: String = tab
        .cells
        .iter()
        .filter(|c| c.y == 0 && c.width > 0)
        .map(|c| c.text.as_str())
        .collect();
    let info = BidiInfo::new(&logical, Some(Level::ltr()));
    let expected = info.reorder_line(&info.paragraphs[0], 0..logical.len());
    // The algorithm's scalar reordering is not used for combining clusters: the mark stays on ב.
    assert!(row(&tab, 0).contains("ב\u{05b0}"));
    assert!(row(&tab, 0).contains("12"));
    assert!(row(&tab, 0).contains("界 👩🏽‍💻"));
    assert_eq!(
        row(&tab, 0)
            .chars()
            .filter(|c| *c == '(' || *c == ')')
            .count(),
        expected.chars().filter(|c| *c == '(' || *c == ')').count()
    );
    for wide in tab.cells.iter().filter(|c| c.width == 2) {
        let x = tab.bidi.visual_x(wide.x, wide.y);
        assert_eq!(tab.bidi.visual_x(wide.x + 1, wide.y), x + 1);
    }
    let tab = self::tab("x \x1b[31;1mאב\x1b[0m y", 16, 2)?;
    for original in tab.cells.iter().filter(|c| c.bold) {
        let x = tab.bidi.visual_x(original.x, original.y);
        let visual = tab
            .bidi
            .cells
            .iter()
            .find(|c| c.x == x && c.y == original.y)
            .unwrap();
        assert_eq!(visual.text, original.text);
        assert!(visual.bold && visual.fg == original.fg);
    }
    Ok(())
}

#[test]
fn wrapped_paragraph_context_survives_viewport_scrolling_and_snapshots() -> Result<()> {
    let mut tab = tab("prefix אבג 1234 סוף tail\r\nmore\r\nlast", 8, 2)?;
    tab.terminal.scroll_viewport(ScrollViewport::Top);
    tab.snapshot()?;
    let second = row(&tab, 1);
    tab.terminal.scroll_viewport(ScrollViewport::Delta(1));
    tab.snapshot()?;
    assert_eq!(row(&tab, 0), second);
    let mut restored = Tab::new(crate::tests::roundtrip(&tab.terminal)?)?;
    restored.terminal.scroll_viewport(ScrollViewport::Row(
        tab.terminal.scrollbar()?.offset as usize,
    ));
    restored.snapshot()?;
    assert_eq!(row(&tab, 0), row(&restored, 0));
    Ok(())
}

#[test]
fn mirrored_brackets_isolates_and_wide_glyphs_follow_rtl_levels() -> Result<()> {
    let mixed = tab("a אב(גד) z", 24, 2)?;
    assert_eq!(row(&mixed, 0), "a (דג)בא z");
    let isolated = tab("a \u{2067}אב 12\u{2069} z", 24, 2)?;
    assert!(row(&isolated, 0).contains("בא"));
    assert!(!row(&isolated, 0).contains(['\u{2067}', '\u{2069}']));
    let wide = tab("אב😀גד", 16, 2)?;
    assert_eq!(row(&wide, 0), "דג😀בא");
    for cell in &wide.cells {
        let x = wide.bidi.visual_x(cell.x, cell.y);
        assert_eq!(
            wide.bidi.logical_point(Point::Viewport(PointCoordinate {
                x: x as u16,
                y: cell.y as u32
            })),
            Point::Viewport(PointCoordinate {
                x: cell.x as u16,
                y: cell.y as u32
            })
        );
    }
    let wrapped = tab("אבג界", 4, 3)?;
    assert_eq!(row(&wrapped, 0), "גבא");
    assert_eq!(row(&wrapped, 1), "界");
    Ok(())
}

#[test]
fn links_and_search_highlights_use_the_same_visual_projection() -> Result<()> {
    let tab = tab(
        "\x1b]8;;https://example.com\x1b\\a אבג z\x1b]8;;\x1b\\",
        16,
        2,
    )?;
    let bounds = gpui::Bounds::new(
        gpui::point(gpui::px(0.), gpui::px(0.)),
        gpui::size(gpui::px(160.), gpui::px(40.)),
    );
    let cell = gpui::size(gpui::px(10.), gpui::px(20.));
    for x in 2..5 {
        let visual = tab.bidi.visual_x(x, 0);
        let point = tab.point(
            gpui::point(gpui::px(visual as f32 * 10. + 5.), gpui::px(5.)),
            bounds,
            cell,
        )?;
        assert_eq!(
            point,
            Point::Viewport(PointCoordinate { x: x as u16, y: 0 })
        );
        assert_eq!(
            crate::links::at_terminal(&tab, point)?.as_deref(),
            Some("https://example.com")
        );
    }
    let mut search = crate::search::Search::new(&tab.terminal)?;
    while crate::find::advance(&mut search, &tab.terminal, "אב")? {}
    let mut visual: Vec<_> = crate::find::highlights(&search, &tab.terminal)?
        .iter()
        .flat_map(|highlight| {
            (highlight.x..highlight.x + highlight.width).map(|x| tab.bidi.visual_x(x, highlight.y))
        })
        .collect();
    visual.sort_unstable();
    assert_eq!(visual, vec![3, 4]);
    Ok(())
}

#[test]
fn mouse_reports_logical_cells_and_preserves_pixel_offsets() -> Result<()> {
    use ghostty_vt::mouse;
    let mut tab = tab("a אבג z\x1b[?1000h\x1b[?1006h", 16, 2)?;
    let mut event = mouse::Event::new()?;
    event
        .set_action(mouse::Action::Press)
        .set_button(Some(mouse::Button::Left))
        .set_position(mouse::Position { x: 23., y: 7. });
    let cell = gpui::size(gpui::px(10.), gpui::px(20.));
    assert_eq!(tab.mouse(&event, cell, true)?, b"\x1b[<0;5;1M");
    tab.terminal.vt_write(b"\x1b[?1006l\x1b[?1016h");
    assert_eq!(tab.mouse(&event, cell, true)?, b"\x1b[<0;43;7M");
    assert_eq!(event.position().x, 23.);
    Ok(())
}
