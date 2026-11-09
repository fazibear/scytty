use gpui::{prelude::*, *};

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), path = "unix.rs")]
mod current;
pub use current::*;

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
        let control = Modifiers {
            control: true,
            ..Default::default()
        };
        let command = Modifiers {
            platform: true,
            ..Default::default()
        };
        assert_eq!(command_modifier(control), !OVERLAY_SCROLLBAR);
        assert_eq!(command_modifier(command), OVERLAY_SCROLLBAR);
        assert!(!clipboard_modifier(control));
        assert_eq!(clipboard_modifier(command), OVERLAY_SCROLLBAR);
        assert_eq!(
            clipboard_modifier(Modifiers {
                shift: true,
                ..control
            }),
            !OVERLAY_SCROLLBAR
        );
        assert!(zoom_modifier(control));
        assert_eq!(zoom_modifier(command), OVERLAY_SCROLLBAR);
        assert!(!zoom_modifier(Modifiers {
            alt: true,
            ..control
        }));
    }
}
