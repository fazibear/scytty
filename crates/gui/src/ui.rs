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

fn error_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .mt(px(8.))
        .p(px(8.))
        .rounded_xl()
        .bg(rgb(0x38204f))
        .border_2()
        .border_color(rgb(0xac50f5))
        .text_color(rgb(0xffffff))
        .font_weight(FontWeight::BOLD)
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
        let mut line = div().flex().items_center().flex_shrink_0();
        let segments =
            self.config
                .status_text(name, title, index, active, self.host.is_some(), tab)?;
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
    pub(super) fn menu_action(
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
    pub(super) fn header(&self, cx: &mut Context<Self>) -> Result<AnyElement> {
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
                        img(images::error_reaper())
                            .size(px(240.))
                            .max_w(relative(1.))
                            .flex_shrink_0()
                            .object_fit(ObjectFit::Contain),
                    )
                    .children(self.remote_restart.as_ref().map(|_| {
                        error_button(
                            "restart-remote-daemon",
                            "Restart with bundled server — ends ALL sessions",
                        )
                        .mt(px(16.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            let Some(warning) = this.remote_restart.take() else {
                                return;
                            };
                            this.start_connect(Some(warning), cx);
                        }))
                    }))
                    .when(!self.connecting, |panel| {
                        panel.child(error_button("copy-error", "Copy error").on_click(
                            move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(error.clone()))
                            },
                        ))
                    })
                    .child(
                        error_button("close-error", "Close window")
                            .on_click(|_, window, _| window.remove_window()),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[test]
    fn message_rows_preserve_unicode_and_blank_lines() {
        assert_eq!(
            message_line_rows("hello 世界", [6].into_iter()),
            ["hello", "世界"]
        );
        assert_eq!(message_line_rows("", std::iter::empty()), [""]);
        assert_eq!(message_line_rows("short", std::iter::empty()), ["short"]);
    }
}
