use anyhow::Result;
use caseless::Caseless;
use ghostty_vt::{
    Terminal,
    screen::CellWide,
    selection::Selection,
    terminal::{Point, PointCoordinate, ScrollViewport},
};

#[derive(Clone, Copy, PartialEq, Eq)]
struct Match {
    start: PointCoordinate,
    end: PointCoordinate,
}
#[derive(Clone, Copy)]
struct Source {
    point: PointCoordinate,
    start: bool,
    end: bool,
}

#[derive(Default)]
pub(super) struct Search {
    needle: String,
    folded: String,
    matches: Vec<Match>,
    selected: Option<usize>,
    row: u32,
    emit_from: u32,
    cols: u16,
    rows: u32,
    dirty: bool,
    text: String,
    source: Vec<Source>,
    chars: Vec<char>,
    history: u32,
    first_history: Option<ghostty_vt::screen::TrackedGridRef>,
    screen: Option<ghostty_vt::screen::Screen>,
    wanted: Option<Match>,
    anchor: Option<(
        ghostty_vt::screen::TrackedGridRef,
        ghostty_vt::screen::TrackedGridRef,
    )>,
}

impl Search {
    pub(super) fn new(terminal: &Terminal) -> Result<Self> {
        let mut search = Self::default();
        search.feed(terminal)?;
        Ok(search)
    }
    pub(super) fn set_needle(&mut self, terminal: &Terminal, needle: &str) -> Result<()> {
        if self.needle != needle {
            self.dirty = true;
            self.wanted = None;
            self.anchor = None;
            self.needle = needle.into();
            self.folded = needle.chars().default_case_fold().collect();
            self.feed(terminal)?;
        }
        Ok(())
    }
    pub(super) fn feed(&mut self, terminal: &Terminal) -> Result<()> {
        let cols = terminal.cols()?;
        let screen = terminal.active_screen()?;
        if self.screen != Some(screen) {
            self.anchor = None;
        }
        self.wanted = if let Some((start, end)) = &self.anchor {
            start
                .point(ghostty_vt::terminal::PointSpace::Screen)?
                .zip(end.point(ghostty_vt::terminal::PointSpace::Screen)?)
                .map(|(start, end)| Match { start, end })
        } else {
            None
        };
        let history: u32 = terminal.scrollback_rows()?.try_into()?;
        let stable_history = self
            .first_history
            .as_ref()
            .map(|first| first.point(ghostty_vt::terminal::PointSpace::Screen))
            .transpose()?
            .flatten()
            .is_some_and(|point| point.y == 0);
        if !self.dirty && cols == self.cols && self.screen == Some(screen) && stable_history {
            // Output cannot rewrite history: keep that progress instead of starving a live search.
            let invalidate = self.row.min(self.history.min(history));
            let mut restart = invalidate;
            if restart < self.row {
                let mut context = 0;
                while restart > 0
                    && context < self.folded.len()
                    && terminal
                        .grid_ref(Point::Screen(PointCoordinate { x: 0, y: restart }))?
                        .row()?
                        .is_wrap_continuation()?
                {
                    restart -= 1;
                    for x in 0..cols {
                        if context >= self.folded.len() {
                            break;
                        }
                        let reference =
                            terminal.grid_ref(Point::Screen(PointCoordinate { x, y: restart }))?;
                        if matches!(
                            reference.cell()?.wide()?,
                            CellWide::SpacerHead | CellWide::SpacerTail
                        ) {
                            continue;
                        }
                        context += crate::unicode::cell_chars(&reference, &mut self.chars)?
                            .iter()
                            .copied()
                            .default_case_fold()
                            .map(char::len_utf8)
                            .sum::<usize>();
                    }
                }
                self.matches.retain(|result| result.end.y < invalidate);
                self.emit_from = invalidate;
                if self
                    .selected
                    .is_some_and(|index| index >= self.matches.len())
                {
                    self.selected = None;
                }
                self.row = restart;
                self.text.clear();
                self.source.clear();
            }
        } else {
            self.dirty = true;
        }
        self.cols = cols;
        self.rows = terminal.total_rows()?.try_into()?;
        self.history = history;
        self.first_history = if history > 0 {
            Some(terminal.track_grid_ref(Point::Screen(PointCoordinate { x: 0, y: 0 }))?)
        } else {
            None
        };
        self.screen = Some(screen);
        Ok(())
    }
    // One physical row per tick; the caller yields between ticks to keep Find responsive.
    pub(super) fn tick(&mut self, terminal: &Terminal) -> Result<bool> {
        if self.dirty {
            self.row = 0;
            self.emit_from = 0;
            self.matches.clear();
            self.selected = None;
            self.text.clear();
            self.source.clear();
            self.dirty = false;
        }
        if self.folded.is_empty() || self.row >= self.rows {
            return Ok(false);
        }
        let carry = self.text.len();
        let wrapped = terminal
            .grid_ref(Point::Screen(PointCoordinate { x: 0, y: self.row }))?
            .row()?
            .is_wrapped()?;
        let mut used = self.cols;
        while !wrapped && used > 0 {
            let cell = terminal
                .grid_ref(Point::Screen(PointCoordinate {
                    x: used - 1,
                    y: self.row,
                }))?
                .cell()?;
            if cell.has_text()?
                && !matches!(cell.wide()?, CellWide::SpacerHead | CellWide::SpacerTail)
            {
                break;
            }
            used -= 1;
        }
        for x in 0..used {
            let point = PointCoordinate { x, y: self.row };
            let reference = terminal.grid_ref(Point::Screen(point))?;
            if matches!(
                reference.cell()?.wide()?,
                CellWide::SpacerHead | CellWide::SpacerTail
            ) {
                continue;
            }
            for &ch in crate::unicode::cell_chars(&reference, &mut self.chars)? {
                let start = self.text.len();
                for folded in std::iter::once(ch).default_case_fold() {
                    self.text.push(folded);
                    self.source.extend(std::iter::repeat_n(
                        Source {
                            point,
                            start: false,
                            end: false,
                        },
                        folded.len_utf8(),
                    ));
                }
                if self.text.len() > start {
                    self.source[start].start = true;
                    self.source.last_mut().unwrap().end = true;
                }
            }
        }
        if !wrapped {
            let length = carry + self.text[carry..].trim_end_matches(' ').len();
            self.text.truncate(length);
            self.source.truncate(length);
        }
        let mut offset = 0;
        while let Some(found) = self.text[offset..].find(&self.folded) {
            let start = offset + found;
            let end = start + self.folded.len();
            if end > carry && self.source[start].start && self.source[end - 1].end {
                let result = Match {
                    start: self.source[start].point,
                    end: self.source[end - 1].point,
                };
                if result.end.y >= self.emit_from && self.matches.last() != Some(&result) {
                    if self.wanted == Some(result) {
                        self.selected = Some(self.matches.len());
                    }
                    self.matches.push(result);
                }
            }
            offset = start + self.text[start..].chars().next().unwrap().len_utf8();
        }
        if wrapped {
            let mut discard = self.text.len().saturating_sub(self.folded.len() - 1);
            while !self.text.is_char_boundary(discard) {
                discard += 1;
            }
            self.text.drain(..discard);
            self.source.drain(..discard);
        } else {
            self.text.clear();
            self.source.clear();
        }
        self.row += 1;
        Ok(self.row < self.rows)
    }
    pub(super) fn total_matches(&self) -> usize {
        self.matches.len()
    }
    pub(super) fn selected_index(&self) -> Option<usize> {
        self.selected
    }
    fn selection<'t>(result: Match, terminal: &'t Terminal) -> Result<Selection<'t>> {
        Ok(Selection::new(
            terminal.grid_ref(Point::Screen(result.start))?,
            terminal.grid_ref(Point::Screen(result.end))?,
            false,
        ))
    }
    pub(super) fn selected_match<'t>(
        &self,
        terminal: &'t Terminal,
    ) -> Result<Option<Selection<'t>>> {
        self.selected
            .and_then(|i| self.matches.get(i))
            .map(|result| Self::selection(*result, terminal))
            .transpose()
    }
    pub(super) fn viewport_matches<'t>(
        &self,
        terminal: &'t Terminal,
    ) -> Result<Vec<Selection<'t>>> {
        let offset = terminal.scrollbar()?.offset;
        let bottom = offset + u64::from(terminal.rows()?);
        self.matches
            .iter()
            .filter(|result| {
                u64::from(result.end.y) >= offset && u64::from(result.start.y) < bottom
            })
            .map(|result| Self::selection(*result, terminal))
            .collect()
    }
    fn select(&mut self, terminal: &mut Terminal, backwards: bool) -> Result<bool> {
        if self.matches.is_empty() {
            return Ok(false);
        }
        let count = self.matches.len();
        let index = match (self.selected, backwards) {
            (None, false) => 0,
            (None, true) => count - 1,
            (Some(index), false) => (index + 1) % count,
            (Some(index), true) => (index + count - 1) % count,
        };
        self.selected = Some(index);
        self.wanted = None;
        self.anchor = Some((
            terminal.track_grid_ref(Point::Screen(self.matches[index].start))?,
            terminal.track_grid_ref(Point::Screen(self.matches[index].end))?,
        ));
        let y = self.matches[index].start.y as usize;
        let scrollbar = terminal.scrollbar()?;
        if (y as u64) < scrollbar.offset
            || y as u64 >= scrollbar.offset + u64::from(terminal.rows()?)
        {
            terminal.scroll_viewport(ScrollViewport::Row(
                y.saturating_sub(usize::from(terminal.rows()?) / 2),
            ));
        }
        Ok(true)
    }
    pub(super) fn select_next(&mut self, terminal: &mut Terminal) -> Result<bool> {
        self.select(terminal, false)
    }
    pub(super) fn select_prev(&mut self, terminal: &mut Terminal) -> Result<bool> {
        self.select(terminal, true)
    }
}

#[cfg(test)]
#[path = "../tests/unit/search.rs"]
mod tests;
