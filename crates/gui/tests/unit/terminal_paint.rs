use super::*;
use gpui::rgb;
fn cell(x: usize, text: &str) -> Cell {
    Cell {
        x,
        y: 0,
        text: text.into(),
        fg: rgb(0xffffff),
        bg: None,
        narrow: true,
        underline: Underline::None,
        underline_color: rgb(0xffffff),
        strikethrough: false,
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
