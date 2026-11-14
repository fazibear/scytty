use crate::search::Search;
use anyhow::Result;
use ghostty_vt::{Terminal, terminal::PointSpace};
use std::{
    ops::Range,
    time::{Duration, Instant},
};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Default)]
pub struct Find {
    pub query: String,
    pub selection: Range<usize>, // UTF-8 byte offsets
    pub marked: Option<Range<usize>>,
    pub reversed: bool,
    pub stats: (usize, Option<usize>, bool),
}

impl Find {
    pub fn replace(&mut self, range: Option<Range<usize>>, text: &str) -> Range<usize> {
        let range = range
            .map(|r| crate::unicode::byte_range(&self.query, r))
            .or_else(|| self.marked.take())
            .unwrap_or_else(|| self.selection.clone());
        // Keep pasted queries single-line and bound work/memory from external input.
        let budget = 4096usize.saturating_sub(self.query.len() - range.len());
        let mut replacement = String::new();
        for grapheme in text.graphemes(true) {
            if replacement.len() + grapheme.len() > budget {
                break;
            }
            replacement.extend(grapheme.chars().filter(|c| !c.is_control()));
        }
        self.query.replace_range(range.clone(), &replacement);
        let inserted = range.start..range.start + replacement.len();
        self.selection = inserted.end..inserted.end;
        self.reversed = false;
        self.marked = None;
        inserted
    }
    pub fn edit(&mut self, key: &str, shift: bool) -> bool {
        let caret = if self.reversed {
            self.selection.start
        } else {
            self.selection.end
        };
        let mut previous = 0;
        let mut next = self.query.len();
        let mut boundary = caret == self.query.len();
        for (index, _) in self.query.grapheme_indices(true) {
            if index < caret {
                previous = index;
            } else if index == caret {
                boundary = true;
            } else {
                next = index;
                break;
            }
        }
        match key {
            "backspace" | "delete" => {
                if self.selection.is_empty() {
                    self.selection = if key == "backspace" {
                        previous..if boundary { caret } else { next }
                    } else {
                        (if boundary { caret } else { previous })..next
                    };
                }
                self.replace(None, "");
            }
            "left" | "right" | "home" | "end" => {
                let position = match key {
                    "home" => 0,
                    "end" => self.query.len(),
                    "left" if !shift && !self.selection.is_empty() => self.selection.start,
                    "right" if !shift && !self.selection.is_empty() => self.selection.end,
                    "left" => previous,
                    _ => next,
                };
                let anchor = if self.reversed {
                    self.selection.end
                } else {
                    self.selection.start
                };
                self.selection = if shift {
                    anchor.min(position)..anchor.max(position)
                } else {
                    position..position
                };
                self.reversed = shift && position < anchor;
                self.marked = None;
            }
            _ => return false,
        }
        true
    }
}

pub fn advance(search: &mut Search, terminal: &Terminal, query: &str) -> Result<bool> {
    search.set_needle(terminal, query)?;
    let deadline = Instant::now() + Duration::from_millis(2);
    loop {
        if !search.tick(terminal)? {
            return Ok(false);
        }
        if Instant::now() >= deadline {
            return Ok(true);
        }
    }
}

pub struct Highlight {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub selected: bool,
}

pub fn highlights(search: &Search, terminal: &Terminal) -> Result<Vec<Highlight>> {
    let offset = terminal.scrollbar()?.offset;
    let cols = usize::from(terminal.cols()?);
    let rows = u64::from(terminal.rows()?);
    let selected = search.selected_match(terminal)?;
    let mut output = Vec::new();
    for selection in search.viewport_matches(terminal)? {
        let Some(mut start) =
            terminal.point_from_grid_ref(&selection.start(), PointSpace::Screen)?
        else {
            continue;
        };
        let Some(mut end) = terminal.point_from_grid_ref(&selection.end(), PointSpace::Screen)?
        else {
            continue;
        };
        if (start.y, start.x) > (end.y, end.x) {
            std::mem::swap(&mut start, &mut end);
        }
        let end_width = if terminal
            .grid_ref(ghostty_vt::terminal::Point::Screen(end))?
            .cell()?
            .wide()?
            == ghostty_vt::screen::CellWide::Wide
        {
            2
        } else {
            1
        };
        let selected = selected
            .as_ref()
            .map(|s| s.equals(terminal, &selection))
            .transpose()?
            .unwrap_or(false);
        for y in u64::from(start.y).max(offset)..=u64::from(end.y).min(offset + rows - 1) {
            let x = if y == u64::from(start.y) {
                usize::from(start.x)
            } else {
                0
            };
            let right = if y == u64::from(end.y) {
                usize::from(end.x) + end_width
            } else {
                cols
            };
            if right > x {
                output.push(Highlight {
                    x,
                    y: (y - offset) as usize,
                    width: right.min(cols).saturating_sub(x),
                    selected,
                });
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
#[path = "../tests/unit/find.rs"]
mod tests;
