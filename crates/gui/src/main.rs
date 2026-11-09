#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{Context as _, Result};
use ghostty_vt::{
    RenderState, Terminal, key,
    render::{CellIterator, RowIterator},
    screen::TrackedGridRef,
    selection::{FormatOptions, Selection},
    terminal::{Point as TerminalPoint, PointCoordinate, ScrollViewport},
};
use gpui::{prelude::*, *};
use scytty_common::{
    protocol::{self, Request, TabInfo, Update},
    stream::Stream,
};
use std::{
    collections::HashMap,
    ops::Range,
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};
mod ansi;
mod config;
mod fonts;
mod images;
mod links;
mod remote;
mod scrollbar;
mod transport;
mod window_state;
use config::Config;

#[derive(Clone)]
struct Cell {
    x: usize,
    y: usize,
    text: String,
    fg: Rgba,
    bg: Option<Rgba>,
    narrow: bool,
}
struct Tab {
    terminal: Terminal,
    render: RenderState,
    images: images::Images,
    cells: Vec<Cell>,
    cursor: Option<(u16, u16)>,
    hold_since: Option<Instant>,
    encoder: key::Encoder,
    size: (u16, u16, u32, u32),
    resize_target: Option<((u16, u16, u32, u32), Instant)>,
    resize_pending: bool,
    anchor: Option<TrackedGridRef>,
    exited: bool,
}
impl Tab {
    fn new(mut terminal: Terminal) -> Result<Self> {
        scytty_common::images::configure(&mut terminal)?;
        Ok(Self {
            terminal,
            render: RenderState::new()?,
            images: Default::default(),
            cells: vec![],
            cursor: None,
            hold_since: None,
            encoder: key::Encoder::new()?,
            size: (0, 0, 0, 0),
            resize_target: None,
            resize_pending: false,
            anchor: None,
            exited: false,
        })
    }
    fn request_resize(&mut self, size: (u16, u16, u32, u32), now: Instant) -> bool {
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
    fn snapshot(&mut self) -> Result<()> {
        if hold_frame(&self.terminal, &mut self.hold_since)? {
            return Ok(());
        }
        let snapshot = self.render.update(&self.terminal)?;
        let mut rows = RowIterator::new()?;
        let mut cells = CellIterator::new()?;
        let mut rows = rows.update(&snapshot)?;
        self.cells.clear();
        let mut y = 0;
        while let Some(row) = rows.next() {
            let mut cells = cells.update(row)?;
            let mut x = 0;
            while let Some(cell) = cells.next() {
                let style = cell.style()?;
                let mut fg = color(cell.fg_color()?, rgb(0xdcdcdc));
                let background = cell.bg_color()?;
                let mut bg = color(background, rgb(0));
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
                self.cells.push(Cell {
                    x,
                    y,
                    text,
                    fg,
                    bg: (background.is_some() || style.inverse || selected).then_some(bg),
                    narrow: cell.raw_cell()?.wide()? == ghostty_vt::screen::CellWide::Narrow,
                });
                x += 1;
            }
            y += 1;
        }
        self.cursor = if snapshot.cursor_visible()? {
            snapshot.cursor_viewport()?.map(|p| (p.x, p.y))
        } else {
            None
        };
        snapshot.clean()?;
        Ok(())
    }
}

struct Gui {
    config: Config,
    initial_font_size: f32,
    session: String,
    remote: bool,
    host: Option<String>,
    plus_menu_position: Option<gpui::Point<Pixels>>,
    plus_menu_selected: usize,
    connection: Option<transport::Connection>,
    writer: Option<Stream>,
    updates: Option<Receiver<Result<Update>>>,
    pending: Option<Result<Update>>,
    window_bounds: Option<WindowBounds>,
    tabs: HashMap<u64, Tab>,
    order: Vec<TabInfo>,
    active: u64,
    error: Option<String>,
    remote_restart: Option<remote::RestartRequired>,
    focus: FocusHandle,
    bounds: Bounds<Pixels>,
    cell: Size<Pixels>,
    marked: String,
    marked_selection: Range<usize>,
    scroll_remainder: f32,
    scroll_drag: Option<(u64, Pixels)>,
    link_press: Option<(gpui::Point<Pixels>, String)>,
    link_hovered: Option<gpui::Point<Pixels>>,
    scrollbar_last_used: Instant,
    scrollbar_position: Option<(u64, u64)>,
    scrollbar_hovered: bool,
}
impl Gui {
    fn new(
        mut config: Config,
        session: String,
        host: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let font = fonts::resolve(&config.font_family);
        let error = font.as_ref().err().map(|e| format!("{e:#}"));
        config.font_family = font.unwrap_or_else(|_| "monospace".into());
        let mut view = Self {
            initial_font_size: config.font_size,
            config,
            session,
            remote: host.is_some(),
            host: host.clone(),
            plus_menu_position: None,
            plus_menu_selected: 0,
            connection: None,
            writer: None,
            updates: None,
            pending: None,
            window_bounds: None,
            tabs: HashMap::new(),
            order: vec![],
            active: 0,
            error,
            remote_restart: None,
            focus: cx.focus_handle(),
            bounds: Bounds::default(),
            cell: size(px(8.), px(20.)),
            marked: String::new(),
            marked_selection: 0..0,
            scroll_remainder: 0.,
            scroll_drag: None,
            link_press: None,
            link_hovered: None,
            scrollbar_last_used: Instant::now(),
            scrollbar_position: None,
            scrollbar_hovered: false,
        };
        if view.error.is_none()
            && let Err(e) = view.connect(host)
        {
            view.remote_restart = e.downcast_ref::<remote::RestartRequired>().cloned();
            view.error = Some(format!("{e:#}"));
        }
        cx.spawn(async move |view, cx| {
            let mut last_status = Instant::now();
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                // shortcut: poll the blocking transport every 16ms; use async wakeups if profiling warrants it.
                if view
                    .update(cx, |view, cx| {
                        if view.error.is_some() {
                            return;
                        }
                        if view.pending.is_none()
                            && let Some(rx) = &view.updates
                        {
                            view.pending = match rx.try_recv() {
                                Ok(update) => Some(update),
                                Err(mpsc::TryRecvError::Empty) => None,
                                Err(_) => Some(Err(anyhow::anyhow!(
                                    "Server disconnected; reopen this session to attach again"
                                ))),
                            };
                        }
                        if view.pending.is_some() || last_status.elapsed() >= Duration::from_secs(1)
                        {
                            last_status = Instant::now();
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        view
    }
    fn connect(&mut self, host: Option<String>) -> Result<()> {
        let connection = match host {
            Some(host) => transport::connect(&host)?,
            None => transport::connect_local()?,
        };
        let mut writer = connection.socket.try_clone()?;
        writer.set_write_timeout(Some(Duration::from_secs(5)))?;
        writer.set_read_timeout(Some(Duration::from_secs(5)))?;
        protocol::handshake(&mut writer)?;
        writer.set_read_timeout(None)?;
        protocol::send(
            &mut writer,
            &Request::Attach {
                session: self.session.clone(),
            },
        )?;
        protocol::send(
            &mut writer,
            &Request::SetScrollback {
                megabytes: self.config.scrollback_mb,
            },
        )?;
        let mut reader = writer.try_clone()?;
        let (tx, updates) = mpsc::sync_channel(256);
        thread::spawn(move || {
            loop {
                let update = protocol::receive(&mut reader).context("Server connection closed or sent an incomplete frame; check SSH output and ~/.scytty/server.log on the server");
                let failed = update.is_err();
                if tx.send(update).is_err() || failed {
                    break;
                }
            }
        });
        self.writer = Some(writer);
        self.updates = Some(updates);
        self.connection = Some(connection);
        Ok(())
    }
    fn request(&mut self, request: Request) -> Result<()> {
        protocol::send(
            self.writer.as_mut().context("Server is not connected")?,
            &request,
        )
    }
    fn input(&mut self, bytes: &[u8]) -> Result<()> {
        let tab = self
            .tabs
            .get_mut(&self.active)
            .context("Waiting for terminal")?;
        if tab.exited {
            return Ok(());
        }
        tab.terminal.set_selection(None)?;
        tab.anchor = None;
        tab.terminal.scroll_viewport(ScrollViewport::Bottom);
        self.request(Request::Input {
            id: self.active,
            bytes: bytes.to_vec(),
        })
    }
    fn paste(&mut self, text: &str) -> Result<()> {
        let Some(tab) = self.tabs.get(&self.active) else {
            return Ok(());
        };
        let bracketed = tab
            .terminal
            .mode(ghostty_vt::terminal::Mode::BRACKETED_PASTE)?;
        let mut data = text.as_bytes().to_vec();
        let mut encoded = vec![0; data.len() * 2 + 32];
        let n = ghostty_vt::paste::encode(&mut data, bracketed, &mut encoded)?;
        self.input(&encoded[..n])
    }
    fn report(&mut self, result: Result<()>, cx: &mut Context<Self>) {
        if let Err(e) = result {
            self.remote_restart = e.downcast_ref::<remote::RestartRequired>().cloned();
            self.error = Some(format!("{e:#}"));
        }
        cx.notify();
    }
    fn drain(&mut self, window: &mut Window) -> Result<()> {
        for _ in 0..256 {
            let Some(rx) = &self.updates else { break };
            let update = match self.pending.take().map(Ok).unwrap_or_else(|| rx.try_recv()) {
                Ok(update) => update?,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(_) => anyhow::bail!("Server disconnected; reopen this session to attach again"),
            };
            match update {
                Update::Snapshot { id, bytes } => {
                    let mut terminal =
                        ghostty_vt::snapshot::Decoder::from_slice(&bytes).decode()?;
                    terminal.set_scrollback_max_bytes(Some(
                        usize::from(self.config.scrollback_mb) * 1024 * 1024,
                    ))?;
                    self.tabs.insert(id, Tab::new(terminal)?);
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
                        tab.size = (cols, rows, width, height);
                        tab.resize_pending = false;
                    }
                }
                Update::State { tabs, active } => {
                    if tabs.is_empty() {
                        window.remove_window();
                        return Ok(());
                    }
                    self.tabs
                        .retain(|id, _| tabs.iter().any(|tab| tab.id == *id));
                    for info in &tabs {
                        if let Some(tab) = self.tabs.get_mut(&info.id) {
                            tab.exited = info.exited;
                        }
                    }
                    let title = tabs
                        .iter()
                        .enumerate()
                        .find(|(_, t)| t.id == active)
                        .map_or_else(|| "scytty".into(), |(i, t)| tab_title(i, t));
                    window.set_window_title(&self.config.window_title(
                        &self.session,
                        &title,
                        self.remote,
                    )?);
                    self.order = tabs;
                    self.active = active;
                }
                Update::Error(message) => anyhow::bail!(message),
            }
        }
        Ok(())
    }
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let stroke = &event.keystroke;
        if self.plus_menu_position.is_some() {
            match stroke.key.as_str() {
                "escape" => self.plus_menu_position = None,
                "down" => {
                    self.plus_menu_selected =
                        (self.plus_menu_selected + 1) % self.config.plus_menu.len()
                }
                "up" => {
                    self.plus_menu_selected =
                        (self.plus_menu_selected + self.config.plus_menu.len() - 1)
                            % self.config.plus_menu.len()
                }
                "enter" => {
                    let action = self.config.plus_menu[self.plus_menu_selected]
                        .action
                        .clone();
                    let result = self.menu_action(&action, cx);
                    self.report(result, cx);
                }
                _ => {}
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let m = stroke.modifiers;
        let command = if cfg!(target_os = "macos") {
            m.platform
        } else {
            m.control
        };
        let result = if let Some(size) =
            zoomed_font_size(self.config.font_size, self.initial_font_size, stroke)
        {
            self.config.font_size = size;
            Some(Ok(()))
        } else if command && stroke.key == "t" {
            Some(self.request(Request::NewTab))
        } else if command && matches!(stroke.key.as_str(), "left" | "right") {
            adjacent_tab(
                &self.order,
                self.active,
                if stroke.key == "left" { -1 } else { 1 },
            )
            .map(|id| self.request(Request::Activate { id }))
        } else if command && (cfg!(target_os = "macos") || m.shift) && stroke.key == "c" {
            let result = self
                .tabs
                .get(&self.active)
                .map(|tab| selected_text(&tab.terminal))
                .transpose()
                .map(|text| {
                    if let Some(Some(text)) = text {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }
                });
            Some(result)
        } else if command && (cfg!(target_os = "macos") || m.shift) && stroke.key == "v" {
            Some(
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.paste(&text)
                } else {
                    Ok(())
                },
            )
        } else {
            self.encode_key(stroke).transpose()
        };
        if let Some(result) = result {
            cx.stop_propagation();
            self.report(result, cx);
        }
        self.focus.focus(window);
    }
    fn encode_key(&mut self, stroke: &Keystroke) -> Result<Option<()>> {
        let Some(tab) = self.tabs.get_mut(&self.active) else {
            return Ok(None);
        };
        let m = stroke.modifiers;
        if let Some(k) = terminal_key(&stroke.key) {
            let mut event = key::Event::new()?;
            let mut mods = key::Mods::empty();
            if m.shift {
                mods |= key::Mods::SHIFT;
            }
            if m.control {
                mods |= key::Mods::CTRL;
            }
            if m.alt {
                mods |= key::Mods::ALT;
            }
            event.set_key(k).set_mods(mods);
            let mut bytes = Vec::new();
            tab.encoder
                .set_options_from_terminal(&tab.terminal)
                .encode_to_vec(&event, &mut bytes)?;
            self.input(&bytes)?;
            return Ok(Some(()));
        }
        if m.control && !m.alt && stroke.key.len() == 1 {
            let ch = stroke.key.as_bytes()[0];
            if ch.is_ascii_alphabetic() {
                self.input(&[ch.to_ascii_uppercase() & 0x1f])?;
                return Ok(Some(()));
            }
        }
        if m.control && stroke.key == "space" {
            self.input(&[0])?;
            return Ok(Some(()));
        }
        if m.alt
            && !m.platform
            && let Some(text) = stroke.key_char.as_deref()
        {
            let bytes = format!("\x1b{text}");
            self.input(bytes.as_bytes())?;
            return Ok(Some(()));
        }
        Ok(None)
    }
    fn link_at(&self, position: gpui::Point<Pixels>) -> Result<Option<String>> {
        if !self.bounds.contains(&position)
            || position.x >= self.bounds.right() - px(scrollbar::WIDTH)
        {
            return Ok(None);
        }
        let Some(tab) = self.tabs.get(&self.active) else {
            return Ok(None);
        };
        let point = selection_point(
            position,
            self.bounds,
            self.cell,
            tab.terminal.cols()?,
            tab.terminal.rows()?,
        );
        links::at_terminal(tab, point)
    }
    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.scroll_drag = None;
        if let Some((start, url)) = self.link_press.take()
            && (event.position.x - start.x).abs() <= px(4.)
            && (event.position.y - start.y).abs() <= px(4.)
            && self.link_at(event.position).ok().flatten().as_deref() == Some(url.as_str())
        {
            cx.open_url(&url);
        }
    }
    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window);
        self.link_press = self
            .link_at(event.position)
            .ok()
            .flatten()
            .map(|url| (event.position, url));
        let result = (|| -> Result<()> {
            if let Some(tab) = self.tabs.get_mut(&self.active) {
                let bar = scrollbar::ScrollBar::new(self.bounds, tab.terminal.scrollbar()?);
                if bar.track.contains(&event.position) {
                    let grab = bar.grab_offset(event.position);
                    self.scroll_drag = Some((self.active, grab));
                    self.scrollbar_last_used = Instant::now();
                    tab.terminal
                        .scroll_viewport(ScrollViewport::Row(bar.row(event.position.y, grab)));
                    return Ok(());
                }
                tab.terminal.set_selection(None)?;
                tab.anchor = Some(tab.terminal.track_grid_ref(selection_point(
                    event.position,
                    self.bounds,
                    self.cell,
                    tab.terminal.cols()?,
                    tab.terminal.rows()?,
                ))?);
            }
            Ok(())
        })();
        self.report(result, cx);
    }
    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let link_hovered = self
            .link_at(event.position)
            .ok()
            .flatten()
            .map(|_| event.position);
        if link_hovered != self.link_hovered {
            self.link_hovered = link_hovered;
            cx.notify();
        }
        if self.link_press.as_ref().is_some_and(|(start, _)| {
            (event.position.x - start.x).abs() > px(4.)
                || (event.position.y - start.y).abs() > px(4.)
        }) {
            self.link_press = None;
        }
        let hovered = self.bounds.contains(&event.position)
            && event.position.x >= self.bounds.right() - px(scrollbar::WIDTH);
        if hovered || hovered != self.scrollbar_hovered {
            self.scrollbar_last_used = Instant::now();
            cx.notify();
        }
        self.scrollbar_hovered = hovered;
        if event.pressed_button != Some(MouseButton::Left) {
            self.scroll_drag = None;
            return;
        }
        let result = (|| -> Result<()> {
            if let Some((id, grab)) = self.scroll_drag {
                if id == self.active
                    && let Some(tab) = self.tabs.get_mut(&id)
                {
                    let bar = scrollbar::ScrollBar::new(self.bounds, tab.terminal.scrollbar()?);
                    tab.terminal
                        .scroll_viewport(ScrollViewport::Row(bar.row(event.position.y, grab)));
                }
                return Ok(());
            }
            if let Some(tab) = self.tabs.get_mut(&self.active)
                && let Some(anchor) = &tab.anchor
                && let Some(start) = anchor.snapshot(&tab.terminal)?
            {
                let end = tab.terminal.grid_ref(selection_point(
                    event.position,
                    self.bounds,
                    self.cell,
                    tab.terminal.cols()?,
                    tab.terminal.rows()?,
                ))?;
                tab.terminal
                    .set_selection(Some(&Selection::new(start, end, false)))?;
            }
            Ok(())
        })();
        self.report(result, cx);
    }
    fn scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(&self.active) {
            self.scrollbar_last_used = Instant::now();
            let delta = event.delta.pixel_delta(self.cell.height);
            tab.terminal
                .scroll_viewport(ScrollViewport::Delta(scroll_rows(
                    &mut self.scroll_remainder,
                    f32::from(delta.y) / f32::from(self.cell.height),
                )));
            cx.notify();
        }
    }
    fn paint(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.bounds = bounds;
        let bounds = Bounds::new(
            bounds.origin,
            size(
                (bounds.size.width - px(scrollbar::RESERVED_WIDTH)).max(px(1.)),
                bounds.size.height,
            ),
        );
        window.handle_input(
            &self.focus,
            ElementInputHandler::new(bounds, cx.entity()),
            cx,
        );
        let font = font(self.config.font_family.clone());
        let font_size = px(self.config.font_size);
        let font_id = window.text_system().resolve_font(&font);
        self.cell = size(
            window.text_system().advance(font_id, font_size, 'M')?.width,
            px(self.config.font_size * 1.25),
        );
        let Some(tab) = self.tabs.get_mut(&self.active) else {
            return Ok(());
        };
        let cell = self.cell;
        let dimensions = (
            (f32::from(bounds.size.width) / f32::from(cell.width))
                .floor()
                .clamp(1., 500.) as u16,
            (f32::from(bounds.size.height) / f32::from(cell.height))
                .floor()
                .clamp(1., 300.) as u16,
            f32::from(cell.width).max(1.) as u32,
            f32::from(cell.height).max(1.) as u32,
        );
        if tab.request_resize(dimensions, Instant::now()) {
            protocol::send(
                self.writer.as_mut().context("Server disconnected")?,
                &Request::Resize {
                    id: self.active,
                    cols: dimensions.0,
                    rows: dimensions.1,
                    width: dimensions.2,
                    height: dimensions.3,
                },
            )?;
        }
        if dimensions != tab.size && !tab.resize_pending {
            window.request_animation_frame();
        }
        tab.snapshot()?;
        let hovered_cells = if let Some(position) = self.link_hovered {
            links::hovered_cells(
                tab,
                selection_point(
                    position,
                    bounds,
                    cell,
                    tab.terminal.cols()?,
                    tab.terminal.rows()?,
                ),
            )?
        } else {
            0..0
        };
        let images = tab.images.prepare(&tab.terminal, bounds, cell)?;
        window.with_content_mask(Some(ContentMask { bounds }), |window| -> Result<()> {
            for image in images.iter().filter(|i| i.z < i32::MIN / 2) {
                image.paint(window)?;
            }
            for c in &tab.cells {
                if let Some(bg) = c.bg {
                    window.paint_quad(fill(
                        Bounds::new(
                            bounds.origin
                                + point(cell.width * c.x as f32, cell.height * c.y as f32),
                            cell,
                        ),
                        bg,
                    ));
                }
            }
            for image in images.iter().filter(|i| i.z < 0 && i.z >= i32::MIN / 2) {
                image.paint(window)?;
            }
            for (i, c) in tab.cells.iter().enumerate() {
                if c.text.is_empty() || c.text == " " {
                    continue;
                }
                let expand = c.narrow
                    && tab.cells.get(i + 1).is_some_and(|next| {
                        next.y == c.y && next.narrow && expand_nerd_icon(&c.text, &next.text)
                    });
                let scale = if expand { self.config.icon_scale } else { 1. };
                let origin =
                    bounds.origin + point(cell.width * c.x as f32, cell.height * c.y as f32);
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
                    font_size * scale,
                    &[run],
                    None,
                );
                let icon_bounds = Bounds::new(origin, size(cell.width * 2., cell.height));
                let origin = if expand {
                    origin
                        + point(
                            (cell.width * 2. - line.width) / 2.,
                            (cell.height - font_size * scale) / 2.,
                        )
                } else {
                    origin
                };
                if expand {
                    window.with_content_mask(
                        Some(ContentMask {
                            bounds: icon_bounds,
                        }),
                        |window| line.paint(origin, cell.height, window, cx),
                    )?;
                } else {
                    line.paint(origin, cell.height, window, cx)?;
                }
            }
            for c in &tab.cells[hovered_cells] {
                window.paint_quad(fill(
                    Bounds::new(
                        bounds.origin
                            + point(
                                cell.width * c.x as f32,
                                cell.height * (c.y + 1) as f32 - px(2.),
                            ),
                        size(cell.width, px(1.)),
                    ),
                    c.fg,
                ));
            }
            for image in images.iter().filter(|i| i.z >= 0) {
                image.paint(window)?;
            }
            if let Some((x, y)) = tab.cursor {
                window.paint_quad(fill(
                    Bounds::new(
                        bounds.origin + point(cell.width * x as f32, cell.height * y as f32),
                        size(px(2.), cell.height),
                    ),
                    rgb(0xffffff),
                ));
            }
            if !self.marked.is_empty() {
                let (x, y) = tab.cursor.unwrap_or((0, 0));
                let run = TextRun {
                    len: self.marked.len(),
                    font,
                    color: rgb(0xffffff).into(),
                    background_color: Some(rgb(0x303030).into()),
                    underline: Some(UnderlineStyle {
                        thickness: px(1.),
                        color: None,
                        wavy: false,
                    }),
                    strikethrough: None,
                };
                window
                    .text_system()
                    .shape_line(self.marked.clone().into(), font_size, &[run], None)
                    .paint(
                        bounds.origin + point(cell.width * x as f32, cell.height * y as f32),
                        cell.height,
                        window,
                        cx,
                    )?;
            }
            Ok(())
        })?;
        let scroll = tab.terminal.scrollbar()?;
        let position = (self.active, scrollbar::distance_from_bottom(scroll));
        if self.scrollbar_position != Some(position)
            || self.scroll_drag.is_some()
            || self.scrollbar_hovered
        {
            self.scrollbar_position = Some(position);
            self.scrollbar_last_used = Instant::now();
        }
        let opacity = scrollbar::opacity(self.scrollbar_last_used.elapsed());
        if cfg!(target_os = "macos") && scroll.total > scroll.len && opacity > 0. {
            window.request_animation_frame();
        }
        scrollbar::ScrollBar::new(self.bounds, scroll).paint(window, opacity);
        Ok(())
    }
    fn segments(
        &self,
        name: &str,
        title: &str,
        index: usize,
        active: bool,
        tab: Option<&TabInfo>,
    ) -> Result<Div> {
        let mut line = div().flex().items_center().flex_shrink_0();
        let segments = self
            .config
            .status_text(name, title, index, active, self.remote, tab)?;
        for (text, color, expand) in status_runs(segments) {
            let scale = if expand { self.config.icon_scale } else { 1. };
            let mut run = div()
                .flex_shrink_0()
                .text_size(px(self.config.font_size * scale))
                .text_color(color.unwrap_or(rgb(0xc6d0f5)))
                .child(text);
            if expand {
                run = run
                    .w(self.cell.width * 2.)
                    .h(px(28.))
                    .line_height(px(28.))
                    .text_center()
                    .overflow_hidden();
            }
            line = line.child(run);
        }
        Ok(line)
    }
    fn menu_action(&mut self, action: &config::MenuAction, cx: &mut Context<Self>) -> Result<()> {
        self.plus_menu_position = None;
        match action {
            config::MenuAction::NewTab => self.request(Request::NewTab),
            config::MenuAction::NewWindow => {
                let session = format!(
                    "window-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_nanos()
                );
                open_window(Config::load()?, session, self.host.clone(), cx)
            }
            config::MenuAction::Connect { host, session } => {
                open_window(Config::load()?, session.clone(), Some(host.clone()), cx)
            }
        }
    }
    fn plus_menu(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let position = self.plus_menu_position?;
        let width = px(240.).min(window.viewport_size().width);
        let row_height = px(self.config.font_size * 1.5 + 8.);
        let height = (row_height * self.config.plus_menu.len() as f32 + px(8.))
            .min(window.viewport_size().height);
        let mut menu = div()
            .id("plus-menu")
            .absolute()
            .left(
                position
                    .x
                    .min((window.viewport_size().width - width).max(px(0.))),
            )
            .top(
                position
                    .y
                    .min((window.viewport_size().height - height).max(px(0.))),
            )
            .w(width)
            .max_h(height)
            .overflow_y_scroll()
            .p(px(4.))
            .bg(rgb(0x252525))
            .border_1()
            .border_color(rgb(0x555555))
            .rounded(px(6.))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if event.button == MouseButton::Left {
                    this.plus_menu_position = None;
                    cx.notify();
                }
            }));
        for (index, entry) in self.config.plus_menu.iter().enumerate() {
            let action = entry.action.clone();
            menu = menu.child(
                div()
                    .id(("plus-menu-item", index))
                    .h(row_height)
                    .px(px(8.))
                    .line_height(row_height)
                    .overflow_hidden()
                    .cursor_pointer()
                    .rounded(px(3.))
                    .bg(if index == self.plus_menu_selected {
                        rgb(0x3b4d70)
                    } else {
                        rgb(0x252525)
                    })
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.plus_menu_selected = index;
                            cx.notify();
                        }
                    }))
                    .child(entry.label.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let result = this.menu_action(&action, cx);
                        this.focus.focus(window);
                        this.report(result, cx);
                    })),
            );
        }
        Some(menu.into_any_element())
    }
    fn header(&self, cx: &mut Context<Self>) -> Result<AnyElement> {
        let active = self.order.iter().find(|t| t.id == self.active);
        let mut bar = div()
            .id("titlebar")
            .flex()
            .items_center()
            .h(px(32.))
            .line_height(px(28.))
            .pt(px(3.5))
            .flex_shrink_0()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |event, window, _| {
                if event.click_count == 2 {
                    window.zoom_window();
                } else {
                    #[cfg(not(target_os = "windows"))]
                    window.start_window_move();
                }
            });
        if cfg!(target_os = "macos") {
            bar = bar.pl(px(78.));
        }
        if cfg!(target_os = "windows") {
            // gpui's start_window_move is a no-op on Windows. A Drag
            // window-control hitbox makes WM_NCHITTEST return HTCAPTION, so
            // titlebar clicks that no element consumes fall through to the
            // native caption drag.
            bar = bar.window_control_area(WindowControlArea::Drag);
        }
        let mut tabs = div()
            .flex()
            .items_center()
            .flex_shrink()
            .min_w_0()
            .overflow_hidden()
            .child(self.segments("left_status", "", 0, false, active)?);
        for (index, tab) in self.order.iter().enumerate() {
            let id = tab.id;
            tabs = tabs.child(
                self.segments(
                    "format_tab",
                    &tab_title(index, tab),
                    index,
                    id == self.active,
                    Some(tab),
                )?
                .id(("tab", id))
                .max_w(px(self.config.font_size * 16.))
                .overflow_hidden()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    let result = this.request(Request::Activate { id });
                    this.focus.focus(window);
                    this.report(result, cx);
                })),
            );
        }
        bar = bar.child(tabs).child(
            div()
                .id("new-tab")
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        this.plus_menu_selected = 0;
                        this.plus_menu_position = if this.config.plus_menu.is_empty() {
                            None
                        } else {
                            Some(event.position)
                        };
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .flex_shrink_0()
                .px(px(8.))
                .cursor_pointer()
                .child("+")
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _, window, cx| {
                    let result = this.request(Request::NewTab);
                    this.focus.focus(window);
                    this.report(result, cx);
                })),
        );
        bar = bar.child(div().flex_1()).child(
            self.segments("right_status", "", 0, false, active)?
                .max_w(relative(0.4))
                .overflow_hidden(),
        );
        if cfg!(target_os = "windows") {
            for (i, label) in ["−", "□", "×"].into_iter().enumerate() {
                bar = bar.child(
                    div()
                        .id(("control", i))
                        .w(px(46.))
                        .flex_shrink_0()
                        .text_center()
                        .child(label)
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(move |_, window, _| match i {
                            0 => window.minimize_window(),
                            1 => window.zoom_window(),
                            _ => window.remove_window(),
                        }),
                );
            }
        }
        Ok(bar.into_any_element())
    }
}
impl Render for Gui {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.window_bounds = Some(window.window_bounds());
        if self.error.is_none()
            && let Err(e) = self.drain(window)
        {
            self.error = Some(format!("{e:#}"));
        }
        let mut root = div()
            .relative()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.plus_menu_position.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .size_full()
            .flex()
            .flex_col()
            .text_size(px(self.config.font_size))
            .font_family(self.config.font_family.clone())
            .text_color(rgb(0xdcdcdc))
            .bg(Rgba {
                a: self.config.background_opacity,
                ..rgb(0)
            })
            .on_key_down(cx.listener(Self::key_down));
        match self.header(cx) {
            Ok(header) => root = root.child(header),
            Err(e) => self.error = Some(format!("{e:#}")),
        }
        if let Some(error) = &self.error {
            let error = error.clone();
            return root.child(
                div()
                    .p(px(24.))
                    .child(error.clone())
                    .children(self.remote_restart.as_ref().map(|_| {
                        div()
                            .id("restart-remote-daemon")
                            .mt(px(16.))
                            .p(px(8.))
                            .bg(rgb(0x803030))
                            .cursor_pointer()
                            .child("Restart with bundled server — ends ALL remote sessions")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let Some(warning) = this.remote_restart.take() else {
                                    return;
                                };
                                let result = warning
                                    .restart()
                                    .and_then(|_| this.connect(this.host.clone()));
                                if result.is_ok() {
                                    this.error = None;
                                }
                                this.report(result, cx);
                            }))
                    }))
                    .child(
                        div()
                            .id("copy-error")
                            .child("Copy error")
                            .cursor_pointer()
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(error.clone()))
                            }),
                    )
                    .child(
                        div()
                            .id("close-error")
                            .child("Close window")
                            .cursor_pointer()
                            .on_click(|_, window, _| window.remove_window()),
                    ),
            );
        }
        if self.tabs.get(&self.active).is_some_and(|tab| tab.exited) {
            root = root.child(
                div()
                    .flex_shrink_0()
                    .child("Shell exited. Close the window to quit."),
            );
        }
        let entity = cx.entity();
        let menu = self.plus_menu(window, cx);
        root.child(
            div()
                .id("terminal")
                // The root cannot track focus: its focus-transfer listener
                // prevent_defaults every mouse down (including the titlebar's
                // NC downs), which blocks the native caption drag.
                .track_focus(&self.focus)
                .flex_1()
                .overflow_hidden()
                .cursor(if self.link_hovered.is_some() {
                    CursorStyle::PointingHand
                } else {
                    CursorStyle::IBeam
                })
                .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _, _, _| {
                        this.scroll_drag = None;
                        this.link_press = None;
                    }),
                )
                .on_mouse_move(cx.listener(Self::mouse_move))
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    if !hovered && this.link_hovered.take().is_some() {
                        cx.notify();
                    }
                    if !hovered && this.scrollbar_hovered {
                        this.scrollbar_hovered = false;
                        this.scrollbar_last_used = Instant::now();
                        cx.notify();
                    }
                }))
                .on_scroll_wheel(cx.listener(Self::scroll))
                .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                    let paths: Vec<_> = paths.paths().iter().map(|p| p.as_path()).collect();
                    let result = dropped_paths(&paths).and_then(|text| this.paste(&text));
                    this.report(result, cx);
                }))
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, cx| {
                            entity.update(cx, |this, cx| {
                                let result = this.paint(bounds, window, cx);
                                if let Err(e) = result {
                                    this.error = Some(format!("{e:#}"));
                                    cx.notify();
                                }
                            });
                        },
                    )
                    .size_full(),
                ),
        )
        .children(menu)
    }
}
impl EntityInputHandler for Gui {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        *actual = Some(range.clone());
        Some(
            self.marked
                .encode_utf16()
                .skip(range.start)
                .take(range.len())
                .collect::<Vec<_>>(),
        )
        .map(|v| String::from_utf16_lossy(&v))
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.marked_selection.clone(),
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        (!self.marked.is_empty()).then(|| 0..self.marked.encode_utf16().count())
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked.clear();
        self.marked_selection = 0..0;
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked.clear();
        self.marked_selection = 0..0;
        let result = if self.tabs.contains_key(&self.active) {
            self.input(text.as_bytes())
        } else {
            Ok(())
        };
        self.report(result, cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut units: Vec<_> = self.marked.encode_utf16().collect();
        let range = range.unwrap_or(0..units.len());
        let start = range.start.min(units.len());
        let end = range.end.max(start).min(units.len());
        units.splice(start..end, text.encode_utf16());
        self.marked = String::from_utf16_lossy(&units);
        self.marked_selection = selected
            .map(|r| start + r.start..start + r.end)
            .unwrap_or_else(|| {
                let end = self.marked.encode_utf16().count();
                end..end
            });
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let (x, y) = self.tabs.get(&self.active)?.cursor?;
        Some(Bounds::new(
            self.bounds.origin + point(self.cell.width * x as f32, self.cell.height * y as f32),
            self.cell,
        ))
    }
    fn character_index_for_point(
        &mut self,
        _: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(0)
    }
}
impl Drop for Gui {
    fn drop(&mut self) {
        if let Some(bounds) = self.window_bounds
            && let Err(e) = window_state::save(bounds)
        {
            eprintln!("Cannot save window geometry: {e:#}");
        }
        if let Some(writer) = &self.writer {
            let _ = writer.shutdown(std::net::Shutdown::Both);
        }
    }
}

