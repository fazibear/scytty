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
    assert!(!command_modifier(control));
    assert!(command_modifier(command));
    assert!(!command_modifier(Modifiers {
        shift: true,
        ..control
    }));
    assert!(command_modifier(Modifiers {
        shift: true,
        ..command
    }));
}
