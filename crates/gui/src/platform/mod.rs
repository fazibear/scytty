use gpui::{prelude::*, *};

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), path = "unix.rs")]
mod current;
pub use current::*;

pub fn focus_terminal(focus: &FocusHandle, window: &mut Window) {
    #[cfg(target_os = "windows")]
    current::focus_native_window(window);
    focus.focus(window);
}

pub fn command_modifier(modifiers: Modifiers) -> bool {
    if cfg!(target_os = "windows") {
        modifiers.control
    } else {
        modifiers.platform
    }
}

pub fn clipboard_modifier(modifiers: Modifiers) -> bool {
    command_modifier(modifiers) && (!cfg!(target_os = "windows") || modifiers.shift)
}

pub fn paint_scrollbar(
    window: &mut Window,
    track: Bounds<Pixels>,
    thumb: Bounds<Pixels>,
    has_history: bool,
    opacity: f32,
) {
    if OVERLAY_SCROLLBAR {
        if !has_history {
            return;
        }
        let mut color = rgba(0xffffff70);
        color.a *= opacity;
        let mut thumb = fill(thumb, color);
        thumb.corner_radii = (thumb.bounds.size.width / 2.).into();
        window.paint_quad(thumb);
    } else {
        window.paint_quad(fill(track, rgb(0x171717)));
        window.paint_quad(fill(thumb, rgb(0x686868)));
    }
}

fn titlebar_base() -> Stateful<Div> {
    div()
        .id("titlebar")
        .flex()
        .items_center()
        .h(px(32.))
        .line_height(px(28.))
        .pt(px(3.5))
        .flex_shrink_0()
        .overflow_hidden()
}

pub fn titlebar_options() -> TitlebarOptions {
    TitlebarOptions {
        title: Some("Scytty".into()),
        appears_transparent: true,
        traffic_light_position: Some(point(px(12.), px(10.))),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/platform/mod.rs"]
mod tests;
