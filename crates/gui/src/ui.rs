use super::*;

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

fn tab_width_limit(font_size: f32, window_width: Pixels, tab_count: usize) -> Pixels {
    let limit = px(font_size * 16.);
    if window_width > px(0.) {
        limit.min(window_width / tab_count.clamp(1, 3) as f32)
    } else {
        limit
    }
}

fn hidden_tabs(
    viewport: Bounds<Pixels>,
    offset: Pixels,
    tabs: impl Iterator<Item = (usize, Bounds<Pixels>)>,
) -> (Option<usize>, Option<usize>) {
    let mut left = None;
    let mut right = None;
    for (index, bounds) in tabs {
        // Windows pixel snapping can move child edges by up to one logical pixel.
        if bounds.left() + offset < viewport.left() - px(1.) {
            left = Some(index);
        }
        if right.is_none() && bounds.right() + offset > viewport.right() + px(1.) {
            right = Some(index);
        }
    }
    (left, right)
}

fn tab_group_offset(
    viewport: Bounds<Pixels>,
    offset: Pixels,
    first: Bounds<Pixels>,
    active: Bounds<Pixels>,
    last: Bounds<Pixels>,
) -> Pixels {
    let (first, last) = if last.right() - first.left() > viewport.size.width + px(0.5) {
        (active, active)
    } else {
        (first, last)
    };
    if first.size.width > viewport.size.width {
        return viewport.left() - first.left();
    }
    if first.left() + offset < viewport.left() {
        viewport.left() - first.left()
    } else if last.right() + offset > viewport.right() {
        viewport.right() - last.right()
    } else {
        offset
    }
}

fn menu_width(labels: impl Iterator<Item = Pixels>, viewport: Pixels) -> Pixels {
    // Menu border/padding plus row border/padding on both sides.
    (labels.fold(px(0.), Pixels::max) + px(44.)).min(viewport)
}

fn error_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(16.))
        .py(px(9.))
        .rounded(px(8.))
        .bg(rgb(0x30213f))
        .border_1()
        .border_color(rgb(0x785494))
        .text_size(px(14.))
        .text_color(rgb(0xf4edf9))
        .font_weight(FontWeight::MEDIUM)
        .flex_shrink_0()
        .hover(|style| style.bg(rgb(0x463057)).border_color(rgb(0xac78d4)))
        .active(|style| style.bg(rgb(0x251830)).border_color(rgb(0xc79aeb)))
        .cursor_pointer()
        .child(label)
}

