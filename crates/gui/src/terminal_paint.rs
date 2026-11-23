use crate::{Cell, expand_nerd_icon};
use ghostty_vt::style::Underline;
use gpui::{
    App, Bounds, ContentMask, Font, Pixels, Point, Rgba, ShapedLine, Size, TextRun, Window, fill,
    point, px, size,
};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Cache {
    key: Option<(Font, Pixels, Size<Pixels>, f32)>,
    rows: Vec<Row>,
    blink_epoch: Option<Instant>,
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
    columns: usize,
    blink: bool,
    fit: f32,
}

pub(super) fn blink_visible(elapsed: Duration) -> bool {
    elapsed.as_millis() % 1000 < 500
}

pub(super) fn styled_font(font: &Font, cell: &Cell) -> Font {
    let mut font = font.clone();
    if cell.bold {
        font = font.bold();
    }
    if cell.italic {
        font = font.italic();
    }
    font
}

fn ascii(cell: &Cell) -> bool {
    cell.width == 1 && cell.text.len() == 1 && cell.text.as_bytes()[0].is_ascii_graphic()
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
            || next.bold != first.bold
            || next.italic != first.italic
            || next.blink != first.blink
            || next.y != first.y
            || next.x != cells[end - 1].x + 1
        {
            break;
        }
        end += 1;
    }
    end
}

// Blank single-width cells a wider fallback glyph may overpaint; wide-cell spacers stop it.
fn overhang_columns(cells: &[Cell], x: usize) -> usize {
    let mut columns = 0;
    for c in cells.iter().skip_while(|c| c.x < x) {
        if c.x != x + columns
            || c.width != 1
            || !(c.text.is_empty() || c.text == " ")
        {
            break;
        }
        columns += 1;
    }
    columns
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
        self.blink_epoch.get_or_insert_with(Instant::now);
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
                let expanded = c.width == 1
                    && source.get(i + 1).is_some_and(|next| {
                        next.width == 1
                            && next.x == c.x + 1
                            && expand_nerd_icon(&c.text, &next.text)
                    });
                let scale = if expanded { icon_scale } else { 1. };
                let text: String = source[i..end].iter().map(|c| c.text.as_str()).collect();
                let run = TextRun {
                    len: text.len(),
                    font: styled_font(font, c),
                    color: c.fg.into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let line = window.text_system().shape_line(
                    text.into(),
                    font_size * scale,
                    std::array::from_ref(&run),
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
                            font: styled_font(font, c),
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
                            columns: usize::from(c.width),
                            blink: c.blink,
                            fit: 1.,
                        });
                    }
                    i = end;
                    continue;
                }
                let columns = if expanded {
                    2
                } else {
                    source[i..end].iter().map(|c| usize::from(c.width)).sum()
                };
                let mut line = line;
                let mut fit = 1.;
                if !expanded && line.width > cell.width * columns as f32 + px(0.01) {
                    let available = cell.width
                        * (columns + overhang_columns(source, c.x + columns)) as f32;
                    // Blank neighbors absorb the overflow; otherwise shrink to fit.
                    if line.width > available + px(0.01) {
                        fit = f32::from(available) / f32::from(line.width);
                        let text: String =
                            source[i..end].iter().map(|c| c.text.as_str()).collect();
                        line = window.text_system().shape_line(
                            text.into(),
                            font_size * fit,
                            &[run],
                            None,
                        );
                    }
                }
                row.text.push(Text {
                    x: c.x,
                    line,
                    expanded,
                    columns,
                    blink: c.blink,
                    fit,
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
        let show_blink = self
            .blink_epoch
            .is_none_or(|epoch| blink_visible(epoch.elapsed()));
        for (y, row) in self.rows.iter().enumerate() {
            for text in &row.text {
                if text.blink && !show_blink {
                    continue;
                }
                let origin = origin + point(cell.width * text.x as f32, cell.height * y as f32);
                let mut bounds =
                    Bounds::new(origin, size(cell.width * text.columns as f32, cell.height));
                if text.expanded {
                    let origin = origin
                        + point(
                            (cell.width * 2. - text.line.width) / 2.,
                            (cell.height - font_size * icon_scale) / 2.,
                        );
                    window.with_content_mask(Some(ContentMask { bounds }), |window| {
                        text.line.paint(origin, cell.height, window, cx)
                    })?;
                } else {
                    // Fallback fonts can be wider than the configured monospace
                    // cell; overpaint blank neighbors instead of clipping glyphs.
                    let overflow = text.line.width - bounds.size.width;
                    if overflow > px(0.01) {
                        let allowance = cell.width
                            * overhang_columns(&row.cells, text.x + text.columns) as f32;
                        bounds.size.width += overflow.min(allowance);
                    }
                    let origin = if text.fit != 1. {
                        origin + point(px(0.), (cell.height - font_size * text.fit) / 2.)
                    } else {
                        origin
                    };
                    window.with_content_mask(Some(ContentMask { bounds }), |window| {
                        text.line.paint(origin, cell.height, window, cx)
                    })?;
                }
            }
            // Decorate the grid, not glyph widths: spaces and wide-cell padding count too.
            let thickness = px((f32::from(font_size) * window.scale_factor() / 14.)
                .round()
                .max(1.)
                / window.scale_factor());
            for c in &row.cells {
                if c.blink && !show_blink {
                    continue;
                }
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
                if c.overline {
                    window.paint_quad(fill(Bounds::new(origin, size(cell.width, thickness)), c.fg));
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
#[path = "../tests/unit/terminal_paint.rs"]
mod tests;
