use anyhow::{Context, Result};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Vec2};
use ghostty_vt::{
    RenderState, Terminal, key,
    render::{CellIterator, RowIterator},
    screen::TrackedGridRef,
    selection::{FormatOptions, Selection},
    terminal::{Point, PointCoordinate, ScrollViewport},
};
mod ansi;
mod config;
mod fonts;
mod images;
mod transport;
use config::Config;
use scytty_common::protocol::{self, Request, TabInfo, Update};
use scytty_common::stream::Stream;
use std::collections::HashMap;
use std::{
    sync::mpsc::{self, Receiver},
    thread,
};

struct App {
    terminal: Terminal,
    render: RenderState,
    images: images::Images,
    last_frame: Vec<egui::epaint::ClippedShape>,
    render_hold_since: Option<std::time::Instant>,
    writer: Stream,
    id: u64,
    size: (u16, u16, u32, u32),
    encoder: key::Encoder,
    exited: bool,
    selection_anchor: Option<TrackedGridRef>,
}

impl App {
    fn new(writer: Stream, id: u64, mut terminal: Terminal) -> Result<Self> {
        scytty_common::images::configure(&mut terminal)?;
        let render = RenderState::new()?;
        let encoder = key::Encoder::new()?;
        Ok(Self {
            render,
            images: images::Images::default(),
            last_frame: Vec::new(),
            render_hold_since: None,
            writer,
            id,
            size: (0, 0, 0, 0),
            encoder,
            terminal,
            exited: false,
            selection_anchor: None,
        })
    }

    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.terminal.set_selection(None)?;
        self.selection_anchor = None;
        self.terminal.scroll_viewport(ScrollViewport::Bottom);
        protocol::send(
            &mut self.writer,
            &Request::Input {
                id: self.id,
                bytes: bytes.to_vec(),
            },
        )?;
        Ok(())
    }

    fn paste(&mut self, text: &str) -> Result<()> {
        let mut data = text.as_bytes().to_vec();
        let mut encoded = vec![0; data.len() * 2 + 32];
        let bracketed = self
            .terminal
            .mode(ghostty_vt::terminal::Mode::BRACKETED_PASTE)?;
        let n = ghostty_vt::paste::encode(&mut data, bracketed, &mut encoded)?;
        self.send(&encoded[..n])
    }

    fn draw(&mut self, ui: &mut egui::Ui, font_size: f32, icon_scale: f32) -> Result<()> {
        if self.exited {
            ui.label("Shell exited. Close the window to quit.");
        }
        let font = FontId::monospace(font_size);
        let cell = ui.fonts_mut(|f| Vec2::new(f.glyph_width(&font, 'M'), f.row_height(&font)));
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let size = (
            (rect.width() / cell.x).floor().clamp(1.0, 500.0) as u16,
            (rect.height() / cell.y).floor().clamp(1.0, 300.0) as u16,
            (cell.x as u32).max(1),
            (cell.y as u32).max(1),
        );
        if size != self.size {
            protocol::send(
                &mut self.writer,
                &Request::Resize {
                    id: self.id,
                    cols: size.0,
                    rows: size.1,
                    width: size.2,
                    height: size.3,
                },
            )?;
            self.size = size;
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() >= 1.0 {
                self.terminal
                    .scroll_viewport(ScrollViewport::Delta((scroll / cell.y).round() as isize));
            }
        }
        if response.clicked() {
            self.terminal.set_selection(None)?;
            self.selection_anchor = None;
        }
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            let point = selection_point(
                origin,
                rect,
                cell,
                self.terminal.cols()?,
                self.terminal.rows()?,
            );
            self.selection_anchor = Some(self.terminal.track_grid_ref(point)?);
        }
        if response.dragged_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
            && let Some(anchor) = &self.selection_anchor
            && let Some(start) = anchor.snapshot(&self.terminal)?
        {
            let end = self.terminal.grid_ref(selection_point(
                pos,
                rect,
                cell,
                self.terminal.cols()?,
                self.terminal.rows()?,
            ))?;
            self.terminal
                .set_selection(Some(&Selection::new(start, end, false)))?;
        }
        if !self.exited {
            let files = ui.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
            let paths: Vec<_> = files.iter().map(|file| file.path()).collect();
            if !paths.is_empty() {
                self.paste(&dropped_paths(&paths)?)?;
            }
        }
        if ui.input(|i| i.focused) && !self.exited {
            for event in ui.input(|i| i.events.clone()) {
                match event {
                    egui::Event::Copy => {
                        if let Some(text) = selected_text(&self.terminal)? {
                            ui.ctx().copy_text(text);
                        }
                    }
                    egui::Event::Text(text) => self.send(text.as_bytes())?,
                    egui::Event::Paste(text) => self.paste(&text)?,
                    egui::Event::Key {
                        key: k,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        if let Some(k) = terminal_key(k) {
                            let mut event = key::Event::new()?;
                            let mut mods = key::Mods::empty();
                            if modifiers.shift {
                                mods |= key::Mods::SHIFT;
                            }
                            if modifiers.ctrl {
                                mods |= key::Mods::CTRL;
                            }
                            if modifiers.alt {
                                mods |= key::Mods::ALT;
                            }
                            event.set_key(k).set_mods(mods);
                            let mut bytes = Vec::new();
                            self.encoder
                                .set_options_from_terminal(&self.terminal)
                                .encode_to_vec(&event, &mut bytes)?;
                            self.send(&bytes)?;
                        } else if modifiers.ctrl && !modifiers.alt {
                            let name = k.name();
                            if name.len() == 1 && name.as_bytes()[0].is_ascii_alphabetic() {
                                self.send(&[name.as_bytes()[0].to_ascii_uppercase() & 0x1f])?;
                            } else if k == egui::Key::Space {
                                self.send(&[0])?;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let painter = ui.painter_at(rect);
        if hold_frame(&self.terminal, &mut self.render_hold_since)? {
            for shape in &self.last_frame {
                painter
                    .with_clip_rect(shape.clip_rect.intersect(rect))
                    .add(shape.shape.clone());
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
            return Ok(());
        }
        let first_shape = ui.ctx().graphics(|graphics| {
            graphics
                .get(painter.layer_id())
                .map_or(0, |list| list.next_idx().0)
        });
        let images = self.images.prepare(&self.terminal, ui, rect, cell)?;
        for image in images.iter().filter(|i| i.below_background()) {
            image.paint(&painter);
        }
        let snapshot = self.render.update(&self.terminal)?;
        let mut rows = RowIterator::new()?;
        let mut cells = CellIterator::new()?;
        let mut row_iter = rows.update(&snapshot)?;
        let mut y = 0;
        let mut text_runs = Vec::new();
        while let Some(row) = row_iter.next() {
            let mut cell_iter = cells.update(row)?;
            let mut x = 0;
            while let Some(c) = cell_iter.next() {
                let pos = rect.min + Vec2::new(x as f32 * cell.x, y as f32 * cell.y);
                let style = c.style()?;
                let mut fg = color(c.fg_color()?, Color32::from_gray(220));
                let background = c.bg_color()?;
                let mut bg = color(background, Color32::BLACK);
                if style.inverse {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if style.faint {
                    fg = fg.gamma_multiply(0.6);
                }
                let selected = c.is_selected()?;
                if selected {
                    bg = Color32::from_rgb(55, 80, 120);
                    fg = Color32::WHITE;
                }
                if background.is_some() || style.inverse || selected {
                    painter.rect_filled(Rect::from_min_size(pos, cell), 0.0, bg);
                }
                let mut text = String::new();
                c.graphemes_utf8(&mut text)?;
                let narrow = c.raw_cell()?.wide()? == ghostty_vt::screen::CellWide::Narrow;
                if style.invisible {
                    text.clear();
                }
                text_runs.push((pos, text, fg, narrow));
                x += 1;
            }
            y += 1;
        }
        for image in images.iter().filter(|i| i.below_text()) {
            image.paint(&painter);
        }
        // Paint text after backgrounds so the next cell cannot cover an enlarged icon.
        for (index, (pos, text, fg, narrow)) in text_runs.iter().enumerate() {
            let expand = *narrow
                && text_runs
                    .get(index + 1)
                    .is_some_and(|(next_pos, next, _, next_narrow)| {
                        next_pos.y == pos.y && *next_narrow && expand_nerd_icon(text, next)
                    });
            if expand {
                let area = Rect::from_min_size(*pos, Vec2::new(cell.x * 2.0, cell.y));
                painter.with_clip_rect(area.intersect(rect)).text(
                    area.center(),
                    egui::Align2::CENTER_CENTER,
                    text,
                    FontId::monospace(font.size * icon_scale),
                    *fg,
                );
            } else {
                painter.text(*pos, egui::Align2::LEFT_TOP, text, font.clone(), *fg);
            }
        }
        for image in images.iter().filter(|i| i.above_text()) {
            image.paint(&painter);
        }
        if snapshot.cursor_visible()?
            && let Some(cursor) = snapshot.cursor_viewport()?
        {
            let pos = rect.min + Vec2::new(cursor.x as f32 * cell.x, cursor.y as f32 * cell.y);
            painter.line_segment(
                [pos, Pos2::new(pos.x, pos.y + cell.y)],
                egui::Stroke::new(2.0, Color32::WHITE),
            );
        }
        snapshot.clean()?;
        self.last_frame = ui.ctx().graphics(|graphics| {
            graphics
                .get(painter.layer_id())
                .map(|list| list.all_entries().skip(first_shape).cloned().collect())
                .unwrap_or_default()
        });
        Ok(())
    }
}

fn hold_frame(terminal: &Terminal, since: &mut Option<std::time::Instant>) -> Result<bool> {
    if !terminal.mode(ghostty_vt::terminal::Mode::SYNC_OUTPUT)? {
        *since = None;
        return Ok(false);
    }
    Ok(since.get_or_insert_with(std::time::Instant::now).elapsed()
        < std::time::Duration::from_secs(1))
}

fn resize_terminal(
    terminal: &mut Terminal,
    cols: u16,
    rows: u16,
    width: u32,
    height: u32,
) -> Result<()> {
    let scroll = terminal.scrollbar()?;
    let at_bottom = scroll.offset.saturating_add(scroll.len) >= scroll.total;
    terminal.resize(cols, rows, width, height)?;
    if at_bottom {
        terminal.scroll_viewport(ScrollViewport::Bottom);
    }
    Ok(())
}

fn dropped_paths(paths: &[&std::path::Path]) -> Result<String> {
    paths
        .iter()
        .map(|path| {
            let path = path
                .to_str()
                .context("Dropped file path is not valid UTF-8")?;
            Ok(format!("'{}'", path.replace('\'', "'\\''")))
        })
        .collect::<Result<Vec<_>>>()
        .map(|paths| paths.join(" ") + " ")
}

fn expand_nerd_icon(text: &str, next: &str) -> bool {
    let mut chars = text.chars();
    matches!(
        chars.next(),
        Some('\u{e000}'..='\u{f8ff}' | '\u{f0000}'..='\u{ffffd}' | '\u{100000}'..='\u{10fffd}')
    ) && chars.next().is_none()
        && (next.is_empty() || next == " ")
}

fn selection_point(pos: Pos2, rect: Rect, cell: Vec2, cols: u16, rows: u16) -> Point {
    Point::Viewport(PointCoordinate {
        x: ((pos.x - rect.min.x) / cell.x)
            .floor()
            .clamp(0.0, f32::from(cols.saturating_sub(1))) as u16,
        y: ((pos.y - rect.min.y) / cell.y)
            .floor()
            .clamp(0.0, f32::from(rows.saturating_sub(1))) as u32,
    })
}

fn selected_text(terminal: &Terminal) -> Result<Option<String>> {
    terminal
        .format_selection_alloc(FormatOptions::new().with_unwrap(true).with_trim(true))?
        .map(|bytes| String::from_utf8(bytes.to_vec()).map_err(Into::into))
        .transpose()
}

fn color(rgb: Option<ghostty_vt::style::RgbColor>, fallback: Color32) -> Color32 {
    rgb.map_or(fallback, |c| Color32::from_rgb(c.r, c.g, c.b))
}

fn terminal_key(k: egui::Key) -> Option<key::Key> {
    use egui::Key as E;
    use key::Key as G;
    Some(match k {
        E::Enter => G::Enter,
        E::Backspace => G::Backspace,
        E::Tab => G::Tab,
        E::Escape => G::Escape,
        E::ArrowUp => G::ArrowUp,
        E::ArrowDown => G::ArrowDown,
        E::ArrowLeft => G::ArrowLeft,
        E::ArrowRight => G::ArrowRight,
        E::Home => G::Home,
        E::End => G::End,
        E::PageUp => G::PageUp,
        E::PageDown => G::PageDown,
        E::Insert => G::Insert,
        E::Delete => G::Delete,
        E::F1 => G::F1,
        E::F2 => G::F2,
        E::F3 => G::F3,
        E::F4 => G::F4,
        E::F5 => G::F5,
        E::F6 => G::F6,
        E::F7 => G::F7,
        E::F8 => G::F8,
        E::F9 => G::F9,
        E::F10 => G::F10,
        E::F11 => G::F11,
        E::F12 => G::F12,
        _ => return None,
    })
}

struct Window {
    session: String,
    config: Config,
    _connection: transport::Connection,
    remote: bool,
    writer: Stream,
    updates: Receiver<Result<Update>>,
    tabs: HashMap<u64, App>,
    order: Vec<TabInfo>,
    active: u64,
    error: Option<String>,
}

impl Window {
    fn new(
        ctx: &egui::Context,
        session: String,
        host: Option<String>,
        config: Config,
    ) -> Result<Self> {
        ctx.set_fonts(fonts::definitions(&config.font_family)?);
        let connection = match host.as_deref() {
            Some(host) => transport::connect(host)?,
            None => transport::connect_local()?,
        };
        let mut writer = connection.socket.try_clone()?;
        writer.set_write_timeout(Some(std::time::Duration::from_secs(5)))?;
        writer.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
        protocol::handshake(&mut writer)?;
        writer.set_read_timeout(None)?;
        protocol::send(
            &mut writer,
            &Request::Attach {
                session: session.clone(),
            },
        )?;
        protocol::send(
            &mut writer,
            &Request::SetScrollback {
                megabytes: config.scrollback_mb,
            },
        )?;
        let mut reader = writer.try_clone()?;
        let (tx, updates) = mpsc::sync_channel(256);
        let ctx = ctx.clone();
        thread::spawn(move || {
            loop {
                let update = protocol::receive(&mut reader).context(
                    "Server connection closed or sent an incomplete frame; check SSH output and ~/.scytty/server.log on the server",
                );
                let failed = update.is_err();
                if tx.send(update).is_err() {
                    break;
                }
                ctx.request_repaint();
                if failed {
                    break;
                }
            }
        });
        Ok(Self {
            session,
            config,
            _connection: connection,
            remote: host.is_some(),
            writer,
            updates,
            tabs: HashMap::new(),
            order: Vec::new(),
            active: 0,
            error: None,
        })
    }

    fn draw(&mut self, ui: &mut egui::Ui) -> Result<()> {
        for _ in 0..256 {
            let update = match self.updates.try_recv() {
                Ok(update) => update?,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    anyhow::bail!("Server disconnected; reopen this session to attach again")
                }
            };
            match update {
                Update::Snapshot { id, bytes } => {
                    let mut terminal =
                        ghostty_vt::snapshot::Decoder::from_slice(&bytes).decode()?;
                    terminal.set_scrollback_max_bytes(Some(
                        usize::from(self.config.scrollback_mb) * 1024 * 1024,
                    ))?;
                    self.tabs
                        .insert(id, App::new(self.writer.try_clone()?, id, terminal)?);
                }
                Update::Output { id, bytes } => {
                    if let Some(tab) = self.tabs.get_mut(&id) {
                        tab.terminal.vt_write(&bytes);
                        tab.terminal.events().for_each(drop);
                    }
                }
                Update::Resized {
                    id,
                    cols,
                    rows,
                    width,
                    height,
                } => {
                    if let Some(tab) = self.tabs.get_mut(&id) {
                        resize_terminal(&mut tab.terminal, cols, rows, width, height)?;
                    }
                }
                Update::State { tabs, active } => {
                    if tabs.is_empty() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        return Ok(());
                    }
                    self.tabs.retain(|id, _| tabs.iter().any(|t| t.id == *id));
                    for info in &tabs {
                        if let Some(tab) = self.tabs.get_mut(&info.id) {
                            tab.exited = info.exited;
                        }
                    }
                    let title = tabs
                        .iter()
                        .enumerate()
                        .find(|(_, tab)| tab.id == active)
                        .map_or_else(|| "scytty".to_owned(), |(index, tab)| tab_title(index, tab));
                    let title = self
                        .config
                        .window_title(&self.session, &title, self.remote)?;
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Title(title));
                    self.order = tabs;
                    self.active = active;
                }
                Update::Error(message) => anyhow::bail!(message),
            }
        }
        let mut requests = Vec::new();
        ui.input_mut(|input| {
            if input.consume_key(egui::Modifiers::COMMAND, egui::Key::T) {
                requests.push(Request::NewTab);
            }
            for (key, direction) in [(egui::Key::ArrowLeft, -1), (egui::Key::ArrowRight, 1)] {
                if input.consume_key(egui::Modifiers::COMMAND, key)
                    && let Some(id) = adjacent_tab(&self.order, self.active, direction)
                {
                    requests.push(Request::Activate { id });
                }
            }
        });
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(1));
        let remote = self.remote;
        let active_tab = self.order.iter().find(|tab| tab.id == self.active);
        let left = self
            .config
            .status_text("left_status", "", 0, false, remote, active_tab)?;
        let right = self
            .config
            .status_text("right_status", "", 0, false, remote, active_tab)?;
        let tabs = self
            .order
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                self.config.status_text(
                    "format_tab",
                    &tab_title(index, tab),
                    index,
                    tab.id == self.active,
                    remote,
                    Some(tab),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        // Native window controls do not shrink with egui's UI zoom.
        let (button_width, bar_height) = titlebar_size(ui.ctx().zoom_factor());
        let spacing = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 0.0;
        let (bar_rect, header) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), bar_height),
            egui::Sense::CLICK | egui::Sense::DRAG,
        );
        if cfg!(target_os = "windows") && header.double_clicked() {
            toggle_maximized(ui.ctx());
        }
        if header.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        let mut content_rect = bar_rect;
        content_rect.max.x =
            (content_rect.max.x - 8.0 / ui.ctx().zoom_factor()).max(content_rect.min.x);
        if cfg!(target_os = "macos") {
            content_rect.min.x = (content_rect.min.x + button_width).min(content_rect.max.x);
        } else if cfg!(target_os = "windows") {
            let controls = windows_controls_rect(bar_rect, ui.ctx().zoom_factor());
            content_rect.max.x = controls.left().max(content_rect.min.x);
            windows_controls(ui, controls);
        }
        let mut bar = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        bar.set_clip_rect(content_rect.intersect(ui.clip_rect()));
        {
            let ui = &mut bar;
            ui.set_min_height(bar_height);
            ui.spacing_mut().item_spacing.x = 0.0;
            let font = FontId::monospace(self.config.font_size);
            let cell = ui.fonts_mut(|fonts| fonts.glyph_width(&font, 'M'));
            let right_width =
                right.iter().map(|(s, _)| s.chars().count()).sum::<usize>() as f32 * cell;
            let end = ui.max_rect().right();
            let right_start = (end - right_width).max(ui.cursor().left());
            let draw =
                |ui: &mut egui::Ui, segments: &[(String, Option<Color32>)], max_width: f32| {
                    let chars: Vec<_> = segments
                        .iter()
                        .flat_map(|(text, color)| {
                            text.chars().map(move |ch| {
                                (
                                    ch.to_string(),
                                    color.unwrap_or(Color32::from_rgb(198, 208, 245)),
                                )
                            })
                        })
                        .take((max_width.max(0.0) / cell + 0.001).floor() as usize)
                        .collect();
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(chars.len() as f32 * cell, bar_height),
                        egui::Sense::CLICK,
                    );
                    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
                    for (index, (text, color)) in chars.iter().enumerate() {
                        let expand = chars
                            .get(index + 1)
                            .is_some_and(|(next, _)| expand_nerd_icon(text, next));
                        let width = if expand { cell * 2.0 } else { cell };
                        let area = Rect::from_min_size(
                            egui::pos2(rect.left() + index as f32 * cell, bar_rect.top()),
                            egui::vec2(width, bar_rect.height()),
                        );
                        painter.text(
                            area.center(),
                            egui::Align2::CENTER_CENTER,
                            text,
                            FontId::monospace(
                                font.size * if expand { self.config.icon_scale } else { 1.0 },
                            ),
                            *color,
                        );
                    }
                    response
                };
            draw(
                ui,
                &left,
                (right_start - ui.cursor().left() - cell * 2.0).max(0.0),
            );
            for (tab, segments) in self.order.iter().zip(&tabs) {
                let width = (right_start - ui.cursor().left() - cell * 2.0).clamp(0.0, cell * 25.0);
                if draw(ui, segments, width)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    requests.push(Request::Activate { id: tab.id });
                }
            }
            let width = (right_start - ui.cursor().left()).max(0.0);
            if draw(ui, &[(" +".into(), None)], width)
                .on_hover_text("New tab (Cmd+T)")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                requests.push(Request::NewTab);
            }
            ui.add_space((right_start - ui.cursor().left()).max(0.0));
            draw(ui, &right, (end - ui.cursor().left()).max(0.0));
        }
        ui.spacing_mut().item_spacing.y = spacing;
        for request in requests {
            protocol::send(&mut self.writer, &request)?;
        }
        if let Some(tab) = self.tabs.get_mut(&self.active) {
            tab.draw(ui, self.config.font_size, self.config.icon_scale)?;
        }
        Ok(())
    }
}

