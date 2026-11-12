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
