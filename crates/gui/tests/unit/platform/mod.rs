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
    let windows = cfg!(target_os = "windows");
    assert_eq!(command_modifier(control), windows);
    assert_eq!(command_modifier(command), !windows);
    assert!(!clipboard_modifier(control));
    assert_eq!(clipboard_modifier(command), !windows);
    assert_eq!(
        clipboard_modifier(Modifiers {
            shift: true,
            ..control
        }),
        windows
    );
    assert_eq!(
        command_modifier(Modifiers {
            shift: true,
            ..control
        }),
        windows
    );
    assert_eq!(
        command_modifier(Modifiers {
            shift: true,
            ..command
        }),
        !windows
    );
}