fn windows_resize_edges(ui: &mut egui::Ui) {
    if ui.input(|input| input.viewport().maximized.unwrap_or(false)) {
        return;
    }
    use egui::{CursorIcon as C, ResizeDirection as D};
    let r = ui.max_rect();
    let edge = 5.0 / ui.ctx().zoom_factor();
    let x = [r.left(), r.left() + edge, r.right() - edge, r.right()];
    let y = [r.top(), r.top() + edge, r.bottom() - edge, r.bottom()];
    for (col, row, direction, cursor) in [
        (0, 0, D::NorthWest, C::ResizeNwSe),
        (1, 0, D::North, C::ResizeVertical),
        (2, 0, D::NorthEast, C::ResizeNeSw),
        (0, 1, D::West, C::ResizeHorizontal),
        (2, 1, D::East, C::ResizeHorizontal),
        (0, 2, D::SouthWest, C::ResizeNeSw),
        (1, 2, D::South, C::ResizeVertical),
        (2, 2, D::SouthEast, C::ResizeNwSe),
    ] {
        let rect = Rect::from_min_max(
            egui::pos2(x[col], y[row]),
            egui::pos2(x[col + 1], y[row + 1]),
        );
        let response = ui
            .interact(
                rect,
                ui.scope_id().with(("resize", col, row)),
                egui::Sense::DRAG,
            )
            .on_hover_cursor(cursor);
        if response.drag_started() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
    }
}