impl Gui {
    pub(super) fn segments(
        &self,
        name: &str,
        title: &str,
        index: usize,
        active: bool,
        tab: Option<&TabInfo>,
    ) -> Result<Div> {
        let segments =
            self.config
                .status_text(name, title, index, active, self.host.is_some(), tab)?;
        Ok(self.text_segments(segments, self.config.font_size, px(28.)))
    }
    fn text_segments(
        &self,
        segments: Vec<(String, Option<Rgba>)>,
        font_size: f32,
        _line_height: Pixels,
    ) -> Div {
        let mut line = div().flex().items_center().flex_shrink_0();
        for (text, color) in status_runs(segments) {
            line = line.child(
                div()
                    .flex_shrink_0()
                    .text_size(px(font_size))
                    .text_color(color.unwrap_or(rgb(0xc6d0f5)))
                    .child(text),
            );
        }
        line
    }
    fn tab_label(&self, label: &config::TabBarLabel) -> Div {
        let color = label.color.unwrap_or(rgb(0xdcdcdc));
        self.text_segments(
            vec![(label.text.clone(), Some(color))],
            self.config.font_size,
            px(28.),
        )
    }
    pub(super) fn menu_action(
        &mut self,
        action: &config::MenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.plus_menu_position = None;
        match action {
            config::MenuAction::Separator => Ok(()),
            config::MenuAction::NewTab => self.request(Request::NewTab),
            config::MenuAction::CloseTab => self.request(Request::CloseTab { id: self.active }),
            config::MenuAction::ReloadConfig => {
                let mut config = Config::load()?;
                config.font_family = fonts::resolve(&config.font_family)?;
                let title = active_tab_title(&self.order, self.active);
                let title = config.window_title(&self.session, &title, self.host.is_some())?;
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
                for tab in self.tabs.values_mut() {
                    tab.terminal
                        .set_default_cursor_style(Some(config.default_cursor_style))?;
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
    pub(super) fn plus_menu(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let position = self.plus_menu_position?;
        let mut menu_font = self.config.font();
        menu_font.weight = FontWeight::MEDIUM;
        let width = menu_width(
            self.config
                .plus_menu
                .iter()
                .filter(|entry| entry.action != config::MenuAction::Separator)
                .map(|entry| {
                    let run = TextRun {
                        len: entry.label.len(),
                        font: menu_font.clone(),
                        color: rgb(0xf4edf9).into(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    window
                        .text_system()
                        .shape_line(entry.label.clone().into(), px(14.), &[run], None)
                        .width
                }),
            window.viewport_size().width,
        );
        let row_height = px((self.config.font_size * 1.5).max(14.) + 8.);
        let height = (self
            .config
            .plus_menu
            .iter()
            .map(|entry| {
                if entry.action == config::MenuAction::Separator {
                    px(9.)
                } else {
                    row_height
                }
            })
            .fold(px(0.), |height, row| height + row)
            + px(8.))
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
            .bg(rgb(0x30213f))
            .border_1()
            .border_color(rgb(0x785494))
            .rounded(px(8.))
            .text_size(px(14.))
            .text_color(rgb(0xf4edf9))
            .font_weight(FontWeight::MEDIUM)
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
            if entry.action == config::MenuAction::Separator {
                menu = menu.child(
                    div()
                        .h(px(9.))
                        .px(px(8.))
                        .py(px(4.))
                        .child(div().h(px(1.)).bg(rgb(0x785494))),
                );
                continue;
            }
            let action = entry.action.clone();
            menu = menu.child(
                div()
                    .id(("plus-menu-item", index))
                    .h(row_height)
                    .px(px(16.))
                    .line_height(row_height)
                    .overflow_hidden()
                    .cursor_pointer()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if index == self.plus_menu_selected {
                        rgb(0xac78d4)
                    } else {
                        rgb(0x30213f)
                    })
                    .bg(if index == self.plus_menu_selected {
                        rgb(0x463057)
                    } else {
                        rgb(0x30213f)
                    })
                    .active(|style| style.bg(rgb(0x251830)).border_color(rgb(0xc79aeb)))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.plus_menu_selected = index;
                            cx.notify();
                        }
                    }))
                    .child(div().w_full().overflow_hidden().child(self.text_segments(
                        vec![(entry.label.clone(), Some(rgb(0xf4edf9)))],
                        14.,
                        row_height,
                    )))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let result = this.menu_action(&action, window, cx);
                        this.focus.focus(window);
                        this.report(result, cx);
                    })),
            );
        }
        Some(menu.into_any_element())
    }
    fn tab_indicator(&self, left: bool, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let index = if left {
            self.tab_overflow.0
        } else {
            self.tab_overflow.1
        };
        let id = self.order.get(index?)?.id;
        let label = if left {
            &self.config.tab_left_indicator
        } else {
            &self.config.tab_right_indicator
        };
        Some(
            div()
                .id(if left { "tabs-left" } else { "tabs-right" })
                .min_w(px(20.))
                .px(px(4.))
                .flex_shrink_0()
                .text_center()
                .child(self.tab_label(label))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    let result = this.request(Request::Activate { id });
                    this.focus.focus(window);
                    this.report(result, cx);
                })),
        )
    }
    pub(super) fn header(&self, window: &mut Window, cx: &mut Context<Self>) -> Result<AnyElement> {
        let active = self.order.iter().find(|t| t.id == self.active);
        let width = self.tab_scroll.bounds().size.width;
        // The strip is content-sized; using its width here repeatedly shrinks longer titles.
        let max_tab_width = tab_width_limit(
            self.config.font_size,
            window.viewport_size().width,
            self.order.len(),
        );
        cx.on_next_frame(window, move |this, _, cx| {
            let viewport = this.tab_scroll.bounds();
            let mut offset = this.tab_scroll.offset();
            let previous_offset = offset.x;
            if let Some(index) = this.order.iter().position(|tab| tab.id == this.active)
                && let Some(active) = this.tab_scroll.bounds_for_item(index)
                && let Some(first) = this.tab_scroll.bounds_for_item(index.saturating_sub(1))
                && let Some(last) = this
                    .tab_scroll
                    .bounds_for_item((index + 1).min(this.order.len() - 1))
            {
                // Scroll only after layout: indicator changes move the viewport between frames.
                offset.x = tab_group_offset(viewport, offset.x, first, active, last);
            }
            let scrolled = f32::from(offset.x - previous_offset).abs() > 0.5;
            if scrolled {
                this.tab_scroll.set_offset(offset);
            }
            let overflow = hidden_tabs(
                viewport,
                offset.x,
                (0..this.order.len()).filter_map(|index| {
                    this.tab_scroll
                        .bounds_for_item(index)
                        .map(|bounds| (index, bounds))
                }),
            );
            if scrolled
                || this.tab_overflow != overflow
                || f32::from(viewport.size.width - width).abs() > 0.5
            {
                this.tab_overflow = overflow;
                cx.notify();
            }
        });
        let mut bar = platform::titlebar();
        let mut tabs = div()
            .id("tab-strip")
            .flex()
            .items_center()
            .flex_shrink()
            .min_w_0()
            .overflow_x_scroll()
            .track_scroll(&self.tab_scroll);
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
                .max_w(max_tab_width)
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
        bar = bar
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_shrink()
                    .min_w_0()
                    .child(self.segments("left_status", "", 0, false, active)?)
                    .children(self.tab_indicator(true, cx))
                    .child(tabs)
                    .children(self.tab_indicator(false, cx)),
            )
            .child(
                div()
                    .id("new-tab")
                    .flex_shrink_0()
                    .px(px(8.))
                    .cursor_pointer()
                    .child(self.tab_label(&self.config.new_tab_button))
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
        Ok(platform::window_controls(bar)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    let first = this
                        .config
                        .next_menu_item(this.config.plus_menu.len().saturating_sub(1), true);
                    this.plus_menu_selected = first.unwrap_or(0);
                    this.plus_menu_position = first.map(|_| event.position);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .into_any_element())
    }
    pub(super) fn find_overlay(&self, window: &Window, cx: &mut Context<Self>) -> Option<Div> {
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
        find_overlay
    }
    pub(super) fn error_screen(
        &self,
        error: String,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let bubble_width = (window.viewport_size().width - px(48.)).clamp(px(41.), px(640.));
        let text_width = bubble_width - px(40.);
        let image_width = (window.viewport_size().width - px(48.)).clamp(px(1.), px(240.));
        let image_pixels = (f32::from(image_width) * window.scale_factor())
            .round()
            .max(1.) as u32;
        let mut wrapper = window
            .text_system()
            .line_wrapper(self.config.font().bold(), px(16.));
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
                                tail.line_to(bounds.origin + point(bounds.size.width, px(0.)));
                                tail.line_to(
                                    bounds.origin
                                        + point(bounds.size.width * 0.5, bounds.size.height),
                                );
                                tail.close();
                                window.paint_path(
                                    tail.build().expect("speech bubble triangle is valid"),
                                    rgb(0xac50f5),
                                );
                                let mut inner = PathBuilder::fill();
                                inner.move_to(bounds.origin + point(px(3.), px(0.)));
                                inner.line_to(
                                    bounds.origin + point(bounds.size.width - px(3.), px(0.)),
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
                        img(images::error_reaper(image_pixels))
                            .size(px(240.))
                            .max_w(relative(1.))
                            .flex_shrink_0()
                            .object_fit(ObjectFit::Contain),
                    )
                    .when(
                        matches!(
                            self.error.as_deref(),
                            Some(
                                protocol::SESSION_ATTACHED_ERROR | protocol::SESSION_REPLACED_ERROR
                            )
                        ),
                        |screen| {
                            screen.child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(8.))
                                    .mt(px(8.))
                                    .child(error_button("reconnect-session", "Reconnect").on_click(
                                        cx.listener(|this, _, _, cx| this.start_connect(None, cx)),
                                    ))
                                    .child(
                                        error_button(
                                            "take-over-session",
                                            "Disconnect previous and connect",
                                        )
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.start_connect_choice(None, None, true, cx)
                                            }),
                                        ),
                                    ),
                            )
                        },
                    )
                    .children(self.remote_restart.as_ref().map(|warning| {
                        div()
                            .flex()
                            .mt(px(8.))
                            .gap(px(8.))
                            .when(warning.existing_path.is_some(), |row| {
                                row.child(
                                    error_button("connect-existing-daemon", "Connect to existing")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            let path = this
                                                .remote_restart
                                                .take()
                                                .and_then(|warning| warning.existing_path);
                                            if let Some(path) = path {
                                                this.start_connect_choice(
                                                    None,
                                                    Some(path),
                                                    false,
                                                    cx,
                                                );
                                            }
                                        })),
                                )
                            })
                            .child(
                                error_button(
                                    "restart-remote-daemon",
                                    if warning.existing_path.is_some() {
                                        "Upload and restart"
                                    } else {
                                        "Restart server"
                                    },
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        let Some(warning) = this.remote_restart.take() else {
                                            return;
                                        };
                                        this.start_connect(Some(warning), cx);
                                    },
                                )),
                            )
                    }))
                    .child(
                        div()
                            .flex()
                            .mt(px(8.))
                            .gap(px(8.))
                            .when(!self.connecting, |row| {
                                row.child(error_button("copy-error", "Copy error").on_click(
                                    move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            error.clone(),
                                        ))
                                    },
                                ))
                            })
                            .child(
                                error_button("close-error", "Close window")
                                    .on_click(|_, window, _| window.remove_window()),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
#[path = "../tests/unit/ui.rs"]
mod tests;
