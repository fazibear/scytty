use crate::{Cell, expand_nerd_icon};
use ghostty_vt::style::Underline;
use gpui::{
    App, Bounds, ContentMask, Font, Pixels, Point, Rgba, ShapedLine, Size, TextRun, Window, fill,
    point, px, size,
};

#[derive(Default)]
pub struct Cache {
    key: Option<(Font, Pixels, Size<Pixels>, f32)>,
    rows: Vec<Row>,
}

#[derive(Default)]
struct Row {
    cells: Vec<Cell>,
    text: Vec<Text>,
    backgrounds: Vec<(usize, usize, Rgba)>,
}
struct Text {
    x: usize,
    line: ShapedLine,
    expanded: bool,
}

fn ascii(cell: &Cell) -> bool {
    cell.narrow && cell.text.len() == 1 && cell.text.as_bytes()[0].is_ascii_graphic()
}

// Only batch simple single-cell text; keep complex glyphs on the existing path.
fn run_end(cells: &[Cell], start: usize) -> usize {
    let first = &cells[start];
    if !ascii(first) {
        return start + 1;
    }
    let mut end = start + 1;
    while let Some(next) = cells.get(end) {
        if !ascii(next)
            || next.fg != first.fg
            || next.y != first.y
            || next.x != cells[end - 1].x + 1
        {
            break;
        }
        end += 1;
    }
    end
}

fn backgrounds(cells: &[Cell], output: &mut Vec<(usize, usize, Rgba)>) {
    output.clear();
    for c in cells {
        if let Some(bg) = c.bg {
            if let Some((x, width, color)) = output.last_mut()
                && *color == bg
                && *x + *width == c.x
            {
                *width += 1;
            } else {
                output.push((c.x, 1, bg));
            }
        }
    }
}

// Rectangles are (horizontal offset, width, vertical offset from the bottom line).
fn underline_segments(
    style: Underline,
    width: Pixels,
    thickness: Pixels,
) -> Vec<(Pixels, Pixels, Pixels)> {
    match style {
        Underline::None | Underline::Curly => vec![],
        Underline::Double => vec![(px(0.), width, px(0.)), (px(0.), width, thickness * -2.)],
        Underline::Dotted | Underline::Dashed => {
            let length = thickness * if style == Underline::Dotted { 1. } else { 3. };
            let mut segments = Vec::new();
            let mut x = px(0.);
            while x < width {
                segments.push((x, length.min(width - x), px(0.)));
                x += length + thickness;
            }
            segments
        }
        _ => vec![(px(0.), width, px(0.))],
    }
}

impl Cache {
    pub fn prepare(
        &mut self,
        cells: &[Cell],
        font: &Font,
        font_size: Pixels,
        cell: Size<Pixels>,
        icon_scale: f32,
        window: &mut Window,
    ) {
        let key = (font.clone(), font_size, cell, icon_scale);
        if self.key.as_ref() != Some(&key) {
            self.rows.clear();
            self.key = Some(key);
        }
        let row_count = cells.last().map_or(0, |c| c.y + 1);
        self.rows.resize_with(row_count, Row::default);
        self.rows.truncate(row_count);
        let mut remaining = cells;
        for (y, row) in self.rows.iter_mut().enumerate() {
            let end = remaining.partition_point(|c| c.y == y);
            let (source, rest) = remaining.split_at(end);
            remaining = rest;
            if row.cells == source {
                continue;
            }
            row.cells.clear();
            row.cells.extend_from_slice(source);
            row.text.clear();
            backgrounds(source, &mut row.backgrounds);
            let mut i = 0;
            while i < source.len() {
                let c = &source[i];
                if c.text.is_empty() || c.text == " " {
                    i += 1;
                    continue;
                }
                let end = run_end(source, i);
                let expanded = c.narrow
                    && source.get(i + 1).is_some_and(|next| {
                        next.narrow && next.x == c.x + 1 && expand_nerd_icon(&c.text, &next.text)
                    });
                let scale = if expanded { icon_scale } else { 1. };
                let text: String = source[i..end].iter().map(|c| c.text.as_str()).collect();
                let run = TextRun {
                    len: text.len(),
                    font: font.clone(),
                    color: c.fg.into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let line = window.text_system().shape_line(
                    text.into(),
                    font_size * scale,
                    &[run],
                    (end > i + 1).then_some(cell.width),
                );
                // Ligatures or fallback fonts must not move terminal cell boundaries.
                if end > i + 1
                    && (1..line.text.len()).any(|index| {
                        (line.x_for_index(index) - cell.width * index as f32).abs() > px(0.01)
                    })
                {
                    for c in &source[i..end] {
                        let run = TextRun {
                            len: c.text.len(),
                            font: font.clone(),
                            color: c.fg.into(),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let line = window.text_system().shape_line(
                            c.text.clone().into(),
                            font_size,
                            &[run],
                            None,
                        );
                        row.text.push(Text {
                            x: c.x,
                            line,
                            expanded: false,
                        });
                    }
                    i = end;
                    continue;
                }
                row.text.push(Text {
                    x: c.x,
                    line,
                    expanded,
                });
                i = end;
            }
        }
    }

    pub fn backgrounds(&self, origin: Point<Pixels>, cell: Size<Pixels>, window: &mut Window) {
        for (y, row) in self.rows.iter().enumerate() {
            for &(x, width, color) in &row.backgrounds {
                window.paint_quad(fill(
                    Bounds::new(
                        origin + point(cell.width * x as f32, cell.height * y as f32),
                        size(cell.width * width as f32, cell.height),
                    ),
                    color,
                ));
            }
        }
    }

    pub fn text(
        &self,
        origin: Point<Pixels>,
        cell: Size<Pixels>,
        font_size: Pixels,
        icon_scale: f32,
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<()> {
        for (y, row) in self.rows.iter().enumerate() {
            for text in &row.text {
                let origin = origin + point(cell.width * text.x as f32, cell.height * y as f32);
                if text.expanded {
                    let bounds = Bounds::new(origin, size(cell.width * 2., cell.height));
                    let origin = origin
                        + point(
                            (cell.width * 2. - text.line.width) / 2.,
                            (cell.height - font_size * icon_scale) / 2.,
                        );
                    window.with_content_mask(Some(ContentMask { bounds }), |window| {
                        text.line.paint(origin, cell.height, window, cx)
                    })?;
                } else {
                    text.line.paint(origin, cell.height, window, cx)?;
                }
            }
            // Decorate the grid, not glyph widths: spaces and wide-cell padding count too.
            let thickness = px((f32::from(font_size) * window.scale_factor() / 14.)
                .round()
                .max(1.)
                / window.scale_factor());
            for c in &row.cells {
                let origin = origin + point(cell.width * c.x as f32, cell.height * y as f32);
                let underline_y = cell.height - thickness * 2.;
                if c.underline == Underline::Curly {
                    window.paint_underline(
                        origin + point(px(0.), cell.height - thickness * 4.),
                        cell.width,
                        &gpui::UnderlineStyle {
                            thickness,
                            color: Some(c.underline_color.into()),
                            wavy: true,
                        },
                    );
                } else if c.underline != Underline::None {
                    for (x, width, y) in underline_segments(c.underline, cell.width, thickness) {
                        window.paint_quad(fill(
                            Bounds::new(origin + point(x, underline_y + y), size(width, thickness)),
                            c.underline_color,
                        ));
                    }
                }
                if c.strikethrough {
                    window.paint_quad(fill(
                        Bounds::new(
                            origin + point(px(0.), cell.height * 0.55),
                            size(cell.width, thickness),
                        ),
                        c.fg,
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
}