fn toggle_maximized(ctx: &egui::Context) {
    let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
}

fn windows_controls_rect(bar: Rect, zoom: f32) -> Rect {
    Rect::from_min_max(
        egui::pos2((bar.right() - 138.0 / zoom).max(bar.left()), bar.top()),
        bar.max,
    )
}

// shortcut: custom controls lack Windows' hover snap menu; use native hit-testing if needed.
fn windows_controls(ui: &mut egui::Ui, rect: Rect) {
    let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
    let controls = [
        ("−", "Minimize"),
        (
            if maximized { "❐" } else { "□" },
            if maximized { "Restore" } else { "Maximize" },
        ),
        ("×", "Close"),
    ];
    for (index, (symbol, name)) in controls.into_iter().enumerate() {
        let button_rect = Rect::from_min_size(
            egui::pos2(rect.left() + rect.width() * index as f32 / 3.0, rect.top()),
            egui::vec2(rect.width() / 3.0, rect.height()),
        );
        let response = ui
            .put(
                button_rect,
                egui::Button::new(symbol)
                    .sense(egui::Sense::CLICK)
                    .frame(false),
            )
            .on_hover_text(name);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
        if response.clicked() {
            match index {
                0 => ui
                    .ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                1 => toggle_maximized(ui.ctx()),
                _ => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
    }
}

fn titlebar_size(zoom: f32) -> (f32, f32) {
    (78.0 / zoom, 32.0 / zoom)
}

fn tab_title(index: usize, tab: &TabInfo) -> String {
    if tab.title.is_empty() {
        format!("Tab {}", index + 1)
    } else {
        tab.title.clone()
    }
}

fn adjacent_tab(tabs: &[TabInfo], active: u64, direction: isize) -> Option<u64> {
    let index = tabs.iter().position(|t| t.id == active)?;
    Some(tabs[(index as isize + direction).rem_euclid(tabs.len() as isize) as usize].id)
}

impl Drop for Window {
    fn drop(&mut self) {
        let _ = self.writer.shutdown(std::net::Shutdown::Both);
    }
}

impl eframe::App for Window {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, self.config.background_opacity]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        if ui.input(|input| input.viewport().close_requested()) {
            return;
        }
        if let Some(error) = &self.error {
            show_error(ui, error);
            return;
        }
        let window_rect = ui.max_rect();
        if let Err(e) = self.draw(ui) {
            eprintln!("Terminal error: {e:#}");
            self.error = Some(format!("Terminal error: {e:#}"));
        }
        if cfg!(target_os = "windows") {
            let mut overlay = ui.new_child(egui::UiBuilder::new().max_rect(window_rect));
            windows_resize_edges(&mut overlay);
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
}

fn show_error(ui: &mut egui::Ui, error: &str) {
    let width = (ui.max_rect().width() - 48.0).clamp(120.0, 520.0);
    let height = (ui.max_rect().height() - 120.0).clamp(60.0, 240.0);
    egui::Window::new("Terminal warning")
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .fixed_size(egui::vec2(width, height + 36.0))
        .show(ui.ctx(), |ui| {
            egui::ScrollArea::vertical()
                .max_height(height)
                .show(ui, |ui| {
                    let mut text = error;
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(width)
                            .desired_rows(6),
                    );
                });
            ui.horizontal(|ui| {
                if ui.button("Copy error").clicked() {
                    ui.ctx().copy_text(error.to_owned());
                }
                if ui.button("Close window").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
}

struct ErrorWindow {
    error: String,
    opacity: f32,
}
impl eframe::App for ErrorWindow {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, self.opacity]
    }
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        show_error(ui, &self.error);
    }
}

fn main() -> eframe::Result {
    let config = Config::load().map_err(|error| eframe::Error::AppCreation(error.into()))?;
    let mut args = std::env::args().skip(1);
    let argument = args.next();
    let (host, session) = if argument.as_deref() == Some("--ssh") {
        let host = args.next().ok_or_else(|| {
            eframe::Error::AppCreation("usage: scytty --ssh HOST [SESSION]".into())
        })?;
        (Some(host), args.next().unwrap_or_else(|| "default".into()))
    } else {
        (None, argument.unwrap_or_else(|| "default".into()))
    };
    let viewport = egui::ViewportBuilder::default();
    // Keep the macOS bundle icon instead of eframe's default runtime icon.
    #[cfg(target_os = "macos")]
    let viewport = viewport
        .with_icon(egui::IconData::default())
        // Native shadows darken transparent content differently when focus changes.
        .with_has_shadow(false);
    #[cfg(target_os = "windows")]
    let viewport = viewport.with_decorations(false);
    eframe::run_native(
        "scytty",
        eframe::NativeOptions {
            viewport: viewport
                .with_inner_size([config.window_width, config.window_height])
                .with_transparent(true)
                .with_fullsize_content_view(true)
                .with_titlebar_shown(false)
                .with_title_shown(false)
                .with_titlebar_buttons_shown(true),
            ..Default::default()
        },
        Box::new(move |cc| {
            let opacity = config.background_opacity;
            let app: Box<dyn eframe::App> = match Window::new(&cc.egui_ctx, session, host, config) {
                Ok(window) => Box::new(window),
                Err(error) => {
                    // A bad configured font must not hide the error itself.
                    cc.egui_ctx.set_fonts(fonts::definitions("monospace")?);
                    Box::new(ErrorWindow {
                        error: format!("{error:#}"),
                        opacity,
                    })
                }
            };
            Ok(app)
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghostty_vt::{TerminalOptions, terminal::Event as TerminalEvent};

    #[test]
    fn synchronized_redraw_waits_for_end_or_timeout() -> Result<()> {
        let mut terminal = Terminal::new(TerminalOptions::default())?;
        let mut since = None;
        assert!(!hold_frame(&terminal, &mut since)?);
        terminal.vt_write(b"\x1b[?2026hpartial redraw");
        assert!(hold_frame(&terminal, &mut since)?);
        since = Some(std::time::Instant::now() - std::time::Duration::from_secs(2));
        assert!(!hold_frame(&terminal, &mut since)?);
        terminal.vt_write(b"\x1b[?2026l");
        assert!(!hold_frame(&terminal, &mut since)?);
        assert!(since.is_none());
        terminal.vt_write(b"\x1b[?2026hnext redraw");
        assert!(hold_frame(&terminal, &mut since)?);
        Ok(())
    }

    #[test]
    fn resize_keeps_live_output_at_bottom() -> Result<()> {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 80,
            rows: 24,
            ..Default::default()
        })?;
        for _ in 0..100 {
            terminal.vt_write(b"some output\r\n");
        }
        for (cols, rows) in [(40, 12), (100, 30), (60, 20)] {
            resize_terminal(&mut terminal, cols, rows, 8, 16)?;
            let scroll = terminal.scrollbar()?;
            assert_eq!(scroll.offset + scroll.len, scroll.total);
        }
        terminal.scroll_viewport(ScrollViewport::Top);
        resize_terminal(&mut terminal, 50, 15, 8, 16)?;
        let scroll = terminal.scrollbar()?;
        assert!(scroll.offset + scroll.len < scroll.total);
        Ok(())
    }

    #[test]
    fn dropped_file_paths_are_shell_quoted() -> Result<()> {
        use std::path::Path;
        assert_eq!(
            dropped_paths(&[Path::new("/tmp/a b"), Path::new("/tmp/it's;$x.png")])?,
            "'/tmp/a b' '/tmp/it'\\''s;$x.png' "
        );
        assert_eq!(
            dropped_paths(&[Path::new("/tmp/日本語.png")])?,
            "'/tmp/日本語.png' "
        );
        Ok(())
    }

    #[test]
    fn escape_encoding() -> Result<()> {
        let mut terminal = Terminal::new(TerminalOptions::default())?;
        let mut encoder = key::Encoder::new()?;
        let mut event = key::Event::new()?;
        event.set_key(terminal_key(egui::Key::Escape).unwrap());
        assert_eq!(event.action(), key::Action::Press);
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
        Ok(())
    }
    #[test]
    fn nerd_icons_expand_only_into_blank_cells() {
        assert!(expand_nerd_icon("\u{e0b0}", " "));
        assert!(expand_nerd_icon("\u{f0001}", ""));
        assert!(!expand_nerd_icon("\u{e0b0}", "x"));
        assert!(!expand_nerd_icon("A", " "));
        assert!(!expand_nerd_icon("\u{e0b0}x", " "));
    }

    #[test]
    fn selection_copy_and_coordinates() -> Result<()> {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 5,
            rows: 3,
            ..Default::default()
        })?;
        terminal.vt_write("héllo world".as_bytes());
        assert!(selected_text(&terminal)?.is_none());
        let start = terminal.grid_ref(Point::Viewport(PointCoordinate { x: 0, y: 0 }))?;
        let end = terminal.grid_ref(Point::Viewport(PointCoordinate { x: 4, y: 1 }))?;
        terminal.set_selection(Some(&Selection::new(start, end, false)))?;
        assert_eq!(selected_text(&terminal)?.as_deref(), Some("héllo worl"));
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(50.0, 30.0));
        assert_eq!(
            selection_point(Pos2::new(-5.0, 99.0), rect, Vec2::splat(10.0), 5, 3),
            Point::Viewport(PointCoordinate { x: 0, y: 2 })
        );
        Ok(())
    }

    #[test]
    fn titlebar_clearance_survives_zoom() {
        for zoom in [0.5, 0.75, 1.0, 1.5, 2.0] {
            let (width, height) = titlebar_size(zoom);
            assert!((width * zoom - 78.0).abs() < 0.001);
            assert!((height * zoom - 32.0).abs() < 0.001);
        }
    }

    #[test]
    fn windows_controls_stay_on_right_at_any_zoom() {
        for zoom in [0.5, 1.0, 1.5, 2.0] {
            let bar = Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 32.0 / zoom));
            let controls = windows_controls_rect(bar, zoom);
            assert_eq!(controls.right(), bar.right());
            assert_eq!(controls.height(), bar.height());
            assert!((controls.width() * zoom - 138.0).abs() < 0.001);
            let narrow = Rect::from_min_size(bar.min, egui::vec2(50.0, bar.height()));
            assert_eq!(windows_controls_rect(narrow, zoom), narrow);
        }
    }

    #[test]
    fn tab_navigation_wraps() {
        let tabs: Vec<_> = [4, 8, 12]
            .into_iter()
            .map(|id| TabInfo {
                id,
                title: String::new(),
                ..Default::default()
            })
            .collect();
        assert_eq!(tab_title(0, &tabs[0]), "Tab 1");
        assert_eq!(
            tab_title(
                1,
                &TabInfo {
                    id: 8,
                    title: "vim".into(),
                    ..Default::default()
                }
            ),
            "vim"
        );
        assert_eq!(adjacent_tab(&tabs, 4, -1), Some(12));
        assert_eq!(adjacent_tab(&tabs, 12, 1), Some(4));
        assert_eq!(adjacent_tab(&tabs, 8, 1), Some(12));
        assert_eq!(adjacent_tab(&[], 0, 1), None);
    }

    #[test]
    fn ghostty_parses_text_and_answers_cursor_query() -> Result<()> {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 80,
            rows: 24,
            ..Default::default()
        })?;
        terminal.vt_write(b"hello\x1b[6n");
        assert_eq!(terminal.cursor_x()?, 5);
        assert!(
            terminal
                .events()
                .any(|e| matches!(e, TerminalEvent::PtyWrite(bytes) if bytes == b"\x1b[1;6R"))
        );
        assert_eq!(terminal_key(egui::Key::ArrowUp), Some(key::Key::ArrowUp));
        Ok(())
    }
}
