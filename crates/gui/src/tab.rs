use crate::{images, terminal_paint};
use anyhow::Result;
use ghostty_vt::{
    RenderState, Terminal, key,
    render::{CellIterator, RowIterator},
    screen::TrackedGridRef,
    selection::FormatOptions,
    terminal::ScrollViewport,
};
use gpui::{Bounds, Pixels, Rgba, Size, rgb};
use std::time::{Duration, Instant};

#[derive(Clone, PartialEq)]
pub(super) struct Cell {
    pub(super) x: usize,
    pub(super) y: usize,
    pub(super) text: String,
    pub(super) fg: Rgba,
    pub(super) bg: Option<Rgba>,
    pub(super) narrow: bool,
    pub(super) underline: ghostty_vt::style::Underline,
    pub(super) underline_color: Rgba,
    pub(super) strikethrough: bool,
}
pub(super) struct Tab {
    pub(super) terminal: Terminal,
    pub(super) render: RenderState,
    pub(super) images: images::Images,
    pub(super) placements: Vec<images::Placement>,
    pub(super) cells: Vec<Cell>,
    pub(super) paint_cache: terminal_paint::Cache,
    pub(super) search: Option<ghostty_vt::search::Search>,
    pub(super) cursor: Option<(u16, u16)>,
    pub(super) cursor_color: Rgba,
    pub(super) cursor_state: Option<ghostty_vt::render::Cursor>,
    pub(super) cursor_since: Instant,
    pub(super) default_colors: Option<(ghostty_vt::style::RgbColor, ghostty_vt::style::RgbColor)>,
    pub(super) hold_since: Option<Instant>,
    pub(super) encoder: key::Encoder,
    pub(super) size: (u16, u16, u32, u32),
    pub(super) resize_target: Option<((u16, u16, u32, u32), Instant)>,
    pub(super) resize_pending: bool,
    pub(super) anchor: Option<TrackedGridRef>,
    pub(super) exited: bool,
}
impl Tab {
    pub(super) fn new(mut terminal: Terminal) -> Result<Self> {
        scytty_common::images::configure(&mut terminal)?;
        scytty_common::clipboard::configure(&mut terminal)?;
        Ok(Self {
            terminal,
            render: RenderState::new()?,
            images: Default::default(),
            placements: Vec::new(),
            cells: vec![],
            paint_cache: Default::default(),
            search: None,
            cursor: None,
            cursor_color: rgb(0xffffff),
            cursor_state: None,
            cursor_since: Instant::now(),
            default_colors: None,
            hold_since: None,
            encoder: key::Encoder::new()?,
            size: (0, 0, 0, 0),
            resize_target: None,
            resize_pending: false,
            anchor: None,
            exited: false,
        })
    }
    pub(super) fn scroll(
        &mut self,
        rows: isize,
        position: ghostty_vt::mouse::Position,
        cell: Size<Pixels>,
        mods: key::Mods,
    ) -> Result<Vec<u8>> {
        use ghostty_vt::{mouse, screen::Screen, terminal::Mode};
        let mut bytes = Vec::new();
        if rows == 0 {
            return Ok(bytes);
        }
        if !mods.contains(key::Mods::SHIFT) && self.terminal.is_mouse_tracking()? {
            let mut encoder = mouse::Encoder::new()?;
            encoder
                .set_options_from_terminal(&self.terminal)
                .set_size(mouse::EncoderSize {
                    screen_width: u32::from(self.size.0) * (f32::from(cell.width) as u32).max(1),
                    screen_height: u32::from(self.size.1) * (f32::from(cell.height) as u32).max(1),
                    cell_width: (f32::from(cell.width) as u32).max(1),
                    cell_height: (f32::from(cell.height) as u32).max(1),
                    padding_top: 0,
                    padding_bottom: 0,
                    padding_left: 0,
                    padding_right: 0,
                });
            let mut event = mouse::Event::new()?;
            event
                .set_action(mouse::Action::Press)
                .set_button(Some(if rows < 0 {
                    mouse::Button::Four
                } else {
                    mouse::Button::Five
                }))
                .set_position(position)
                .set_mods(mods);
            for _ in 0..rows.unsigned_abs() {
                encoder.encode_to_vec(&event, &mut bytes)?;
            }
        } else if !mods.contains(key::Mods::SHIFT)
            && self.terminal.active_screen()? == Screen::Alternate
            && self.terminal.mode(Mode::ALT_SCROLL)?
        {
            self.encoder.set_options_from_terminal(&self.terminal);
            let mut event = key::Event::new()?;
            event.set_key(if rows < 0 {
                key::Key::ArrowUp
            } else {
                key::Key::ArrowDown
            });
            for _ in 0..rows.unsigned_abs() {
                self.encoder.encode_to_vec(&event, &mut bytes)?;
            }
        } else {
            self.terminal.scroll_viewport(ScrollViewport::Delta(rows));
        }
        Ok(bytes)
    }

