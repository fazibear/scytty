use gpui::{prelude::*, *};

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), path = "unix.rs")]
mod current;
pub use current::*;

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
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn keyboard_modifiers() {
        let macos = cfg!(target_os = "macos");
        let control = Modifiers {
            control: true,
            ..Default::default()
        };
        let command = Modifiers {
            platform: true,
            ..Default::default()
        };
        assert_eq!(command_modifier(control), !macos);
        assert_eq!(command_modifier(command), macos);
        assert!(!clipboard_modifier(control));
        assert_eq!(clipboard_modifier(command), macos);
        assert_eq!(
            clipboard_modifier(Modifiers {
                shift: true,
                ..control
            }),
            !macos
        );
        assert!(zoom_modifier(control));
        assert_eq!(zoom_modifier(command), macos);
        assert!(!zoom_modifier(Modifiers {
            alt: true,
            ..control
        }));
    }
}
