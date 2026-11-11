use anyhow::Result;
use gpui::{prelude::*, *};
use std::process::Command;

pub const OVERLAY_SCROLLBAR: bool = false;
pub const WINDOWS_PATHS: bool = false;

pub fn set_progress(
    _progress: Option<&scytty_common::protocol::Progress>,
    _window: &Window,
    _cx: &mut App,
) -> Result<()> {
    Ok(())
}
pub fn clear_progress(_window: WindowId) {}

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

pub fn hide_console(_command: &mut Command) {}
