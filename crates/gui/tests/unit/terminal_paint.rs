use super::*;
use gpui::rgb;

#[test]
fn block_elements_cover_the_expected_cell_regions() {
    assert_eq!(block_rectangles("▀"), Some(vec![(0, 0, 8, 4)]));
    assert_eq!(block_rectangles("▄"), Some(vec![(0, 4, 8, 4)]));
    assert_eq!(block_rectangles("█"), Some(vec![(0, 0, 8, 8)]));
    assert_eq!(block_rectangles("▌"), Some(vec![(0, 0, 4, 8)]));
    assert_eq!(block_rectangles("▐"), Some(vec![(4, 0, 4, 8)]));
    for ch in '▁'..='▏' {
        assert!(block_rectangles(&ch.to_string()).is_some());
    }
    let expected_areas = [16, 16, 16, 48, 32, 48, 48, 16, 32, 48];
    for (ch, area) in ('▖'..='▟').zip(expected_areas) {
        let rects = block_rectangles(&ch.to_string()).unwrap();
        assert_eq!(rects.iter().map(|r| r.2 * r.3).sum::<u8>(), area);
        assert!(rects.iter().all(|r| r.0 + r.2 <= 8 && r.1 + r.3 <= 8));
    }
    for text in ["a", "//", "─", "░", "█\u{301}", ""] {
        assert!(block_rectangles(text).is_none());
    }
}

#[test]
fn block_edges_meet_at_fractional_windows_scale_factors() {
    let origin = point(px(2.3), px(3.7));
    let cell = size(px(9.3), px(18.7));
    for scale in [1., 1.25, 1.5, 1.75, 2.] {
        let full = block_bounds(origin, cell, (0, 0, 8, 8), scale);
        let upper = block_bounds(origin, cell, (0, 0, 8, 4), scale);
        let lower = block_bounds(origin, cell, (0, 4, 8, 4), scale);
        assert_eq!(upper.origin.y + upper.size.height, lower.origin.y);
        assert_eq!(upper.size.height + lower.size.height, full.size.height);
        let next = block_bounds(
            origin + point(cell.width, px(0.)),
            cell,
            (0, 0, 8, 8),
            scale,
        );
        assert_eq!(full.origin.x + full.size.width, next.origin.x);
        for value in [
            full.origin.x,
            full.origin.y,
            full.size.width,
            full.size.height,
        ] {
            let physical = f32::from(value) * scale;
            assert!((physical - physical.round()).abs() < 0.0001);
        }
    }
}

#[test]
fn generic_symbols_center_by_ink_bounds() {
    for text in ["\u{f179}", "⚠", "★"] {
        assert!(centered_symbol(text).is_some());
    }
    for text in ["A", "a\u{301}", "─", "█", "⣿", "\u{e0b0}"] {
        assert!(centered_symbol(text).is_none());
    }
    let cell = size(px(10.), px(20.));
    for bottom in [px(-3.), px(0.), px(2.)] {
        let ink = Bounds::new(point(px(1.), bottom), size(px(6.), px(8.)));
        let offset = centered_origin(cell, ink, px(14.));
        let visible_top = offset.y + px(14.) - ink.origin.y - ink.size.height;
        let visible_left = offset.x + ink.origin.x;
        assert_eq!(visible_top, px(6.));
        assert_eq!(cell.height - visible_top - ink.size.height, px(6.));
        assert_eq!(visible_left, px(2.));
        assert_eq!(cell.width - visible_left - ink.size.width, px(2.));
    }
}

#[test]
fn glyph_overhang_is_not_font_specific() {
    for text in ["\u{f179}", "⚠", "➜", "A"] {
        assert_eq!(overhang_columns(&[cell(0, text), cell(1, " ")], 1), 1);
        assert_eq!(overhang_columns(&[cell(0, text), cell(1, "x")], 1), 0);
    }
}
fn cell(x: usize, text: &str) -> Cell {
    Cell {
        x,
        y: 0,
        text: text.into(),
        fg: rgb(0xffffff),
        bg: None,
        width: 1,
        bold: false,
        italic: false,
        blink: false,
        overline: false,
        underline: Underline::None,
        underline_color: rgb(0xffffff),
        strikethrough: false,
    }
}
#[test]
fn font_styles_and_blink_boundaries() {
    let font = gpui::font("monospace");
    for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut c = cell(0, "a");
        c.bold = bold;
        c.italic = italic;
        let styled = styled_font(&font, &c);
        assert_eq!(styled.weight == gpui::FontWeight::BOLD, bold);
        assert_eq!(styled.style == gpui::FontStyle::Italic, italic);
    }
    for millis in [0, 499, 1000, 1499] {
        assert!(blink_visible(Duration::from_millis(millis)));
    }
    for millis in [500, 999, 1500, 1999] {
        assert!(!blink_visible(Duration::from_millis(millis)));
    }
    for style in 0..3 {
        let mut cells = vec![cell(0, "a"), cell(1, "b"), cell(2, "c")];
        match style {
            0 => cells[1].bold = true,
            1 => cells[1].italic = true,
            _ => cells[1].blink = true,
        }
        assert_eq!(run_end(&cells, 0), 1);
        assert_eq!(run_end(&cells, 1), 2);
    }
}

