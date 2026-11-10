use anyhow::Result;
use ghostty_vt::{
    Terminal,
    search::{Search, Status},
    terminal::PointSpace,
};
use std::{
    ops::Range,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Find {
    pub query: String,
    pub selection: Range<usize>, // UTF-8 byte offsets
    pub marked: Option<Range<usize>>,
    pub reversed: bool,
    pub stats: (usize, Option<usize>, bool),
}

impl Find {
    pub fn utf16_range(&self, range: Range<usize>) -> Range<usize> {
        self.query[..range.start].encode_utf16().count()
            ..self.query[..range.end].encode_utf16().count()
    }
    pub fn byte_range(&self, range: Range<usize>) -> Range<usize> {
        fn index(text: &str, offset: usize) -> usize {
            let mut units = 0;
            for (i, c) in text.char_indices() {
                if units + c.len_utf16() > offset {
                    return i;
                }
                units += c.len_utf16();
            }
            text.len()
        }
        let start = index(&self.query, range.start);
        start..index(&self.query, range.end).max(start)
    }
    pub fn replace(&mut self, range: Option<Range<usize>>, text: &str) -> Range<usize> {
        let range = range
            .map(|r| self.byte_range(r))
            .or_else(|| self.marked.take())
            .unwrap_or_else(|| self.selection.clone());
        // Keep pasted queries single-line and bound work/memory from external input.
        let budget = 4096usize.saturating_sub(self.query.len() - range.len());
        let mut replacement = String::new();
        for c in text.chars().filter(|c| !c.is_control()) {
            if replacement.len() + c.len_utf8() > budget {
                break;
            }
            replacement.push(c);
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
        let previous = self.query[..caret]
            .char_indices()
            .last()
            .map_or(0, |(i, _)| i);
        let next = self.query[caret..]
            .chars()
            .next()
            .map_or(self.query.len(), |c| caret + c.len_utf8());
        match key {
            "backspace" | "delete" => {
                if self.selection.is_empty() {
                    self.selection = if key == "backspace" {
                        previous..self.selection.end
                    } else {
                        self.selection.start..next
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
    search.set_needle(terminal, Some(query))?;
    search.feed(terminal)?;
    let deadline = Instant::now() + Duration::from_millis(2);
    loop {
        match search.tick()? {
            Status::Complete => return Ok(false),
            Status::FeedRequired => search.feed(terminal)?,
            Status::Running => {}
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
mod tests {
    use super::*;
    use ghostty_vt::{TerminalOptions, terminal::ScrollViewport};
    #[test]
    fn edits_unicode_and_composition_without_splitting_characters() {
        let mut find = Find::default();
        find.replace(None, "a😀é");
        assert_eq!(find.utf16_range(find.selection.clone()), 4..4);
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
    fn highlights_wrapped_and_wide_matches() -> Result<()> {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 5,
            rows: 3,
            ..Default::default()
        })?;
        terminal.vt_write("abcdneedle\r\n界".as_bytes());
        let mut search = Search::new(&terminal)?;
        while advance(&mut search, &terminal, "needle")? {}
        assert_eq!(search.total_matches()?, 1);
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
        assert_eq!(search.total_matches()?, 0);
        assert!(highlights(&search, &terminal)?.is_empty());
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
        assert_eq!(search.total_matches()?, 2);
        assert!(search.select_next(&terminal)?);
        search.feed(&terminal)?;
        assert!(!highlights(&search, &terminal)?.is_empty());
        assert!(search.select_next(&terminal)?);
        search.feed(&terminal)?;
        assert!(highlights(&search, &terminal)?.iter().any(|h| h.selected));
        assert!(search.select_prev(&terminal)?);
        assert!(terminal.selection()?.is_none());
        terminal.scroll_viewport(ScrollViewport::Bottom);
        terminal.vt_write(b"\r\nNEEDLE");
        while advance(&mut search, &terminal, "needle")? {}
        assert_eq!(search.total_matches()?, 3);
        while advance(&mut search, &terminal, "missing")? {}
        assert_eq!(search.total_matches()?, 0);
        Ok(())
    }
}
