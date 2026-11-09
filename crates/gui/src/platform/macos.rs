use anyhow::Result;
use gpui::{prelude::*, *};
use std::process::Command;

pub const OVERLAY_SCROLLBAR: bool = true;

pub fn command_modifier(modifiers: Modifiers) -> bool {
    modifiers.platform
}

pub fn clipboard_modifier(modifiers: Modifiers) -> bool {
    command_modifier(modifiers)
}

pub fn zoom_modifier(modifiers: Modifiers) -> bool {
    !modifiers.alt && (modifiers.control || modifiers.platform)
}

pub fn titlebar() -> Stateful<Div> {
    super::titlebar_base().pl(px(78.)).pr(px(8.)).on_mouse_down(
        MouseButton::Left,
        |event, window, _| {
            if event.click_count == 2 {
                window.zoom_window();
            } else {
                window.start_window_move();
            }
        },
    )
}

pub fn window_controls(bar: Stateful<Div>) -> Stateful<Div> {
    bar
}

pub fn paint_scrollbar(
    window: &mut Window,
    _track: Bounds<Pixels>,
    thumb: Bounds<Pixels>,
    has_history: bool,
    opacity: f32,
) {
    if !has_history {
        return;
    }
    let mut color = rgba(0xffffff70);
    color.a *= opacity;
    let mut thumb = fill(thumb, color);
    thumb.corner_radii = (thumb.bounds.size.width / 2.).into();
    window.paint_quad(thumb);
}

pub fn connect_local() -> Result<crate::transport::Connection> {
    let mut command = Command::new(scytty_common::connection::server_executable()?);
    command.arg("--stdio");
    crate::transport::bridge(command)
}

pub fn hide_console(_command: &mut Command) {}