#[test]
fn underline_patterns_cover_and_stay_within_the_cell() {
    assert!(underline_segments(Underline::None, px(7.), px(1.)).is_empty());
    assert_eq!(
        underline_segments(Underline::Single, px(7.), px(1.)),
        vec![(px(0.), px(7.), px(0.))]
    );
    assert_eq!(
        underline_segments(Underline::Double, px(7.), px(1.)).len(),
        2
    );
    for style in [Underline::Dotted, Underline::Dashed] {
        let segments = underline_segments(style, px(7.5), px(1.));
        assert_eq!(segments[0].0, px(0.));
        assert!(
            segments
                .iter()
                .all(|&(x, width, _)| width > px(0.) && x + width <= px(7.5))
        );
        assert!(segments.windows(2).all(|s| s[0].0 + s[0].1 < s[1].0));
    }
}
#[test]
fn merges_backgrounds_without_painting_over_gaps() {
    let mut cells: Vec<_> = (0..80).map(|x| cell(x, "x")).collect();
    for c in &mut cells {
        c.bg = Some(rgb(0x123456));
    }
    let mut output = vec![];
    backgrounds(&cells, &mut output);
    assert_eq!(output, vec![(0, 80, rgb(0x123456))]);
    assert_eq!(run_end(&cells, 0), 80);
    cells[1].bg = None;
    cells[2].bg = Some(rgb(0xff0000));
    backgrounds(&cells, &mut output);
    assert_eq!(
        output,
        vec![
            (0, 1, rgb(0x123456)),
            (2, 1, rgb(0xff0000)),
            (3, 77, rgb(0x123456))
        ]
    );
}

#[test]
fn batches_ascii_without_crossing_style_grid_or_complex_text() {
    let mut cells = vec![cell(0, "a"), cell(1, "b"), cell(2, "c")];
    assert_eq!(run_end(&cells, 0), 3);
    cells[2].fg = rgb(0xff0000);
    assert_eq!(run_end(&cells, 0), 2);
    cells[1] = cell(1, "界");
    assert_eq!(run_end(&cells, 0), 1);
    cells[1] = cell(1, "e\u{301}");
    assert_eq!(run_end(&cells, 1), 2);
    cells[1] = cell(2, "b");
    assert_eq!(run_end(&cells, 0), 1);
    cells[1] = cell(1, "b");
    cells[1].y = 1;
    assert_eq!(run_end(&cells, 0), 1);
}

#[test]
fn overhang_only_covers_contiguous_blank_single_width_cells() {
    let cells = vec![
        cell(0, "➜"),
        cell(1, " "),
        cell(2, " "),
        cell(3, "x"),
        cell(4, " "),
    ];
    assert_eq!(overhang_columns(&cells, 1), 2);
    assert_eq!(overhang_columns(&cells, 3), 0);
    assert_eq!(overhang_columns(&cells, 5), 0);
    assert_eq!(overhang_columns(&cells, 9), 0);

    let mut wide_tail = cell(1, "");
    wide_tail.width = 0;
    assert_eq!(
        overhang_columns(&[cell(0, "⚠"), wide_tail, cell(2, " ")], 1),
        0
    );

    let mut wide_head = cell(1, "😀");
    wide_head.width = 2;
    assert_eq!(overhang_columns(&[cell(0, "➜"), wide_head], 1), 0);

    let mut invisible_wide = cell(1, "");
    invisible_wide.width = 2;
    assert_eq!(
        overhang_columns(&[cell(0, "➜"), invisible_wide, cell(2, " ")], 1),
        0
    );

    let mut gap = cell(3, " ");
    gap.x = 3;
    assert_eq!(overhang_columns(&[cell(0, "➜"), gap], 1), 0);

    assert_eq!(overhang_columns(&[], 0), 0);
}
