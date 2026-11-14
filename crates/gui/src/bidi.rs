use crate::tab::Cell;
use anyhow::Result;
use ghostty_vt::{
    Terminal,
    screen::CellWide,
    terminal::{Point, PointCoordinate},
};
use std::ops::Range;
use unicode_bidi::{BidiInfo, Level};

#[derive(Default)]
pub(super) struct Layout {
    pub(super) cells: Vec<Cell>,
    to_visual: Vec<usize>,
    to_logical: Vec<usize>,
    cols: usize,
}
struct Line {
    bytes: Range<usize>,
    items: Vec<(usize, usize)>, // logical cell index and paragraph byte offset
}

impl Layout {
    pub(super) fn visual_x(&self, x: usize, y: usize) -> usize {
        if self.cols > 0 && x < self.cols && y < self.to_visual.len() / self.cols {
            self.to_visual[y * self.cols + x]
        } else {
            x
        }
    }
    pub(super) fn mouse_position(
        &self,
        position: ghostty_vt::mouse::Position,
        width: u32,
        height: u32,
    ) -> ghostty_vt::mouse::Position {
        let mut result = position;
        if self.cols > 0
            && position.x >= 0.
            && position.y >= 0.
            && position.x < (self.cols as f32 * width as f32)
            && position.y < ((self.to_logical.len() / self.cols) as f32 * height as f32)
        {
            let x = (position.x / width as f32) as usize;
            let y = (position.y / height as f32) as usize;
            result.x = self.to_logical[y * self.cols + x] as f32 * width as f32
                + position.x % width as f32;
        }
        result
    }
    pub(super) fn logical_point(&self, point: Point) -> Point {
        if let Point::Viewport(mut point) = point {
            if self.cols > 0
                && usize::from(point.x) < self.cols
                && (point.y as usize) < self.to_logical.len() / self.cols
            {
                point.x =
                    self.to_logical[point.y as usize * self.cols + usize::from(point.x)] as u16;
            }
            Point::Viewport(point)
        } else {
            point
        }
    }
    pub(super) fn update(&mut self, cells: &[Cell], terminal: &Terminal) -> Result<()> {
        self.cols = usize::from(terminal.cols()?);
        self.cells = cells.to_vec();
        self.to_visual = cells.iter().map(|cell| cell.x).collect();
        self.to_logical = self.to_visual.clone();
        if cells.is_empty() {
            return Ok(());
        }
        let top: u32 = terminal.scrollbar()?.offset.try_into()?;
        let bottom = top + u32::from(terminal.rows()?);
        let total: u32 = terminal.total_rows()?.try_into()?;
        // Resolve the entire soft-wrapped paragraph, including context outside the viewport.
        let mut start = top;
        while start > 0
            && terminal
                .grid_ref(Point::Screen(PointCoordinate { x: 0, y: start }))?
                .row()?
                .is_wrap_continuation()?
        {
            start -= 1;
        }
        let mut end = bottom.min(total);
        while end < total
            && terminal
                .grid_ref(Point::Screen(PointCoordinate { x: 0, y: end - 1 }))?
                .row()?
                .is_wrapped()?
        {
            end += 1;
        }
        let mut text = String::new();
        let mut lines = Vec::new();
        let mut chars = Vec::new();
        for y in start..end {
            let begin = text.len();
            let mut items = Vec::new();
            for x in 0..self.cols {
                let reference =
                    terminal.grid_ref(Point::Screen(PointCoordinate { x: x as u16, y }))?;
                if matches!(
                    reference.cell()?.wide()?,
                    CellWide::SpacerHead | CellWide::SpacerTail
                ) {
                    continue;
                }
                if y >= top && y < bottom {
                    items.push(((y - top) as usize * self.cols + x, text.len()));
                }
                text.extend(crate::unicode::cell_chars(&reference, &mut chars)?);
            }
            if y >= top && y < bottom {
                lines.push(Line {
                    bytes: begin..text.len(),
                    items,
                });
            }
            let wrapped = terminal
                .grid_ref(Point::Screen(PointCoordinate { x: 0, y }))?
                .row()?
                .is_wrapped()?;
            if !wrapped || y + 1 == end {
                self.paragraph(cells, &text, &lines);
                text.clear();
                lines.clear();
            }
        }
        // Paint order must follow physical columns, not the original logical indices.
        self.cells.sort_by_key(|cell| (cell.y, cell.x));
        Ok(())
    }
    fn paragraph(&mut self, cells: &[Cell], text: &str, lines: &[Line]) {
        // A terminal grid has a fixed left-to-right coordinate system; RTL runs reorder within it.
        let info = BidiInfo::new(text, Some(Level::ltr()));
        if !info.has_rtl() {
            return;
        }
        for line in lines {
            let mut x = 0;
            for paragraph in &info.paragraphs {
                let range = line.bytes.start.max(paragraph.range.start)
                    ..line.bytes.end.min(paragraph.range.end);
                if range.is_empty() {
                    continue;
                }
                let levels = info.reordered_levels(paragraph, range.clone());
                let items: Vec<_> = line
                    .items
                    .iter()
                    .filter(|(_, byte)| range.contains(byte))
                    .collect();
                let cell_levels: Vec<_> = items.iter().map(|(_, byte)| levels[*byte]).collect();
                for item in BidiInfo::reorder_visual(&cell_levels) {
                    let (index, _) = *items[item];
                    let cell = &cells[index];
                    let level = cell_levels[item];
                    let mut visual = cell.clone();
                    visual.x = x;
                    if level.is_rtl() {
                        visual.text = visual
                            .text
                            .chars()
                            .map(|ch| unicode_bidi_mirroring::get_mirrored(ch).unwrap_or(ch))
                            .collect();
                    }
                    visual.text.retain(|ch| !matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'));
                    self.cells[index] = visual;
                    self.to_visual[index] = x;
                    self.to_logical[cell.y * self.cols + x] = cell.x;
                    if cell.width == 2 && index + 1 < cells.len() {
                        self.cells[index + 1].x = x + 1;
                        self.to_visual[index + 1] = x + 1;
                        self.to_logical[cell.y * self.cols + x + 1] = cell.x + 1;
                    }
                    x += usize::from(cell.width);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/bidi.rs"]
mod tests;
