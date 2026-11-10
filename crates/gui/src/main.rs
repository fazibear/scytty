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
mod clipboard;
mod config;
mod find;
mod fonts;
mod images;
mod links;
mod notifications;
mod platform;
mod pointer;
mod reconnect;
mod remote;
mod scrollbar;
mod semantic;
mod terminal_paint;
mod transport;
mod window_state;
use config::Config;

#[derive(Clone, PartialEq)]
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
    placements: Vec<images::Placement>,
    cells: Vec<Cell>,
    paint_cache: terminal_paint::Cache,
    search: Option<ghostty_vt::search::Search>,
    cursor: Option<(u16, u16)>,
    cursor_color: Rgba,
    default_colors: Option<(ghostty_vt::style::RgbColor, ghostty_vt::style::RgbColor)>,
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
    fn background(&mut self) -> Result<Rgba> {
        let background = if hold_frame(&self.terminal, &mut self.hold_since)? {
            self.default_colors.map(|(_, background)| background)
        } else {
            self.terminal.bg_color()?
        };
        Ok(color(background, rgb(0)))
    }
    fn prepare_frame(&mut self, bounds: Bounds<Pixels>, cell: Size<Pixels>) -> Result<()> {
        if self.snapshot()? {
            self.placements = self.images.prepare(&self.terminal, bounds, cell)?;
        }
        Ok(())
    }
    // Images must commit with text, even when only the image placements changed.
    fn snapshot(&mut self) -> Result<bool> {
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

fn message_line_rows(line: &str, breaks: impl Iterator<Item = usize>) -> Vec<String> {
    let mut start = 0;
    breaks
        .chain(std::iter::once(line.len()))
        .map(|end| {
            let row = line[start..end].trim().to_owned();
            start = end;
            row
        })
        .collect()
}

struct Connected {
    connection: transport::Connection,
    writer: Stream,
    updates: Receiver<Result<Update>>,
    wakeups: async_channel::Receiver<()>,
}

struct Gui {
    config: Config,
    initial_font_size: f32,
    session: String,
    bin: Option<String>,
    remote: bool,
    host: Option<String>,
    plus_menu_position: Option<gpui::Point<Pixels>>,
    plus_menu_selected: usize,
    connection: Option<transport::Connection>,
    connecting: bool,
    has_connected: bool,
    reconnect: Option<reconnect::Reconnect>,
    writer: Option<Stream>,
    updates: Option<Receiver<Result<Update>>>,
    window_bounds: Option<WindowBounds>,
    tabs: HashMap<u64, Tab>,
    order: Vec<TabInfo>,
    active: u64,
    error: Option<String>,
    remote_restart: Option<remote::RestartRequired>,
    focus: FocusHandle,
    window_handle: AnyWindowHandle,
    bounds: Bounds<Pixels>,
    cell: Size<Pixels>,
    find: Option<find::Find>,
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
        bin: Option<String>,
        window_handle: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        let font = fonts::resolve(&config.font_family);
        let error = font.as_ref().err().map(|e| format!("{e:#}"));
        config.font_family = font.unwrap_or_else(|_| "monospace".into());
        let mut view = Self {
            window_handle,
            initial_font_size: config.font_size,
            config,
            session,
            bin,
            remote: host.is_some(),
            host: host.clone(),
            plus_menu_position: None,
            plus_menu_selected: 0,
            connection: None,
            connecting: false,
            has_connected: false,
            reconnect: None,
            writer: None,
            updates: None,
            window_bounds: None,
            tabs: HashMap::new(),
            order: vec![],
            active: 0,
            error,
            remote_restart: None,
            focus: cx.focus_handle(),
            bounds: Bounds::default(),
            cell: size(px(8.), px(20.)),
            find: None,
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
        if view.error.is_none() {
            view.start_connect(None, cx);
        }
        cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if view
                    .update(cx, |view, cx| {
                        if view.error.is_some() {
                            return;
                        }
                        if view
                            .reconnect
                            .as_mut()
                            .is_some_and(|retry| retry.start_if_due(Instant::now()))
                        {
                            view.start_connect(None, cx);
                        }
                        cx.notify();
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
    fn start_connect(&mut self, restart: Option<remote::RestartRequired>, cx: &mut Context<Self>) {
        self.connecting = true;
        self.error = None;
        self.remote_restart = None;
        self.updates = None;
        self.writer = None;
        self.connection = None;
        let host = self.host.clone();
        let session = self.session.clone();
        let bin = self.bin.clone();
        let task = cx.background_executor().spawn(async move {
            if let Some(warning) = restart {
                warning.restart()?;
            }
            Self::connect(host, session, bin)
        });
        cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                match result {
                    Ok(connected) => {
                        view.connection = Some(connected.connection);
                        view.writer = Some(connected.writer);
                        view.updates = Some(connected.updates);
                        let wakeups = connected.wakeups;
                        let window_handle = view.window_handle;
                        cx.spawn(async move |view, cx| {
                            loop {
                                let closed = wakeups.recv().await.is_err();
                                let result = window_handle.update(cx, |_, window, cx| {
                                    view.update(cx, |view, cx| {
                                        // Hidden windows may not render; never tie socket reads to paint.
                                        if view.error.is_none() {
                                            let result = view.drain(window, cx);
                                            view.report(result, cx);
                                        }
                                        cx.notify();
                                    })
                                });
                                if result.and_then(|result| result).is_err() || closed {
                                    break;
                                }
                            }
                        })
                        .detach();
                    }
                    Err(error) => {
                        if view.reconnect.is_some()
                            && error.downcast_ref::<remote::RestartRequired>().is_none()
                            && error
                                .downcast_ref::<reconnect::IncompatiblePeer>()
                                .is_none()
                        {
                            view.retry_connection(cx);
                        } else {
                            view.report(Err(error), cx);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn connect(host: Option<String>, session: String, bin: Option<String>) -> Result<Connected> {
        let connection = match host.as_deref() {
            Some(host) => transport::connect(host)?,
            None => platform::connect_local()?,
        };
        Self::attach(connection, session, bin).map_err(
            |error| match remote::restart_after_mismatch(host.as_deref()) {
                Err(warning) if warning.downcast_ref::<remote::RestartRequired>().is_some() => {
                    warning.context(format!("{error:#}"))
                }
                _ => error,
            },
        )
    }

    fn attach(
        connection: transport::Connection,
        session: String,
        bin: Option<String>,
    ) -> Result<Connected> {
        let mut writer = connection.socket.try_clone()?;
        writer.set_write_timeout(Some(Duration::from_secs(5)))?;
        writer.set_read_timeout(Some(Duration::from_secs(5)))?;
        protocol::handshake(&mut writer).map_err(|error| {
            if reconnect::transport_error(&error) {
                error
            } else {
                error.context(reconnect::IncompatiblePeer)
            }
        })?;
        writer.set_read_timeout(None)?;
        protocol::send(&mut writer, &Request::Attach { session, bin })?;
        let mut reader = writer.try_clone()?;
        let (tx, updates) = mpsc::sync_channel(256);
        let (wake, wakeups) = async_channel::bounded(1);
        thread::spawn(move || {
            loop {
                let update = protocol::receive(&mut reader).context("Server connection closed or sent an incomplete frame; check SSH output and ~/.scytty/server.log on the server");
                let failed = update.is_err();
                if tx.send(update).is_err() {
                    break;
                }
                let _ = wake.try_send(());
                if failed {
                    break;
                }
            }
        });
        Ok(Connected {
            connection,
            writer,
            updates,
            wakeups,
        })
    }
    fn request(&mut self, request: Request) -> Result<()> {
        if self.connecting {
            return Ok(());
        }
        protocol::send(
            self.writer.as_mut().context("Server is not connected")?,
            &request,
        )
    }
    fn input(&mut self, bytes: &[u8]) -> Result<()> {
        if self.connecting {
            return Ok(());
        }
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
        if let Some(find) = &mut self.find {
            find.replace(None, text);
            return Ok(());
        }
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
    fn retry_connection(&mut self, cx: &mut Context<Self>) {
        self.connecting = true;
        self.error = None;
        self.updates = None;
        self.writer = None;
        self.connection = None;
        self.end_mouse_drag();
        self.link_press = None;
        self.marked.clear();
        self.marked_selection = 0..0;
        match &mut self.reconnect {
            Some(retry) => retry.failed(Instant::now()),
            None => self.reconnect = Some(reconnect::Reconnect::new(Instant::now())),
        }
        cx.notify();
    }
    fn report(&mut self, result: Result<()>, cx: &mut Context<Self>) {
        if let Err(e) = result {
            if self.has_connected
                && reconnect::transport_error(&e)
                && e.downcast_ref::<remote::RestartRequired>().is_none()
            {
                self.retry_connection(cx);
                return;
            }
            self.reconnect = None;
            self.connecting = false;
            self.remote_restart = e.downcast_ref::<remote::RestartRequired>().cloned();
            self.error = Some(format!("{e:#}"));
        }
        cx.notify();
    }
    fn drain(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<()> {
        for _ in 0..256 {
            let Some(rx) = &self.updates else { break };
            let update = match rx.try_recv() {
                Ok(update) => update?,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(_) => {
                    return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into());
                }
            };
            match update {
                Update::Snapshot { id, bytes } => {
                    let mut terminal =
                        ghostty_vt::snapshot::Decoder::from_slice(&bytes).decode()?;
                    terminal.set_scrollback_max_bytes(Some(
                        usize::from(self.config.scrollback_mb) * 1024 * 1024,
                    ))?;
                    self.tabs.insert(id, Tab::new(terminal)?);
                    if self.connecting {
                        self.has_connected = true;
                        self.reconnect = None;
                        self.connecting = false;
                        // A rejected attach must not close with an unread configuration request.
                        self.request(Request::SetScrollback {
                            megabytes: self.config.scrollback_mb,
                        })?;
                    }
                }
                Update::Output { id, bytes } => {
                    if let Some(tab) = self.tabs.get_mut(&id) {
                        tab.terminal.vt_write(&bytes);
                        for event in tab.terminal.events() {
                            if let ghostty_vt::terminal::Event::ClipboardWrite(write) = event {
                                if let Some(item) = clipboard::item(write) {
                                    cx.write_to_clipboard(item);
                                }
                            } else {
                                notifications::handle(event);
                            }
                        }
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
                    let result = self.menu_action(&action, window, cx);
                    self.report(result, cx);
                }
                _ => {}
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let m = stroke.modifiers;
        if platform::clipboard_modifier(m) && stroke.key == "f" {
            let find = self.find.get_or_insert_with(Default::default);
            find.selection = 0..find.query.len();
            find.reversed = false;
            self.marked.clear();
            self.marked_selection = 0..0;
            self.focus.focus(window);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if let Some(find) = &mut self.find {
            let handled = match stroke.key.as_str() {
                "escape" => {
                    self.find = None;
                    for tab in self.tabs.values_mut() {
                        tab.search = None;
                    }
                    true
                }
                "enter" => {
                    let result = self.find_step(m.shift);
                    self.report(result, cx);
                    true
                }
                "a" if platform::command_modifier(m) => {
                    find.selection = 0..find.query.len();
                    find.reversed = false;
                    true
                }
                "c" if platform::clipboard_modifier(m) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        find.query[find.selection.clone()].to_owned(),
                    ));
                    true
                }
                "v" if platform::clipboard_modifier(m) => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        find.replace(None, &text);
                    }
                    true
                }
                key => find.edit(key, m.shift),
            };
            if handled {
                cx.stop_propagation();
                cx.notify();
            }
            return;
        }
        let command = platform::command_modifier(m);
        let result = if let Some(size) =
            zoomed_font_size(self.config.font_size, self.initial_font_size, stroke)
        {
            self.config.font_size = size;
            Some(Ok(()))
        } else if command && stroke.key == "t" {
            Some(self.request(Request::NewTab))
        } else if platform::clipboard_modifier(m) && matches!(stroke.key.as_str(), "up" | "down") {
            Some((|| -> Result<()> {
                if let Some(tab) = self.tabs.get_mut(&self.active)
                    && let Some(row) = semantic::prompt_row(&tab.terminal, stroke.key == "up")?
                {
                    tab.terminal.scroll_viewport(ScrollViewport::Row(row));
                    self.scrollbar_last_used = Instant::now();
                }
                Ok(())
            })())
        } else if command && matches!(stroke.key.as_str(), "left" | "right") {
            adjacent_tab(
                &self.order,
                self.active,
                if stroke.key == "left" { -1 } else { 1 },
            )
            .map(|id| self.request(Request::Activate { id }))
        } else if platform::clipboard_modifier(m) && stroke.key == "c" {
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
        } else if platform::clipboard_modifier(m) && stroke.key == "v" {
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
    fn find_step(&mut self, previous: bool) -> Result<()> {
        let Some(find) = &self.find else {
            return Ok(());
        };
        let Some(tab) = self.tabs.get_mut(&self.active) else {
            return Ok(());
        };
        if tab.search.is_none() {
            tab.search = Some(ghostty_vt::search::Search::new(&tab.terminal)?);
        }
        let search = tab.search.as_mut().unwrap();
        find::advance(search, &tab.terminal, &find.query)?;
        if previous {
            search.select_prev(&tab.terminal)?;
        } else {
            search.select_next(&tab.terminal)?;
        }
        Ok(())
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
    fn end_mouse_drag(&mut self) {
        self.scroll_drag = None;
        for tab in self.tabs.values_mut() {
            tab.anchor = None;
        }
    }
    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.end_mouse_drag();
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
                if platform::command_modifier(event.modifiers) && event.modifiers.shift {
                    let point = selection_point(
                        event.position,
                        self.bounds,
                        self.cell,
                        tab.terminal.cols()?,
                        tab.terminal.rows()?,
                    );
                    let selection = tab.terminal.select_output(tab.terminal.grid_ref(point)?)?;
                    tab.terminal.set_selection(selection.as_ref())?;
                    tab.anchor = None;
                    self.link_press = None;
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
            self.end_mouse_drag();
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
        let highlights = if let Some(find) = &mut self.find {
            if tab.search.is_none() {
                tab.search = Some(ghostty_vt::search::Search::new(&tab.terminal)?);
            }
            let search = tab.search.as_mut().unwrap();
            let running = find::advance(search, &tab.terminal, &find.query)?;
            if search.selected_index()?.is_none() && search.total_matches()? > 0 {
                search.select_next(&tab.terminal)?;
                search.feed(&tab.terminal)?;
            }
            let stats = (search.total_matches()?, search.selected_index()?, running);
            if find.stats != stats {
                find.stats = stats;
                cx.notify();
            }
            if running {
                window.request_animation_frame();
            }
            find::highlights(search, &tab.terminal)?
        } else {
            vec![]
        };
        tab.prepare_frame(bounds, cell)?;
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
        tab.paint_cache.prepare(
            &tab.cells,
            &font,
            font_size,
            cell,
            self.config.icon_scale,
            window,
        );
        let images = &tab.placements;
        window.with_content_mask(Some(ContentMask { bounds }), |window| -> Result<()> {
            for image in images.iter().filter(|i| i.z < i32::MIN / 2) {
                image.paint(window)?;
            }
            tab.paint_cache.backgrounds(bounds.origin, cell, window);
            for highlight in &highlights {
                window.paint_quad(fill(
                    Bounds::new(
                        bounds.origin
                            + point(
                                cell.width * highlight.x as f32,
                                cell.height * highlight.y as f32,
                            ),
                        size(cell.width * highlight.width as f32, cell.height),
                    ),
                    if highlight.selected {
                        rgba(0xb36b00dd)
                    } else {
                        rgba(0x80700099)
                    },
                ));
            }
            for image in images.iter().filter(|i| i.z < 0 && i.z >= i32::MIN / 2) {
                image.paint(window)?;
            }
            tab.paint_cache.text(
                bounds.origin,
                cell,
                font_size,
                self.config.icon_scale,
                window,
                cx,
            )?;
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
                    tab.cursor_color,
                ));
            }
            if self.find.is_none() && !self.marked.is_empty() {
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
        if platform::OVERLAY_SCROLLBAR && scroll.total > scroll.len && opacity > 0. {
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
    fn menu_action(
        &mut self,
        action: &config::MenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.plus_menu_position = None;
        match action {
            config::MenuAction::NewTab => self.request(Request::NewTab),
            config::MenuAction::ReloadConfig => {
                let mut config = Config::load()?;
                config.font_family = fonts::resolve(&config.font_family)?;
                let title = self
                    .order
                    .iter()
                    .enumerate()
                    .find(|(_, tab)| tab.id == self.active)
                    .map_or_else(|| "scytty".into(), |(index, tab)| tab_title(index, tab));
                let title = config.window_title(&self.session, &title, self.remote)?;
                if config.scrollback_mb != self.config.scrollback_mb {
                    if self.writer.is_some() && !self.connecting {
                        self.request(Request::SetScrollback {
                            megabytes: config.scrollback_mb,
                        })?;
                    }
                    for tab in self.tabs.values_mut() {
                        tab.terminal.set_scrollback_max_bytes(Some(
                            usize::from(config.scrollback_mb) * 1024 * 1024,
                        ))?;
                    }
                }
                self.initial_font_size = config.font_size;
                self.config = config;
                window.set_window_title(&title);
                if self.writer.is_some() && !self.connecting {
                    self.error = None;
                }
                cx.notify();
                Ok(())
            }
            config::MenuAction::NewWindow => {
                let session = format!(
                    "window-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_nanos()
                );
                open_window(
                    Config::load()?,
                    session,
                    self.host.clone(),
                    self.bin.clone(),
                    cx,
                )
            }
            config::MenuAction::Connect { host, session, bin } => open_window(
                Config::load()?,
                session.clone(),
                Some(host.clone()),
                bin.clone(),
                cx,
            ),
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
                        let result = this.menu_action(&action, window, cx);
                        this.focus.focus(window);
                        this.report(result, cx);
                    })),
            );
        }
        Some(menu.into_any_element())
    }
    fn header(&self, cx: &mut Context<Self>) -> Result<AnyElement> {
        let active = self.order.iter().find(|t| t.id == self.active);
        let mut bar = platform::titlebar();
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
        Ok(platform::window_controls(bar).into_any_element())
    }
}
impl Render for Gui {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.window_bounds = Some(window.window_bounds());
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
                ..self
                    .tabs
                    .get_mut(&self.active)
                    .and_then(|tab| tab.background().ok())
                    .unwrap_or_else(|| rgb(0))
            })
            .on_key_down(cx.listener(Self::key_down));
        match self.header(cx) {
            Ok(header) => root = root.child(header),
            Err(e) => {
                self.connecting = false;
                self.error = Some(format!("{e:#}"));
            }
        }
        let mut find_overlay = None;
        if let Some(find) = &self.find {
            let (count, selected, running) = find.stats;
            let before = find.query[..find.selection.start].to_owned();
            let selected_text = find.query[find.selection.clone()].to_owned();
            let after = find.query[find.selection.end..].to_owned();
            find_overlay = Some(
                div()
                    .absolute()
                    .top(px(36.))
                    .right(px(8.))
                    .w(px(420.))
                    .max_w((window.viewport_size().width - px(16.)).max(px(0.)))
                    .occlude()
                    .flex()
                    .items_center()
                    .h(px(32.))
                    .px(px(8.))
                    .gap(px(8.))
                    .bg(rgb(0x28202f))
                    .child("Find:")
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(before)
                            .child(div().bg(rgb(0x604080)).child(if selected_text.is_empty() {
                                "▏".to_owned()
                            } else {
                                selected_text
                            }))
                            .child(after),
                    )
                    .child(format!(
                        "{} / {}{}",
                        selected.map_or(0, |i| i + 1),
                        count,
                        if running { "…" } else { "" }
                    ))
                    .child(
                        div()
                            .id("find-previous")
                            .cursor_pointer()
                            .child("↑")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    // Ghostty's next match is older content (up).
                                    let result = this.find_step(false);
                                    this.report(result, cx);
                                }),
                            ),
                    )
                    .child(
                        div()
                            .id("find-next")
                            .cursor_pointer()
                            .child("↓")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    let result = this.find_step(true);
                                    this.report(result, cx);
                                }),
                            ),
                    )
                    .child(
                        div()
                            .id("find-close")
                            .cursor_pointer()
                            .child("×")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.find = None;
                                    for tab in this.tabs.values_mut() {
                                        tab.search = None;
                                    }
                                    cx.notify();
                                }),
                            ),
                    ),
            );
        }
        if let Some(retry) = &self.reconnect {
            root = root.child(
                div()
                    .flex_shrink_0()
                    .px(px(8.))
                    .py(px(4.))
                    .bg(rgb(0x38204f))
                    .child(format!(
                        "Reconnecting to {}… (attempt {}) — input paused",
                        self.host.as_deref().unwrap_or("local server"),
                        retry.attempts
                    )),
            );
        }
        if let Some(error) = self.error.clone().or_else(|| {
            (self.connecting && self.reconnect.is_none()).then(|| {
                format!(
                    "Connecting to {}…",
                    self.host.as_deref().unwrap_or("local server")
                )
            })
        }) {
            let bubble_width = (window.viewport_size().width - px(48.)).clamp(px(41.), px(640.));
            let text_width = bubble_width - px(40.);
            let mut wrapper = window
                .text_system()
                .line_wrapper(font(self.config.font_family.clone()).bold(), px(16.));
            let rows: Vec<_> = error
                .split('\n')
                .flat_map(|line| {
                    let fragments = [LineFragment::text(line)];
                    message_line_rows(
                        line,
                        wrapper
                            .wrap_line(&fragments, text_width)
                            .map(|boundary| boundary.ix),
                    )
                })
                .collect();
            return root.child(
                div()
                    .id("error-screen")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .flex_1()
                            .flex_shrink_0()
                            .p(px(24.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .id("error-message")
                                    .text_center()
                                    .w(bubble_width)
                                    .max_h(px(240.))
                                    .flex_shrink_0()
                                    .overflow_y_scroll()
                                    .p(px(18.))
                                    .rounded_xl()
                                    .bg(rgb(0x38204f))
                                    .border_2()
                                    .border_color(rgb(0xac50f5))
                                    .text_color(rgb(0xffffff))
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(px(16.))
                                    .line_height(px(24.))
                                    .shadow_lg()
                                    .flex()
                                    .flex_col()
                                    .children(rows.into_iter().map(|line| {
                                        div()
                                            .flex()
                                            .flex_col()
                                            .flex_shrink_0()
                                            .w(text_width)
                                            .h(px(24.))
                                            .whitespace_nowrap()
                                            .text_center()
                                            .child(line)
                                    })),
                            )
                            .child(
                                canvas(
                                    |_, _, _| (),
                                    |bounds, _, window, _| {
                                        let mut tail = PathBuilder::fill();
                                        tail.move_to(bounds.origin);
                                        tail.line_to(
                                            bounds.origin + point(bounds.size.width, px(0.)),
                                        );
                                        tail.line_to(
                                            bounds.origin
                                                + point(
                                                    bounds.size.width * 0.5,
                                                    bounds.size.height,
                                                ),
                                        );
                                        tail.close();
                                        window.paint_path(
                                            tail.build().expect("speech bubble triangle is valid"),
                                            rgb(0xac50f5),
                                        );
                                        let mut inner = PathBuilder::fill();
                                        inner.move_to(bounds.origin + point(px(3.), px(0.)));
                                        inner.line_to(
                                            bounds.origin
                                                + point(bounds.size.width - px(3.), px(0.)),
                                        );
                                        inner.line_to(
                                            bounds.origin
                                                + point(
                                                    bounds.size.width * 0.5,
                                                    bounds.size.height - px(4.),
                                                ),
                                        );
                                        inner.close();
                                        window.paint_path(
                                            inner.build().expect("speech bubble inset is valid"),
                                            rgb(0x38204f),
                                        );
                                    },
                                )
                                .w(px(28.))
                                .h(px(18.))
                                .mt(px(-2.))
                                .flex_shrink_0(),
                            )
                            .child(
                                img(images::error_reaper())
                                    .size(px(240.))
                                    .max_w(relative(1.))
                                    .flex_shrink_0()
                                    .object_fit(ObjectFit::Contain),
                            )
                            .children(self.remote_restart.as_ref().map(|_| {
                                div()
                                    .id("restart-remote-daemon")
                                    .mt(px(16.))
                                    .p(px(8.))
                                    .rounded_xl()
                                    .bg(rgb(0x38204f))
                                    .border_2()
                                    .border_color(rgb(0xac50f5))
                                    .text_color(rgb(0xffffff))
                                    .font_weight(FontWeight::BOLD)
                                    .cursor_pointer()
                                    .child("Restart with bundled server — ends ALL sessions")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let Some(warning) = this.remote_restart.take() else {
                                            return;
                                        };
                                        this.start_connect(Some(warning), cx);
                                    }))
                            }))
                            .when(!self.connecting, |panel| {
                                panel.child(
                                    div()
                                        .id("copy-error")
                                        .mt(px(8.))
                                        .p(px(8.))
                                        .rounded_xl()
                                        .bg(rgb(0x38204f))
                                        .border_2()
                                        .border_color(rgb(0xac50f5))
                                        .text_color(rgb(0xffffff))
                                        .font_weight(FontWeight::BOLD)
                                        .child("Copy error")
                                        .cursor_pointer()
                                        .on_click(move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                error.clone(),
                                            ))
                                        }),
                                )
                            })
                            .child(
                                div()
                                    .id("close-error")
                                    .mt(px(8.))
                                    .p(px(8.))
                                    .rounded_xl()
                                    .bg(rgb(0x38204f))
                                    .border_2()
                                    .border_color(rgb(0xac50f5))
                                    .text_color(rgb(0xffffff))
                                    .font_weight(FontWeight::BOLD)
                                    .child("Close window")
                                    .cursor_pointer()
                                    .on_click(|_, window, _| window.remove_window()),
                            ),
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
                    pointer::cursor(
                        self.order
                            .iter()
                            .find(|tab| tab.id == self.active)
                            .and_then(|tab| tab.mouse_shape.as_deref()),
                    )
                })
                .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _, _, _| {
                        this.end_mouse_drag();
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
        .children(find_overlay)
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
        if let Some(find) = &self.find {
            let range = find.byte_range(range);
            *actual = Some(find.utf16_range(range.clone()));
            return Some(find.query[range].to_owned());
        }
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
            range: self.find.as_ref().map_or_else(
                || self.marked_selection.clone(),
                |f| f.utf16_range(f.selection.clone()),
            ),
            reversed: self.find.as_ref().is_some_and(|f| f.reversed),
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        if let Some(find) = &self.find {
            return find.marked.clone().map(|r| find.utf16_range(r));
        }
        (!self.marked.is_empty()).then(|| 0..self.marked.encode_utf16().count())
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(find) = &mut self.find {
            find.marked = None;
        }
        self.marked.clear();
        self.marked_selection = 0..0;
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked.clear();
        self.marked_selection = 0..0;
        if let Some(find) = &mut self.find {
            find.replace(range, text);
            cx.notify();
            return;
        }
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
        if let Some(find) = &mut self.find {
            let inserted = find.replace(range, text);
            let start = find.utf16_range(inserted.clone()).start;
            if let Some(selected) = selected {
                find.selection = find.byte_range(start + selected.start..start + selected.end);
                find.reversed = false;
            }
            find.marked = Some(inserted);
            cx.notify();
            return;
        }
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
        if self.find.is_some() {
            return Some(Bounds::new(
                point((self.bounds.right() - px(420.)).max(px(8.)), px(36.)),
                size(px(200.), px(32.)),
            ));
        }
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
    if !platform::zoom_modifier(mods) {
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
    bin: Option<String>,
    cx: &mut gpui::App,
) -> Result<()> {
    let window_size = size(px(config.window_width), px(config.window_height));
    let options = WindowOptions {
        window_bounds: Some(
            window_state::load(cx)
                .unwrap_or_else(|| WindowBounds::Windowed(Bounds::centered(None, window_size, cx))),
        ),
        titlebar: Some(platform::titlebar_options()),
        window_background: WindowBackgroundAppearance::Transparent,
        window_min_size: Some(size(px(200.), px(100.))),
        ..Default::default()
    };
    cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| Gui::new(config, session, host, bin, window.window_handle(), cx));
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
        if let Err(e) = open_window(config, session, host, None, cx) {
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

    #[test]
    fn message_rows_preserve_unicode_and_blank_lines() {
        assert_eq!(
            message_line_rows("hello 世界", [6].into_iter()),
            ["hello", "世界"]
        );
        assert_eq!(message_line_rows("", std::iter::empty()), [""]);
        assert_eq!(message_line_rows("short", std::iter::empty()), ["short"]);
    }

    #[test]
    fn ssh_connection_failure_is_returned_from_worker() {
        let result =
            thread::spawn(|| Gui::connect(Some("-invalid-host".into()), "test".into(), None))
                .join()
                .expect("connection worker panicked");
        let error = result.err().expect("invalid SSH host must fail");
        assert!(error.to_string().contains("invalid SSH destination"));
    }

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
