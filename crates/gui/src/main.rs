#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{Context as _, Result};
use ghostty_vt::{
    key,
    selection::Selection,
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
    time::{Duration, Instant},
};
mod ansi;
mod bidi;
mod cli;
mod clipboard;
mod config;
mod cursor;
mod find;
mod fonts;
mod images;
mod key_bindings;
mod keyboard;
use key_bindings::Action as KeyAction;
mod links;
mod notifications;
mod paths;
mod platform;
mod pointer;
mod reconnect;
mod remote;
mod scrollbar;
mod search;
mod semantic;
mod tab;
mod terminal_paint;
mod transport;
mod ui;
mod unicode;
use tab::{Cell, Tab, resize_terminal, selected_text};
mod window_state;
use config::Config;

fn active_progress(tabs: &[TabInfo], active: u64) -> Option<scytty_common::protocol::Progress> {
    let tab = tabs.iter().find(|tab| tab.id == active && !tab.exited)?;
    let mut progress = tab.progress.clone()?;
    if !matches!(
        progress.state.as_str(),
        "normal" | "error" | "paused" | "indeterminate"
    ) {
        return None;
    }
    progress.percent = progress.percent.map(|percent| percent.min(100));
    Some(progress)
}

struct Gui {
    config: Config,
    initial_font_size: f32,
    session: String,
    bin: Option<String>,
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
    native_progress: Option<scytty_common::protocol::Progress>,
    error: Option<String>,
    remote_restart: Option<remote::RestartRequired>,
    server_path: Option<String>,
    focus: FocusHandle,
    window_handle: AnyWindowHandle,
    bounds: Bounds<Pixels>,
    cell: Size<Pixels>,
    find: Option<find::Find>,
    marked: String,
    marked_selection: Range<usize>,
    scroll_remainder: f32,
    horizontal_scroll_remainder: f32,
    reported_mouse_buttons: Vec<(u64, MouseButton)>,
    tab_scroll: ScrollHandle,
    tab_overflow: (Option<usize>, Option<usize>),
    keyboard: keyboard::Keyboard,
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
            native_progress: None,
            initial_font_size: config.font_size,
            config,
            session,
            bin,
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
            server_path: None,
            focus: cx.focus_handle(),
            bounds: Bounds::default(),
            cell: size(px(8.), px(20.)),
            find: None,
            marked: String::new(),
            marked_selection: 0..0,
            scroll_remainder: 0.,
            horizontal_scroll_remainder: 0.,
            reported_mouse_buttons: Vec::new(),
            tab_scroll: ScrollHandle::new(),
            tab_overflow: (None, None),
            keyboard: keyboard::Keyboard::default(),
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
            let mut status_tick = false;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                status_tick = !status_tick;
                if view
                    .update(cx, |view, cx| {
                        if status_tick
                            && view
                                .reconnect
                                .as_mut()
                                .is_some_and(|retry| retry.start_if_due(Instant::now()))
                        {
                            view.start_connect(None, cx);
                        }
                        if status_tick
                            || view.tabs.get(&view.active).is_some_and(|tab| {
                                (tab.cursor.is_some()
                                    && tab.cursor_state.is_some_and(|c| c.blinking))
                                    || tab.cells.iter().any(|cell| cell.blink)
                            })
                        {
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
    fn start_connect(&mut self, restart: Option<remote::RestartRequired>, cx: &mut Context<Self>) {
        self.start_connect_choice(restart, None, false, cx);
    }

    fn start_connect_choice(
        &mut self,
        restart: Option<remote::RestartRequired>,
        existing_path: Option<String>,
        take_over: bool,
        cx: &mut Context<Self>,
    ) {
        if restart.is_some() {
            self.server_path = None;
        }
        if existing_path.is_some() {
            self.server_path = existing_path;
        }
        let existing_path = self.server_path.clone();
        self.connecting = true;
        self.error = None;
        self.remote_restart = None;
        self.disconnect();
        let host = self.host.clone();
        let session = self.session.clone();
        let bin = self.bin.clone();
        let task = cx.background_executor().spawn(async move {
            if let Some(warning) = restart {
                warning.restart()?;
            }
            transport::connect_session(host, session, bin, existing_path, take_over)
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
                                        if let Err(error) = view.drain(window, cx) {
                                            view.connection_failed(error, cx);
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
                            view.connection_failed(error, cx);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
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
        self.input_for(self.active, bytes)
    }
    fn input_for(&mut self, id: u64, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() || self.connecting {
            return Ok(());
        }
        let tab = self.tabs.get_mut(&id).context("Waiting for terminal")?;
        if tab.exited {
            return Ok(());
        }
        tab.cursor_since = Instant::now();
        tab.terminal.set_selection(None)?;
        tab.anchor = None;
        tab.terminal.scroll_viewport(ScrollViewport::Bottom);
        self.request(Request::Input {
            id,
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
    fn disconnect(&mut self) {
        self.reported_mouse_buttons.clear();
        self.keyboard.clear();
        self.updates = None;
        self.writer = None;
        self.connection = None;
    }
    fn retry_connection(&mut self, cx: &mut Context<Self>) {
        self.connecting = true;
        self.error = None;
        self.disconnect();
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
    fn update_native_progress(&mut self, window: &Window, force: bool, cx: &mut Context<Self>) {
        let progress = if self.connecting || self.error.is_some() {
            None
        } else {
            active_progress(&self.order, self.active)
        };
        if force || self.native_progress != progress {
            self.native_progress = progress;
            if let Err(error) = platform::set_progress(self.native_progress.as_ref(), window, cx) {
                eprintln!("Native progress: {error:#}");
            }
        }
    }
    fn clear_native_progress(&mut self, cx: &mut Context<Self>) {
        if self.native_progress.take().is_some() {
            // Receive failures can be reported inside an existing window update.
            let view = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = view.update(cx, |view, cx| {
                    if view.native_progress.is_none() {
                        let _ = view
                            .window_handle
                            .update(cx, |_, window, cx| platform::set_progress(None, window, cx));
                    }
                });
            });
        }
    }
    fn connection_failed(&mut self, error: anyhow::Error, cx: &mut Context<Self>) {
        self.clear_native_progress(cx);
        if self.has_connected
            && reconnect::transport_error(&error)
            && error.downcast_ref::<remote::RestartRequired>().is_none()
        {
            self.retry_connection(cx);
        } else {
            self.disconnect();
            self.reconnect = None;
            self.connecting = false;
            self.remote_restart = error.downcast_ref::<remote::RestartRequired>().cloned();
            self.error = Some(format!("{error:#}"));
            cx.notify();
        }
    }
    fn report(&mut self, result: Result<()>, cx: &mut Context<Self>) {
        if let Err(error) = result {
            if reconnect::transport_error(&error) {
                self.connection_failed(error, cx);
                return;
            }
            // Local config/UI errors must not cancel a connection or its retry timer.
            self.clear_native_progress(cx);
            self.error = Some(format!("{error:#}"));
        }
        cx.notify();
    }
    fn drain(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<()> {
        for _ in 0..256 {
            let Some(rx) = &self.updates else { break };
            let update = match rx.try_recv() {
                Ok(Ok(update)) => update,
                Ok(Err(error)) => {
                    self.disconnect();
                    return Err(error);
                }
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
                    terminal.set_default_cursor_style(Some(self.config.default_cursor_style))?;
                    self.tabs.insert(id, Tab::new(terminal)?);
                    if self.connecting {
                        self.has_connected = true;
                        self.reconnect = None;
                        self.connecting = false;
                        // A rejected attach must not close with an unread configuration request.
                        self.request(Request::SetScrollback {
                            megabytes: self.config.scrollback_mb,
                        })?;
                        self.update_native_progress(window, false, cx);
                    }
                }
                Update::Output { id, bytes } => {
                    if let Some(tab) = self.tabs.get_mut(&id) {
                        tab.terminal.vt_write(&bytes);
                        if let Some(search) = &mut tab.search {
                            search.feed(&tab.terminal)?;
                        }
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
                        if let Some(search) = &mut tab.search {
                            search.feed(&tab.terminal)?;
                        }
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
                    let title = active_tab_title(&tabs, active);
                    match self
                        .config
                        .window_title(&self.session, &title, self.host.is_some())
                    {
                        Ok(title) => window.set_window_title(&title),
                        Err(error) => self.report(Err(error), cx),
                    }
                    self.order = tabs;
                    self.active = active;
                    self.update_native_progress(window, false, cx);
                }
                Update::Error(message) => {
                    self.disconnect();
                    anyhow::bail!(message);
                }
            }
        }
        self.report_focus(window.is_window_active())
    }
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.keyboard.sync(window.modifiers(), window.capslock().on);
        let stroke = &event.keystroke;
        if self.plus_menu_position.is_some() {
            match stroke.key.as_str() {
                "escape" => self.plus_menu_position = None,
                "down" | "up" => {
                    if let Some(index) = self
                        .config
                        .next_menu_item(self.plus_menu_selected, stroke.key == "down")
                    {
                        self.plus_menu_selected = index;
                    }
                }
                "enter" => {
                    if let Some(action) = self
                        .config
                        .plus_menu
                        .get(self.plus_menu_selected)
                        .map(|entry| entry.action.clone())
                        .filter(|action| *action != config::MenuAction::Separator)
                    {
                        let result = self.menu_action(&action, window, cx);
                        self.report(result, cx);
                    }
                }
                _ => {}
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let m = stroke.modifiers;
        let actions = self.config.key_actions(stroke);
        if let Some(find) = &mut self.find
            && !actions.contains(&KeyAction::Find)
        {
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
        let result = if !actions.is_empty() {
            Some(key_bindings::run_actions(&actions, |action| {
                self.run_key_action(action, window, cx)
            }))
        } else {
            self.encode_key(
                stroke,
                if event.is_held {
                    key::Action::Repeat
                } else {
                    key::Action::Press
                },
            )
            .transpose()
        };
        if let Some(result) = result {
            cx.stop_propagation();
            self.report(result, cx);
        }
        self.focus.focus(window);
    }
    fn run_key_action(
        &mut self,
        action: KeyAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        match action {
            KeyAction::NewTab => self.menu_action(&config::MenuAction::NewTab, window, cx),
            KeyAction::NewWindow => self.menu_action(&config::MenuAction::NewWindow, window, cx),
            KeyAction::ReloadConfig => {
                self.menu_action(&config::MenuAction::ReloadConfig, window, cx)
            }
            KeyAction::CloseTab => self.menu_action(&config::MenuAction::CloseTab, window, cx),
            KeyAction::PreviousTab | KeyAction::NextTab => {
                if let Some(id) = adjacent_tab(
                    &self.order,
                    self.active,
                    if action == KeyAction::PreviousTab {
                        -1
                    } else {
                        1
                    },
                ) {
                    self.request(Request::Activate { id })?;
                }
                Ok(())
            }
            KeyAction::PreviousPrompt | KeyAction::NextPrompt => {
                if let Some(tab) = self.tabs.get_mut(&self.active)
                    && let Some(row) =
                        semantic::prompt_row(&tab.terminal, action == KeyAction::PreviousPrompt)?
                {
                    tab.terminal.scroll_viewport(ScrollViewport::Row(row));
                    self.scrollbar_last_used = Instant::now();
                }
                Ok(())
            }
            KeyAction::Copy => {
                if let Some(tab) = self.tabs.get(&self.active)
                    && let Some(text) = selected_text(&tab.terminal)?
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                Ok(())
            }
            KeyAction::Paste => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.paste(&text)?;
                }
                Ok(())
            }
            KeyAction::ZoomIn | KeyAction::ZoomOut | KeyAction::ResetZoom => {
                self.config.font_size = action
                    .font_size(self.config.font_size, self.initial_font_size)
                    .unwrap();
                Ok(())
            }
            KeyAction::Find => {
                let find = self.find.get_or_insert_with(Default::default);
                find.selection = 0..find.query.len();
                find.reversed = false;
                self.marked.clear();
                self.marked_selection = 0..0;
                self.focus.focus(window);
                Ok(())
            }
        }
    }
    fn find_step(&mut self, previous: bool) -> Result<()> {
        let Some(find) = &self.find else {
            return Ok(());
        };
        let Some(tab) = self.tabs.get_mut(&self.active) else {
            return Ok(());
        };
        if tab.search.is_none() {
            tab.search = Some(search::Search::new(&tab.terminal)?);
        }
        let search = tab.search.as_mut().unwrap();
        find::advance(search, &tab.terminal, &find.query)?;
        if previous {
            search.select_prev(&mut tab.terminal)?;
        } else {
            search.select_next(&mut tab.terminal)?;
        }
        Ok(())
    }
    fn encode_key(&mut self, stroke: &Keystroke, action: key::Action) -> Result<Option<()>> {
        // GPUI consumes Shift for punctuation; preserve its logical-key modifiers.
        let id = self.keyboard.target(stroke).unwrap_or(self.active);
        let Some(tab) = self.tabs.get_mut(&id).filter(|tab| !tab.exited) else {
            return Ok(None);
        };
        let Some(bytes) = keyboard::encode(
            &tab.terminal,
            &mut tab.encoder,
            stroke,
            action,
            !self.marked.is_empty(),
            self.keyboard.caps_lock,
        )?
        else {
            return Ok(None);
        };
        self.input_for(id, &bytes)?;
        if !self.connecting {
            self.keyboard.remember(id, stroke);
        }
        Ok(Some(()))
    }
    fn send_key_release(&mut self, id: u64, stroke: &Keystroke) -> Result<()> {
        let Some(tab) = self.tabs.get_mut(&id).filter(|tab| !tab.exited) else {
            return Ok(());
        };
        if let Some(bytes) = keyboard::encode(
            &tab.terminal,
            &mut tab.encoder,
            stroke,
            key::Action::Release,
            false,
            self.keyboard.caps_lock,
        )? {
            self.request(Request::Input { id, bytes })?;
        }
        Ok(())
    }
    fn key_up(&mut self, event: &KeyUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.keyboard.caps_lock = window.capslock().on;
        if let Some((id, stroke)) = self.keyboard.release(&event.keystroke) {
            let result = self.send_key_release(id, &stroke);
            self.report(result, cx);
            cx.stop_propagation();
        }
    }
    fn modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (stroke, action) in self.keyboard.modifiers_changed(event) {
            if action == key::Action::Release {
                self.key_up(&KeyUpEvent { keystroke: stroke }, window, cx);
            } else if self.find.is_none() && self.plus_menu_position.is_none() {
                let result = self.encode_key(&stroke, action).map(|_| ());
                self.report(result, cx);
            }
        }
    }
    fn report_focus(&mut self, focused: bool) -> Result<()> {
        if self.connecting {
            return Ok(());
        }
        let Some(writer) = self.writer.as_mut() else {
            return Ok(());
        };
        for (&id, tab) in &mut self.tabs {
            if let Some(bytes) = tab.focus(focused && id == self.active)? {
                protocol::send(
                    writer,
                    &Request::Input {
                        id,
                        bytes: bytes.to_vec(),
                    },
                )?;
            }
        }
        Ok(())
    }
    fn release_keys(&mut self) -> Result<()> {
        for (id, mut stroke) in self.keyboard.clear() {
            stroke.modifiers = Modifiers::default();
            self.send_key_release(id, &stroke)?;
        }
        Ok(())
    }
    fn release_mouse(&mut self, position: gpui::Point<Pixels>) -> Result<()> {
        self.end_mouse_drag();
        self.link_press = None;
        let buttons = std::mem::take(&mut self.reported_mouse_buttons);
        release_mouse_buttons(buttons, |id, button| {
            self.report_mouse(
                id,
                ghostty_vt::mouse::Action::Release,
                Some(button),
                position,
                Modifiers::default(),
            )
        })
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
        let point = tab.point(position, self.bounds, self.cell)?;
        links::at_terminal(tab, point)
    }
    fn end_mouse_drag(&mut self) {
        self.scroll_drag = None;
        for tab in self.tabs.values_mut() {
            tab.anchor = None;
        }
    }
    fn report_mouse(
        &mut self,
        id: u64,
        action: ghostty_vt::mouse::Action,
        button: Option<MouseButton>,
        position: gpui::Point<Pixels>,
        modifiers: Modifiers,
    ) -> Result<()> {
        use ghostty_vt::mouse;
        if self.connecting || self.writer.is_none() {
            return Ok(());
        }
        let pressed = self
            .reported_mouse_buttons
            .iter()
            .any(|(tab, _)| *tab == id);
        let Some(tab) = self.tabs.get_mut(&id) else {
            return Ok(());
        };
        let width = (f32::from(self.cell.width) as u32).max(1);
        let height = (f32::from(self.cell.height) as u32).max(1);
        let mut event = mouse::Event::new()?;
        let mods = keyboard::shared_mods(modifiers);
        event
            .set_action(action)
            .set_mods(mods)
            .set_button(button.map(|button| match button {
                MouseButton::Left => mouse::Button::Left,
                MouseButton::Right => mouse::Button::Right,
                MouseButton::Middle => mouse::Button::Middle,
                MouseButton::Navigate(NavigationDirection::Back) => mouse::Button::Eight,
                MouseButton::Navigate(NavigationDirection::Forward) => mouse::Button::Nine,
            }))
            .set_position(mouse::Position {
                x: f32::from(position.x - self.bounds.left()).clamp(
                    0.,
                    (u32::from(tab.terminal.cols()?) * width).saturating_sub(1) as f32,
                ),
                y: f32::from(position.y - self.bounds.top()).clamp(
                    0.,
                    (u32::from(tab.terminal.rows()?) * height).saturating_sub(1) as f32,
                ),
            });
        let bytes = tab.mouse(&event, self.cell, pressed)?;
        if !bytes.is_empty() {
            self.request(Request::Input { id, bytes })?;
        }
        Ok(())
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self
            .reported_mouse_buttons
            .iter()
            .position(|(_, button)| *button == event.button)
        {
            let (id, _) = self.reported_mouse_buttons.remove(index);
            let result = self.report_mouse(
                id,
                ghostty_vt::mouse::Action::Release,
                Some(event.button),
                event.position,
                event.modifiers,
            );
            self.report(result, cx);
            return;
        }
        if event.button != MouseButton::Left {
            return;
        }
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
        let report = self
            .tabs
            .get(&self.active)
            .map(|tab| tab.terminal.is_mouse_tracking())
            .transpose();
        let tracking = match report {
            Ok(value) => value.unwrap_or(false),
            Err(error) => {
                self.report(Err(error.into()), cx);
                return;
            }
        };
        let scrollbar = self
            .tabs
            .get(&self.active)
            .and_then(|tab| tab.terminal.scrollbar().ok())
            .is_some_and(|scroll| {
                scrollbar::ScrollBar::new(self.bounds, scroll)
                    .track
                    .contains(&event.position)
            });
        if tracking && !event.modifiers.shift && !scrollbar {
            if !self
                .reported_mouse_buttons
                .contains(&(self.active, event.button))
            {
                self.reported_mouse_buttons
                    .push((self.active, event.button));
            }
            self.link_press = None;
            let result = self.report_mouse(
                self.active,
                ghostty_vt::mouse::Action::Press,
                Some(event.button),
                event.position,
                event.modifiers,
            );
            self.report(result, cx);
            return;
        }
        if event.button != MouseButton::Left {
            return;
        }
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
                    let point = tab.point(event.position, self.bounds, self.cell)?;
                    let selection = tab.terminal.select_output(tab.terminal.grid_ref(point)?)?;
                    tab.terminal.set_selection(selection.as_ref())?;
                    tab.anchor = None;
                    self.link_press = None;
                    return Ok(());
                }
                tab.terminal.set_selection(None)?;
                tab.anchor = Some(tab.terminal.track_grid_ref(tab.point(
                    event.position,
                    self.bounds,
                    self.cell,
                )?)?);
            }
            Ok(())
        })();
        self.report(result, cx);
    }
    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.scroll_drag.is_none()
            && (self.bounds.contains(&event.position)
                || self
                    .reported_mouse_buttons
                    .iter()
                    .any(|(id, _)| *id == self.active))
            && !event.modifiers.shift
        {
            let tracking = self
                .tabs
                .get(&self.active)
                .map(|tab| tab.terminal.is_mouse_tracking())
                .transpose();
            match tracking {
                Ok(Some(true)) => {
                    self.link_hovered = None;
                    let button = event.pressed_button.filter(|button| {
                        self.reported_mouse_buttons
                            .contains(&(self.active, *button))
                    });
                    let result = self.report_mouse(
                        self.active,
                        ghostty_vt::mouse::Action::Motion,
                        button,
                        event.position,
                        event.modifiers,
                    );
                    self.report(result, cx);
                    return;
                }
                Err(error) => {
                    self.report(Err(error.into()), cx);
                    return;
                }
                _ => {}
            }
        }
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
                let end =
                    tab.terminal
                        .grid_ref(tab.point(event.position, self.bounds, self.cell)?)?;
                tab.terminal
                    .set_selection(Some(&Selection::new(start, end, false)))?;
            }
            Ok(())
        })();
        self.report(result, cx);
    }
    fn scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let result = (|| -> Result<()> {
            if let Some(tab) = self.tabs.get_mut(&self.active) {
                self.scrollbar_last_used = Instant::now();
                let delta = event.delta.pixel_delta(self.cell.height);
                let rows = scroll_rows(
                    &mut self.scroll_remainder,
                    f32::from(delta.y) / f32::from(self.cell.height),
                );
                let mods = keyboard::shared_mods(event.modifiers);
                let position = ghostty_vt::mouse::Position {
                    x: f32::from(event.position.x - self.bounds.left()).max(0.),
                    y: f32::from(event.position.y - self.bounds.top()).max(0.),
                };
                let mut bytes = tab.scroll(rows, position, self.cell, mods)?;
                let columns = scroll_rows(
                    &mut self.horizontal_scroll_remainder,
                    f32::from(delta.x) / f32::from(self.cell.width),
                );
                if !event.modifiers.shift && tab.terminal.is_mouse_tracking()? && columns != 0 {
                    use ghostty_vt::mouse;
                    let mut wheel = mouse::Event::new()?;
                    wheel
                        .set_action(mouse::Action::Press)
                        .set_button(Some(if columns < 0 {
                            mouse::Button::Six
                        } else {
                            mouse::Button::Seven
                        }))
                        .set_mods(mods)
                        .set_position(position);
                    for _ in 0..columns.unsigned_abs() {
                        bytes.extend(tab.mouse(&wheel, self.cell, false)?);
                    }
                }
                if !bytes.is_empty() {
                    self.input(&bytes)?;
                }
            }
            Ok(())
        })();
        self.report(result, cx);
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
            terminal_cell_height(
                window.text_system().ascent(font_id, font_size),
                window.text_system().descent(font_id, font_size),
                window.scale_factor(),
            ),
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
        if tab.request_resize(
            dimensions,
            Instant::now(),
            !self.connecting && self.writer.is_some(),
        ) {
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
        if !self.connecting && dimensions != tab.size && !tab.resize_pending {
            window.request_animation_frame();
        }
        let highlights = if let Some(find) = &mut self.find {
            if tab.search.is_none() {
                tab.search = Some(search::Search::new(&tab.terminal)?);
            }
            let search = tab.search.as_mut().unwrap();
            let running = find::advance(search, &tab.terminal, &find.query)?;
            if !running && search.selected_index().is_none() && search.total_matches() > 0 {
                search.select_next(&mut tab.terminal)?;
            }
            let stats = (search.total_matches(), search.selected_index(), running);
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
        if tab.animation_deadline.is_some() {
            window.request_animation_frame();
        }
        let hovered_cells = if let Some(position) = self.link_hovered {
            links::hovered_cells(tab, tab.point(position, bounds, cell)?)?
        } else {
            0..0
        };
        tab.paint_cache.prepare(
            &tab.bidi.cells,
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
                for x in highlight.x..highlight.x + highlight.width {
                    window.paint_quad(fill(
                        Bounds::new(
                            bounds.origin
                                + point(
                                    cell.width * tab.bidi.visual_x(x, highlight.y) as f32,
                                    cell.height * highlight.y as f32,
                                ),
                            size(cell.width, cell.height),
                        ),
                        if highlight.selected {
                            rgba(0xb36b00dd)
                        } else {
                            rgba(0x80700099)
                        },
                    ));
                }
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
                                cell.width * tab.bidi.visual_x(c.x, c.y) as f32,
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
            cursor::paint(tab, bounds.origin, cell, &font, font_size, window, cx)?;
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
                        bounds.origin
                            + point(
                                cell.width * tab.bidi.visual_x(x as usize, y as usize) as f32,
                                cell.height * y as f32,
                            ),
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
}
fn release_mouse_buttons(
    buttons: Vec<(u64, MouseButton)>,
    mut release: impl FnMut(u64, MouseButton) -> Result<()>,
) -> Result<()> {
    let mut result = Ok(());
    for (id, button) in buttons {
        let released = release(id, button);
        result = result.and(released);
    }
    result
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
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
            .on_modifiers_changed(cx.listener(Self::modifiers_changed));
        match self.header(window, cx) {
            Ok(header) => root = root.child(header),
            Err(e) => {
                self.error = Some(format!("{e:#}"));
            }
        }
        let menu = self.plus_menu(window, cx);
        if let Some(error) = self.error.clone().or_else(|| {
            self.connecting.then(|| {
                let host = self.host.as_deref().unwrap_or("local server");
                match &self.reconnect {
                    Some(retry) => format!("Reconnecting to {host}… (attempt {})", retry.attempts),
                    None => format!("Connecting to {host}…"),
                }
            })
        }) {
            return root
                .child(self.error_screen(error, window, cx))
                .children(menu);
        }

        if self.tabs.get(&self.active).is_some_and(|tab| tab.exited) {
            root = root.child(
                div()
                    .flex_shrink_0()
                    .child("Shell exited. Close the window to quit."),
            );
        }
        let entity = cx.entity();
        let find_overlay = self.find_overlay(window, cx);
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
                .on_any_mouse_down(cx.listener(Self::mouse_down))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
                .on_mouse_up(MouseButton::Right, cx.listener(Self::mouse_up))
                .on_mouse_up(MouseButton::Middle, cx.listener(Self::mouse_up))
                .on_mouse_up(
                    MouseButton::Navigate(NavigationDirection::Back),
                    cx.listener(Self::mouse_up),
                )
                .on_mouse_up(
                    MouseButton::Navigate(NavigationDirection::Forward),
                    cx.listener(Self::mouse_up),
                )
                .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
                .on_mouse_up_out(MouseButton::Right, cx.listener(Self::mouse_up))
                .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::mouse_up))
                .on_mouse_up_out(
                    MouseButton::Navigate(NavigationDirection::Back),
                    cx.listener(Self::mouse_up),
                )
                .on_mouse_up_out(
                    MouseButton::Navigate(NavigationDirection::Forward),
                    cx.listener(Self::mouse_up),
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
                    let result = paths::dropped(
                        &paths,
                        platform::WINDOWS_PATHS && this.host.is_none(),
                        this.bin.as_deref(),
                    )
                    .and_then(|text| this.paste(&text));
                    this.report(result, cx);
                }))
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, cx| {
                            entity.update(cx, |this, cx| {
                                let result = this.paint(bounds, window, cx);
                                this.report(result, cx);
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
        let text = self
            .find
            .as_ref()
            .map_or(self.marked.as_str(), |find| find.query.as_str());
        let range = unicode::byte_range(text, range);
        *actual = Some(unicode::utf16_range(text, range.clone()));
        Some(text[range].to_owned())
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
                |f| unicode::utf16_range(&f.query, f.selection.clone()),
            ),
            reversed: self.find.as_ref().is_some_and(|f| f.reversed),
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        if let Some(find) = &self.find {
            return find
                .marked
                .clone()
                .map(|r| unicode::utf16_range(&find.query, r));
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
            if let Some(selected) = selected {
                let selected = unicode::byte_range(&find.query[inserted.clone()], selected);
                find.selection = inserted.start + selected.start..inserted.start + selected.end;
            }
            find.marked = Some(inserted);
            cx.notify();
            return;
        }
        self.marked_selection = unicode::replace_marked(&mut self.marked, range, text, selected);
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
        let tab = self.tabs.get(&self.active)?;
        let (x, y) = tab.cursor?;
        let x = tab.bidi.visual_x(x as usize, y as usize);
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
        platform::clear_progress(self.window_handle.window_id());
        if let Some(bounds) = self.window_bounds
            && let Err(e) = window_state::save(bounds)
        {
            eprintln!("Cannot save window geometry: {e:#}");
        }
        self.disconnect();
    }
}

