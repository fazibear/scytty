use anyhow::Result;
use gpui::{prelude::*, *};
use std::process::Command;

pub const OVERLAY_SCROLLBAR: bool = false;

pub fn set_progress(
    _progress: Option<&scytty_common::protocol::Progress>,
    _window: &Window,
) -> Result<()> {
    Ok(())
}
pub fn clear_progress(_window: WindowId) {}
pub fn tick_progress(_window: WindowId) {}

pub fn init_notifications() -> Result<()> {
    Ok(())
}

pub fn configure_notification(notification: &mut notify_rust::Notification) {
    let body = notification
        .body
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    notification.body(&body);
}

pub fn command_modifier(modifiers: Modifiers) -> bool {
    modifiers.control
}

pub fn clipboard_modifier(modifiers: Modifiers) -> bool {
    command_modifier(modifiers) && modifiers.shift
}

pub fn zoom_modifier(modifiers: Modifiers) -> bool {
    !modifiers.alt && modifiers.control
}

pub fn titlebar() -> Stateful<Div> {
    super::titlebar_base().on_mouse_down(MouseButton::Left, |event, window, _| {
        if event.click_count == 2 {
            window.zoom_window();
        } else {
            window.start_window_move();
        }
    })
}

pub fn window_controls(bar: Stateful<Div>) -> Stateful<Div> {
    bar
}

pub fn paint_scrollbar(
    window: &mut Window,
    track: Bounds<Pixels>,
    thumb: Bounds<Pixels>,
    _has_history: bool,
    _opacity: f32,
) {
    window.paint_quad(fill(track, rgb(0x171717)));
    window.paint_quad(fill(thumb, rgb(0x686868)));
}

pub fn connect_local() -> Result<crate::transport::Connection> {
    let mut command = Command::new(scytty_common::connection::server_executable()?);
    command.arg("--stdio");
    crate::transport::bridge(command)
}

pub fn hide_console(_command: &mut Command) {}