fn zoomed_font_size(current: f32, initial: f32, stroke: &Keystroke) -> Option<f32> {
    let mods = stroke.modifiers;
    if mods.alt || !(mods.control || (cfg!(target_os = "macos") && mods.platform)) {
        return None;
    }
    let step = match stroke.key.as_str() {
        "0" => return Some(initial),
        "+" | "=" => 1.,
        "-" | "_" => -1.,
        _ => return None,
    };
    Some((current + step).clamp(6., 96.))
}

fn scroll_rows(remainder: &mut f32, delta: f32) -> isize {
    // GPUI's content-motion delta has the opposite sign to Ghostty's viewport delta.
    *remainder -= delta;
    let rows = remainder.trunc() as isize;
    *remainder -= rows as f32;
    rows
}

fn hold_frame(terminal: &Terminal, since: &mut Option<Instant>) -> Result<bool> {
    if !terminal.mode(ghostty_vt::terminal::Mode::SYNC_OUTPUT)? {
        *since = None;
        return Ok(false);
    }
    Ok(since.get_or_insert_with(Instant::now).elapsed() < Duration::from_secs(1))
}
fn resize_terminal(
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
fn dropped_paths(paths: &[&std::path::Path]) -> Result<String> {
    paths
        .iter()
        .map(|p| {
            Ok(format!(
                "'{}'",
                p.to_str()
                    .context("Dropped file path is not valid UTF-8")?
                    .replace('\'', "'\\''")
            ))
        })
        .collect::<Result<Vec<_>>>()
        .map(|p| p.join(" ") + " ")
}
fn is_nerd_icon(ch: char) -> bool {
    matches!(ch, '\u{e000}'..='\u{f8ff}' | '\u{f0000}'..='\u{ffffd}' | '\u{100000}'..='\u{10fffd}')
}
fn status_runs(segments: Vec<(String, Option<Rgba>)>) -> Vec<(String, Option<Rgba>, bool)> {
    let mut chars = segments
        .into_iter()
        .flat_map(|(text, color)| text.chars().map(|ch| (ch, color)).collect::<Vec<_>>())
        .peekable();
    let mut runs: Vec<(String, Option<Rgba>, bool)> = Vec::new();
    while let Some((ch, color)) = chars.next() {
        let expand = is_nerd_icon(ch) && chars.peek().is_some_and(|(next, _)| *next == ' ');
        if expand {
            chars.next();
            runs.push((ch.to_string(), color, true));
        } else if let Some((text, previous_color, false)) = runs.last_mut()
            && *previous_color == color
        {
            text.push(ch);
        } else {
            runs.push((ch.to_string(), color, false));
        }
    }
    runs
}
fn expand_nerd_icon(text: &str, next: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(is_nerd_icon)
        && chars.next().is_none()
        && (next.is_empty() || next == " ")
}
fn selection_point(
    pos: gpui::Point<Pixels>,
    bounds: Bounds<Pixels>,
    cell: Size<Pixels>,
    cols: u16,
    rows: u16,
) -> TerminalPoint {
    TerminalPoint::Viewport(PointCoordinate {
        x: (f32::from(pos.x - bounds.origin.x) / f32::from(cell.width))
            .floor()
            .clamp(0., f32::from(cols.saturating_sub(1))) as u16,
        y: (f32::from(pos.y - bounds.origin.y) / f32::from(cell.height))
            .floor()
            .clamp(0., f32::from(rows.saturating_sub(1))) as u32,
    })
}
fn selected_text(terminal: &Terminal) -> Result<Option<String>> {
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
fn terminal_key(k: &str) -> Option<key::Key> {
    use key::Key as K;
    Some(match k {
        "enter" => K::Enter,
        "backspace" => K::Backspace,
        "tab" => K::Tab,
        "escape" => K::Escape,
        "up" => K::ArrowUp,
        "down" => K::ArrowDown,
        "left" => K::ArrowLeft,
        "right" => K::ArrowRight,
        "home" => K::Home,
        "end" => K::End,
        "pageup" => K::PageUp,
        "pagedown" => K::PageDown,
        "insert" => K::Insert,
        "delete" => K::Delete,
        "f1" => K::F1,
        "f2" => K::F2,
        "f3" => K::F3,
        "f4" => K::F4,
        "f5" => K::F5,
        "f6" => K::F6,
        "f7" => K::F7,
        "f8" => K::F8,
        "f9" => K::F9,
        "f10" => K::F10,
        "f11" => K::F11,
        "f12" => K::F12,
        _ => return None,
    })
}
fn open_window(
    config: Config,
    session: String,
    host: Option<String>,
    cx: &mut gpui::App,
) -> Result<()> {
    let window_size = size(px(config.window_width), px(config.window_height));
    let options = WindowOptions {
        window_bounds: Some(
            window_state::load(cx)
                .unwrap_or_else(|| WindowBounds::Windowed(Bounds::centered(None, window_size, cx))),
        ),
        titlebar: Some(TitlebarOptions {
            title: Some("Scytty".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(10.))),
        }),
        window_background: WindowBackgroundAppearance::Transparent,
        window_min_size: Some(size(px(200.), px(100.))),
        ..Default::default()
    };
    cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| Gui::new(config, session, host, cx));
        view.read(cx).focus.focus(window);
        view
    })?;
    Ok(())
}