    pub(super) fn request_resize(
        &mut self,
        size: (u16, u16, u32, u32),
        now: Instant,
        connected: bool,
    ) -> bool {
        if !connected {
            return false;
        }
        if self.resize_target.is_none_or(|(target, _)| target != size) {
            self.resize_target = Some((size, now));
        }
        if self.resize_pending || size == self.size {
            return false;
        }
        // Let live resizing settle before reflowing the shell's screen again.
        if self.size != (0, 0, 0, 0)
            && now.duration_since(self.resize_target.unwrap().1) < Duration::from_millis(75)
        {
            return false;
        }
        self.resize_pending = true;
        true
    }
    pub(super) fn background(&mut self) -> Result<Rgba> {
        let background = if hold_frame(&self.terminal, &mut self.hold_since)? {
            self.default_colors.map(|(_, background)| background)
        } else {
            self.terminal.bg_color()?
        };
        Ok(color(background, rgb(0)))
    }
    pub(super) fn prepare_frame(
        &mut self,
        bounds: Bounds<Pixels>,
        cell: Size<Pixels>,
    ) -> Result<()> {
        if self.snapshot()? {
            self.placements = self.images.prepare(&self.terminal, bounds, cell)?;
        }
        Ok(())
    }
    // Images must commit with text, even when only the image placements changed.
    pub(super) fn snapshot(&mut self) -> Result<bool> {
        if hold_frame(&self.terminal, &mut self.hold_since)? {
            return Ok(false);
        }
        let snapshot = self.render.update(&self.terminal)?;
        let cursor = snapshot.cursor()?;
        if self.cursor_state != Some(cursor) {
            self.cursor_since = Instant::now();
            self.cursor_state = Some(cursor);
        }
        self.cursor = if cursor.visible {
            cursor.viewport.map(|p| (p.x, p.y))
        } else {
            None
        };
        let colors = snapshot.colors()?;
        self.cursor_color = color(snapshot.cursor_color()?, rgb(0xffffff));
        let cols = usize::from(snapshot.cols()?);
        let rows_count = usize::from(snapshot.rows()?);
        let effective_colors = (colors.foreground, colors.background);
        let colors_changed = self.default_colors != Some(effective_colors);
        self.default_colors = Some(effective_colors);
        let full = colors_changed
            || snapshot.dirty()? == ghostty_vt::render::Dirty::Full
            || self
                .cells
                .last()
                .is_none_or(|c| c.x + 1 != cols || c.y + 1 != rows_count);
        if !full && snapshot.dirty()? == ghostty_vt::render::Dirty::Clean {
            return Ok(true);
        }
        let mut rows = RowIterator::new()?;
        let mut cells = CellIterator::new()?;
        let mut rows = rows.update(&snapshot)?;
        if full {
            self.cells.clear();
        }
        let mut y = 0;
        while let Some(row) = rows.next() {
            if !full && !row.dirty()? {
                y += 1;
                continue;
            }
            let mut cells = cells.update(row)?;
            let mut x = 0;
            while let Some(cell) = cells.next() {
                let style = cell.style()?;
                let mut fg = color(
                    cell.fg_color()?,
                    color(Some(colors.foreground), rgb(0xdcdcdc)),
                );
                let background = cell.bg_color()?;
                let mut bg = color(background, color(Some(colors.background), rgb(0)));
                if style.inverse {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if style.faint {
                    fg.r *= 0.6;
                    fg.g *= 0.6;
                    fg.b *= 0.6;
                }
                let selected = cell.is_selected()?;
                if selected {
                    bg = rgb(0x375078);
                    fg = rgb(0xffffff);
                }
                let mut text = String::new();
                cell.graphemes_utf8(&mut text)?;
                if style.invisible {
                    text.clear();
                }
                let cell = Cell {
                    x,
                    y,
                    text,
                    fg,
                    bg: (background.is_some() || style.inverse || selected).then_some(bg),
                    narrow: cell.raw_cell()?.wide()? == ghostty_vt::screen::CellWide::Narrow,
                    underline: if style.invisible {
                        ghostty_vt::style::Underline::None
                    } else {
                        style.underline
                    },
                    underline_color: if selected {
                        fg
                    } else {
                        match style.underline_color {
                            ghostty_vt::style::StyleColor::None => fg,
                            ghostty_vt::style::StyleColor::Rgb(value) => color(Some(value), fg),
                            ghostty_vt::style::StyleColor::Palette(index) => {
                                color(Some(colors.palette[usize::from(index.0)]), fg)
                            }
                        }
                    },
                    strikethrough: style.strikethrough && !style.invisible,
                };
                if full {
                    self.cells.push(cell);
                } else {
                    self.cells[y * cols + x] = cell;
                }
                x += 1;
            }
            y += 1;
        }
        snapshot.clean()?;
        Ok(true)
    }
}

fn hold_frame(terminal: &Terminal, since: &mut Option<Instant>) -> Result<bool> {
    if !terminal.mode(ghostty_vt::terminal::Mode::SYNC_OUTPUT)? {
        *since = None;
        return Ok(false);
    }
    Ok(since.get_or_insert_with(Instant::now).elapsed() < Duration::from_secs(1))
}
pub(super) fn resize_terminal(
    terminal: &mut Terminal,
    cols: u16,
    rows: u16,
    width: u32,
    height: u32,
) -> Result<()> {
    let scroll = terminal.scrollbar()?;
    let bottom = scroll.offset.saturating_add(scroll.len) >= scroll.total;
    terminal.resize(cols, rows, width, height)?;
    if bottom {
        terminal.scroll_viewport(ScrollViewport::Bottom);
    }
    Ok(())
}
pub(super) fn selected_text(terminal: &Terminal) -> Result<Option<String>> {
    terminal
        .format_selection_alloc(FormatOptions::new().with_unwrap(true).with_trim(true))?
        .map(|bytes| String::from_utf8(bytes.to_vec()).map_err(Into::into))
        .transpose()
}
fn color(color: Option<ghostty_vt::style::RgbColor>, fallback: Rgba) -> Rgba {
    color.map_or(fallback, |c| {
        rgb((u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b))
    })
}

#[cfg(test)]
#[path = "../tests/unit/tab.rs"]
mod tests;