fn scroll_rows(remainder: &mut f32, delta: f32) -> isize {
    // GPUI's content-motion delta has the opposite sign to Ghostty's viewport delta.
    *remainder -= delta;
    let rows = remainder.trunc() as isize;
    *remainder -= rows as f32;
    rows
}

fn terminal_cell_height(ascent: Pixels, descent: Pixels, scale: f32) -> Pixels {
    // Font metrics use a negative descent on macOS; never subtract descender space.
    let height = ascent + descent.abs();
    px((f32::from(height) * scale).floor().max(1.) / scale)
}

fn is_nerd_icon(ch: char) -> bool {
    matches!(ch, '\u{e000}'..='\u{f8ff}' | '\u{f0000}'..='\u{ffffd}' | '\u{100000}'..='\u{10fffd}')
}
fn status_runs(segments: Vec<(String, Option<Rgba>)>) -> Vec<(String, Option<Rgba>, bool)> {
    let mut chars = segments
        .iter()
        .flat_map(|(text, color)| text.chars().map(move |ch| (ch, *color)))
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
fn active_tab_title(tabs: &[TabInfo], active: u64) -> String {
    tabs.iter()
        .enumerate()
        .find(|(_, tab)| tab.id == active)
        .map_or_else(|| "scytty".into(), |(index, tab)| tab_title(index, tab))
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
        let weak_view = view.downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            let _ = weak_view.update(cx, |view, _| view.disconnect());
            true
        });
        view.update(cx, |_, cx| {
            cx.on_app_quit(|view, _| {
                view.disconnect();
                async {}
            })
            .detach();
            cx.observe_window_activation(window, |view, window, cx| {
                let focused = window.is_window_active();
                let result = if focused {
                    view.update_native_progress(window, true, cx);
                    Ok(())
                } else {
                    let mouse = view.release_mouse(window.mouse_position());
                    let keys = view.release_keys();
                    mouse.and(keys)
                }
                .and_then(|()| view.report_focus(focused));
                view.report(result, cx);
            })
            .detach();
        });
        view.read(cx).focus.focus(window);
        view
    })?;
    Ok(())
}

fn main() -> Result<()> {
    let Some(cli::Options { host, session, bin }) = cli::parse(std::env::args().skip(1))? else {
        println!("{}", cli::HELP);
        return Ok(());
    };
    let config = Config::load()?;
    Application::new().run(move |cx| {
        if let Err(e) = open_window(config, session, host, bin, cx) {
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
#[path = "../tests/unit/gui.rs"]
mod tests;
