use anyhow::Result;
use gpui::{prelude::*, *};
use std::{os::windows::process::CommandExt, process::Command};

pub const OVERLAY_SCROLLBAR: bool = true;

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
    super::titlebar_base()
        .pl(px(8.))
        // GPUI's start_window_move is a no-op on Windows; use native caption hit-testing.
        .window_control_area(WindowControlArea::Drag)
        .on_mouse_down(MouseButton::Left, |event, window, _| {
            if event.click_count == 2 {
                window.zoom_window();
            }
        })
}

pub fn window_controls(mut bar: Stateful<Div>) -> Stateful<Div> {
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
    hide_console(&mut command);
    crate::transport::bridge(command)
}

pub fn hide_console(command: &mut Command) {
    // ssh.exe otherwise creates a visible console window for each GUI child.
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}
