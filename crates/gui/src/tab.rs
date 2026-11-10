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
        self.cursor = if snapshot.cursor_visible()? {
            snapshot.cursor_viewport()?.map(|p| (p.x, p.y))
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
mod tests {
    use super::*;
    use crate::{selection_point, terminal_key};
    use ghostty_vt::TerminalOptions;
    use ghostty_vt::selection::Selection;
    use ghostty_vt::terminal::{Point as TerminalPoint, PointCoordinate};
    use gpui::{point, px, size};
    #[test]
    fn terminal_decorations_include_spaces_wide_cells_colors_and_resets() -> Result<()> {
        use ghostty_vt::style::Underline;
        let mut tab = Tab::new(Terminal::new(TerminalOptions::default())?)?;
        tab.terminal.vt_write("\x1b[4;9mA 界\x1b[0mB\r\n\x1b[4:2mD\x1b[4:3mC\x1b[4:4mP\x1b[4:5mH\x1b[24;29mN\r\n\x1b[4;58;2;12;34;56mX\x1b[58;5;1mY\x1b[59mZ\x1b[8;9mI".as_bytes());
        tab.snapshot()?;
        for c in &tab.cells[..4] {
            assert_eq!(c.underline, Underline::Single);
            assert!(c.strikethrough);
        }
        assert_eq!(tab.cells[4].underline, Underline::None);
        assert!(!tab.cells[4].strikethrough);
        let row: Vec<_> = tab
            .cells
            .iter()
            .filter(|c| c.y == 1)
            .take(5)
            .map(|c| c.underline)
            .collect();
        assert_eq!(
            row,
            [
                Underline::Double,
                Underline::Curly,
                Underline::Dotted,
                Underline::Dashed,
                Underline::None
            ]
        );
        let row: Vec<_> = tab.cells.iter().filter(|c| c.y == 2).take(4).collect();
        assert_eq!(row[0].underline_color, rgb(0x0c2238));
        assert_ne!(row[1].underline_color, row[0].underline_color);
        assert_eq!(row[2].underline_color, row[2].fg);
        assert_eq!(row[3].underline, Underline::None);
        assert!(!row[3].strikethrough);
        let mut bytes = Vec::new();
        ghostty_vt::snapshot::encode_to(&tab.terminal, &mut bytes)?;
        let mut restored = Tab::new(ghostty_vt::snapshot::Decoder::from_slice(&bytes).decode()?)?;
        restored.snapshot()?;
        assert!(tab.cells == restored.cells);
        Ok(())
    }
    #[test]
    fn key_encoding_and_sync_redraw() -> Result<()> {
        let mut terminal = Terminal::new(TerminalOptions::default())?;
        let mut encoder = key::Encoder::new()?;
        let mut event = key::Event::new()?;
        event.set_key(terminal_key("escape").unwrap());
        let mut bytes = Vec::new();
        encoder
            .set_options_from_terminal(&terminal)
            .encode_to_vec(&event, &mut bytes)?;
        assert_eq!(bytes, b"\x1b");
        terminal.vt_write(b"\x1b[>1u");
        bytes.clear();
        encoder
            .set_options_from_terminal(&terminal)
            .encode_to_vec(&event, &mut bytes)?;
        assert_eq!(bytes, b"\x1b[27u");
        let mut since = None;
        terminal.vt_write(b"\x1b[?2026hhello");
        assert!(hold_frame(&terminal, &mut since)?);
        since = Some(Instant::now() - Duration::from_secs(2));
        assert!(!hold_frame(&terminal, &mut since)?);
        terminal.vt_write(b"\x1b[?2026l");
        assert!(!hold_frame(&terminal, &mut since)?);
        assert!(since.is_none());
        Ok(())
    }
    #[test]
    fn resize_coalesces_changes_and_waits_for_acknowledgement() -> Result<()> {
        let mut tab = Tab::new(Terminal::new(TerminalOptions::default())?)?;
        let now = Instant::now();
        let first = (80, 24, 8, 16);
        let intermediate = (90, 26, 8, 16);
        let latest = (100, 30, 8, 16);
        assert!(!tab.request_resize(first, now, false));
        assert!(!tab.resize_pending);
        assert!(tab.resize_target.is_none());
        assert!(tab.request_resize(first, now, true));
        assert!(!tab.request_resize(intermediate, now + Duration::from_millis(10), true));
        assert!(!tab.request_resize(latest, now + Duration::from_millis(20), true));
        assert!(!tab.request_resize(latest, now + Duration::from_millis(200), true));
        tab.size = first;
        tab.resize_pending = false;
        // Window/font changes during an outage neither send nor consume a resize acknowledgement.
        assert!(!tab.request_resize(latest, now + Duration::from_millis(200), false));
        assert!(!tab.resize_pending);
        assert!(tab.request_resize(latest, now + Duration::from_millis(200), true));
        tab.size = latest;
        tab.resize_pending = false;
        assert!(!tab.request_resize(latest, now + Duration::from_millis(210), true));
        assert!(!tab.request_resize(intermediate, now + Duration::from_millis(220), true));
        assert!(!tab.request_resize(intermediate, now + Duration::from_millis(250), true));
        assert!(tab.request_resize(intermediate, now + Duration::from_millis(300), true));
        Ok(())
    }

    #[test]
    fn application_cursor_visibility_and_restore() -> Result<()> {
        let mut tab = Tab::new(Terminal::new(TerminalOptions::default())?)?;
        for bytes in [b"\x1b[?25l".as_slice(), b"text", b"\x1b[?1049h"] {
            tab.terminal.vt_write(bytes);
            tab.snapshot()?;
            assert!(tab.cursor.is_none());
        }
        tab.terminal.vt_write(b"\x1b[?1049l\x1b[?25h");
        for _ in 0..3 {
            tab.snapshot()?;
            assert!(tab.cursor.is_some());
        }
        tab.terminal.vt_write(b"\x1b[H\x1b[?25l\x1b[7m \x1b[0m");
        tab.snapshot()?;
        assert!(tab.cursor.is_none());
        assert!(
            tab.cells[0].bg.is_some(),
            "software cursor on a blank cell must retain its background"
        );
        for visible in [false, true] {
            tab.terminal
                .vt_write(if visible { b"\x1b[?25h" } else { b"\x1b[?25l" });
            let mut bytes = Vec::new();
            ghostty_vt::snapshot::encode_to(&tab.terminal, &mut bytes)?;
            let terminal = ghostty_vt::snapshot::Decoder::from_slice(&bytes).decode()?;
            let mut restored = Tab::new(terminal)?;
            restored.snapshot()?;
            assert_eq!(restored.cursor.is_some(), visible);
        }
        Ok(())
    }
    #[test]
    fn osc_default_colors_update_cached_cells_and_reset() -> Result<()> {
        use ghostty_vt::style::RgbColor;
        let mut terminal = Terminal::new(TerminalOptions::default())?;
        terminal.set_default_fg_color(Some(RgbColor {
            r: 0xdc,
            g: 0xdc,
            b: 0xdc,
        }))?;
        terminal.set_default_bg_color(Some(RgbColor { r: 0, g: 0, b: 0 }))?;
        terminal.set_default_cursor_color(Some(RgbColor {
            r: 255,
            g: 255,
            b: 255,
        }))?;
        terminal.vt_write(b"A\x1b[7mB\x1b[0m");
        let mut tab = Tab::new(terminal)?;
        tab.snapshot()?;
        assert_eq!(tab.cells[0].fg, rgb(0xdcdcdc));
        tab.terminal
            .vt_write(b"\x1b]10;#112233\x07\x1b]11;#445566\x07\x1b]12;#778899\x07");
        tab.snapshot()?;
        assert_eq!(tab.cells[0].fg, rgb(0x112233));
        assert_eq!(tab.cells[1].fg, rgb(0x445566));
        assert_eq!(tab.cells[1].bg, Some(rgb(0x112233)));
        assert_eq!(tab.cursor_color, rgb(0x778899));
        assert_eq!(color(tab.terminal.bg_color()?, rgb(0)), rgb(0x445566));
        let mut bytes = Vec::new();
        ghostty_vt::snapshot::encode_to(&tab.terminal, &mut bytes)?;
        let mut tab = Tab::new(ghostty_vt::snapshot::Decoder::from_slice(&bytes).decode()?)?;
        tab.snapshot()?;
        assert_eq!(tab.cells[0].fg, rgb(0x112233));
        assert_eq!(tab.cursor_color, rgb(0x778899));
        tab.terminal
            .vt_write(b"\x1b]110\x07\x1b]111\x07\x1b]112\x07");
        tab.snapshot()?;
        assert_eq!(tab.cells[0].fg, rgb(0xdcdcdc));
        assert_eq!(tab.cells[1].fg, rgb(0));
        assert_eq!(tab.cursor_color, rgb(0xffffff));
        assert_eq!(color(tab.terminal.bg_color()?, rgb(0)), rgb(0));
        tab.terminal.vt_write(b"\x1b[?2026h\x1b]11;#123456\x07");
        assert_eq!(tab.background()?, rgb(0));
        tab.snapshot()?;
        assert_eq!(tab.background()?, rgb(0));
        tab.terminal.vt_write(b"\x1b[?2026l");
        assert_eq!(tab.background()?, rgb(0x123456));
        Ok(())
    }
    #[test]
    fn incremental_cells_match_full_rebuilds() -> Result<()> {
        fn compare(tab: &mut Tab) -> Result<()> {
            tab.snapshot()?;
            let cells = tab.cells.clone();
            let cursor = tab.cursor;
            tab.render = RenderState::new()?;
            tab.snapshot()?;
            assert!(
                tab.cells == cells,
                "incremental cells differ from a full rebuild"
            );
            assert_eq!(tab.cursor, cursor);
            Ok(())
        }
        let mut tab = Tab::new(Terminal::new(TerminalOptions {
            cols: 12,
            rows: 4,
            ..Default::default()
        })?)?;
        for bytes in [
            "first\r\nsecond\r\nthird\r\nfourth",
            "\x1b[2;3Hé界\x1b[31mred\x1b[0m",
            "\x1b[1;1H\x1b[7mX\x1b[0m",
            "\r\nnew line\r\nmore history",
            "\x1b[?25l",
            "\x1b[?1049halternate",
            "\x1b[?1049l\x1b[?25h",
            "\x1b[2J",
        ] {
            tab.terminal.vt_write(bytes.as_bytes());
            compare(&mut tab)?;
        }
        tab.terminal.scroll_viewport(ScrollViewport::Top);
        compare(&mut tab)?;
        let start = tab
            .terminal
            .grid_ref(TerminalPoint::Viewport(PointCoordinate { x: 0, y: 0 }))?;
        let end = tab
            .terminal
            .grid_ref(TerminalPoint::Viewport(PointCoordinate { x: 5, y: 1 }))?;
        tab.terminal
            .set_selection(Some(&Selection::new(start, end, false)))?;
        compare(&mut tab)?;
        tab.terminal.set_selection(None)?;
        compare(&mut tab)?;
        // Same cell count, different geometry must invalidate the whole cache.
        resize_terminal(&mut tab.terminal, 8, 6, 80, 120)?;
        compare(&mut tab)?;
        compare(&mut tab)?;
        Ok(())
    }

    #[test]
    #[ignore = "manual snapshot timing; run with --ignored --nocapture"]
    fn snapshot_timing() -> Result<()> {
        let mut tab = Tab::new(Terminal::new(TerminalOptions {
            cols: 120,
            rows: 40,
            ..Default::default()
        })?)?;
        tab.terminal.vt_write(&b"abcdef ".repeat(600));
        tab.snapshot()?;
        let start = Instant::now();
        for _ in 0..500 {
            tab.snapshot()?;
        }
        eprintln!("500 idle snapshots: {:?}", start.elapsed());
        let start = Instant::now();
        for _ in 0..500 {
            tab.terminal.vt_write(b"\x1b[1;1HX");
            tab.snapshot()?;
        }
        eprintln!("500 single-row snapshots: {:?}", start.elapsed());
        Ok(())
    }

    #[test]
    fn selection_and_snapshot() -> Result<()> {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 5,
            rows: 3,
            ..Default::default()
        })?;
        terminal.vt_write("héllo world".as_bytes());
        let start = terminal.grid_ref(TerminalPoint::Viewport(PointCoordinate { x: 0, y: 0 }))?;
        let end = terminal.grid_ref(TerminalPoint::Viewport(PointCoordinate { x: 4, y: 1 }))?;
        terminal.set_selection(Some(&Selection::new(start, end, false)))?;
        assert_eq!(selected_text(&terminal)?.as_deref(), Some("héllo worl"));
        assert_eq!(
            selection_point(
                point(px(-5.), px(99.)),
                Bounds::new(point(px(0.), px(0.)), size(px(50.), px(30.))),
                size(px(10.), px(10.)),
                5,
                3
            ),
            TerminalPoint::Viewport(PointCoordinate { x: 0, y: 2 })
        );
        let mut tab = Tab::new(terminal)?;
        tab.snapshot()?;
        assert!(tab.cells.iter().any(|c| c.text == "é"));
        Ok(())
    }
    #[test]
    fn resize_preserves_scrollback_position() -> Result<()> {
        let mut terminal = Terminal::new(TerminalOptions::default())?;
        for _ in 0..100 {
            terminal.vt_write(b"output\r\n");
        }
        resize_terminal(&mut terminal, 40, 12, 8, 16)?;
        let scroll = terminal.scrollbar()?;
        assert_eq!(scroll.offset + scroll.len, scroll.total);
        terminal.scroll_viewport(ScrollViewport::Top);
        resize_terminal(&mut terminal, 50, 15, 8, 16)?;
        let scroll = terminal.scrollbar()?;
        assert!(scroll.offset + scroll.len < scroll.total);
        Ok(())
    }
}