fn main() -> Result<()> {
    let config = Config::load()?;
    let mut args = std::env::args().skip(1);
    let argument = args.next();
    let (host, session) = if argument.as_deref() == Some("--ssh") {
        (
            Some(
                args.next()
                    .context("usage: scytty --ssh user@host [session]")?,
            ),
            args.next().unwrap_or_else(|| "default".into()),
        )
    } else {
        (None, argument.unwrap_or_else(|| "default".into()))
    };
    Application::new().run(move |cx| {
        if let Err(e) = open_window(config, session, host, cx) {
            eprintln!("Cannot open Scytty: {e:#}");
            cx.quit();
        }
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use ghostty_vt::TerminalOptions;
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
        assert!(tab.request_resize(first, now));
        assert!(!tab.request_resize(intermediate, now + Duration::from_millis(10)));
        assert!(!tab.request_resize(latest, now + Duration::from_millis(20)));
        assert!(!tab.request_resize(latest, now + Duration::from_millis(200)));
        tab.size = first;
        tab.resize_pending = false;
        assert!(tab.request_resize(latest, now + Duration::from_millis(200)));
        tab.size = latest;
        tab.resize_pending = false;
        assert!(!tab.request_resize(latest, now + Duration::from_millis(210)));
        assert!(!tab.request_resize(intermediate, now + Duration::from_millis(220)));
        assert!(!tab.request_resize(intermediate, now + Duration::from_millis(250)));
        assert!(tab.request_resize(intermediate, now + Duration::from_millis(300)));
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
    #[test]
    fn status_icons_expand_only_into_a_following_space() {
        let red = Some(rgb(0xff0000));
        assert_eq!(
            status_runs(vec![
                ("a\u{301} \u{e0b0}main\u{f0001}".into(), red),
                (" next\u{e0b0}".into(), None),
            ]),
            vec![
                ("a\u{301} \u{e0b0}main".into(), red, false),
                ("\u{f0001}".into(), red, true),
                ("next\u{e0b0}".into(), None, false),
            ]
        );
        assert_eq!(
            status_runs(vec![("\u{e0b0}  x".into(), None)]),
            vec![("\u{e0b0}".into(), None, true), (" x".into(), None, false),]
        );
        assert!(status_runs(vec![]).is_empty());
    }

    #[test]
    fn zoom_shortcuts_and_limits() {
        let stroke = |key: &str, control| Keystroke {
            key: key.into(),
            key_char: None,
            modifiers: Modifiers {
                control,
                ..Default::default()
            },
        };
        assert_eq!(zoomed_font_size(16., 18., &stroke("+", true)), Some(17.));
        assert_eq!(zoomed_font_size(16., 18., &stroke("=", true)), Some(17.));
        assert_eq!(zoomed_font_size(16., 18., &stroke("-", true)), Some(15.));
        assert_eq!(zoomed_font_size(6., 18., &stroke("-", true)), Some(6.));
        assert_eq!(zoomed_font_size(96., 18., &stroke("+", true)), Some(96.));
        assert_eq!(zoomed_font_size(16., 18., &stroke("+", false)), None);
        assert_eq!(zoomed_font_size(16., 18., &stroke("c", true)), None);
        assert_eq!(zoomed_font_size(24., 18., &stroke("0", true)), Some(18.));
        assert_eq!(zoomed_font_size(24., 18., &stroke("0", false)), None);
        let mut command_zero = stroke("0", false);
        command_zero.modifiers.platform = true;
        assert_eq!(
            zoomed_font_size(24., 18., &command_zero),
            cfg!(target_os = "macos").then_some(18.)
        );
    }

    #[test]
    fn fractional_trackpad_scroll_accumulates() {
        let mut remainder = 0.;
        assert_eq!(scroll_rows(&mut remainder, 0.25), 0);
        assert_eq!(scroll_rows(&mut remainder, 0.75), -1);
        assert_eq!(scroll_rows(&mut remainder, -0.5), 0);
        assert_eq!(scroll_rows(&mut remainder, -1.5), 2);
    }

    #[test]
    fn paths_icons_and_navigation() -> Result<()> {
        assert_eq!(
            dropped_paths(&[std::path::Path::new("/tmp/it's;$x.png")])?,
            "'/tmp/it'\\''s;$x.png' "
        );
        assert!(expand_nerd_icon("\u{e0b0}", " "));
        assert!(!expand_nerd_icon("\u{e0b0}", "x"));
        let tabs: Vec<_> = [4, 8, 12]
            .into_iter()
            .map(|id| TabInfo {
                id,
                ..Default::default()
            })
            .collect();
        assert_eq!(adjacent_tab(&tabs, 4, -1), Some(12));
        assert_eq!(adjacent_tab(&tabs, 12, 1), Some(4));
        assert_eq!(adjacent_tab(&[], 0, 1), None);
        Ok(())
    }
}
