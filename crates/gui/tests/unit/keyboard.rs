use super::*;
use ghostty_vt::TerminalOptions;
use gpui::Capslock;

fn stroke(key: &str, text: Option<&str>, modifiers: Modifiers) -> Keystroke {
    Keystroke {
        key: key.into(),
        key_char: text.map(str::to_owned),
        modifiers,
    }
}

fn send(terminal: &Terminal, stroke: &Keystroke, action: key::Action) -> Result<Option<Vec<u8>>> {
    encode(
        terminal,
        &mut key::Encoder::new()?,
        stroke,
        action,
        false,
        false,
    )
}

#[test]
fn legacy_keys_and_control_punctuation_use_the_encoder() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    for (name, expected) in [
        ("enter", "\r"),
        ("tab", "\t"),
        ("escape", "\x1b"),
        ("backspace", "\x7f"),
        ("up", "\x1b[A"),
        ("f1", "\x1bOP"),
    ] {
        assert_eq!(
            send(
                &terminal,
                &stroke(name, None, Modifiers::default()),
                key::Action::Press
            )?
            .unwrap(),
            expected.as_bytes(),
            "{name}"
        );
    }
    let ctrl = Modifiers {
        control: true,
        ..Default::default()
    };
    for (name, expected) in [
        ("a", 1),
        ("\\", 28),
        ("]", 29),
        ("^", 30),
        ("_", 31),
        ("space", 0),
    ] {
        assert_eq!(
            send(&terminal, &stroke(name, None, ctrl), key::Action::Press)?,
            Some(vec![expected]),
            "ctrl-{name}"
        );
    }
    let shift = Modifiers {
        shift: true,
        ..Default::default()
    };
    assert_eq!(
        send(&terminal, &stroke("[", None, ctrl), key::Action::Press)?.unwrap(),
        b"\x1b[91;5u"
    );
    assert_eq!(
        send(&terminal, &stroke("tab", None, shift), key::Action::Press)?.unwrap(),
        b"\x1b[Z"
    );
    terminal.vt_write(b"\x1b[?1h\x1b[?67h");
    assert_eq!(
        send(
            &terminal,
            &stroke("up", None, Modifiers::default()),
            key::Action::Repeat
        )?
        .unwrap(),
        b"\x1bOA"
    );
    assert_eq!(
        send(
            &terminal,
            &stroke("backspace", None, Modifiers::default()),
            key::Action::Press
        )?
        .unwrap(),
        b"\x08"
    );
    assert!(
        send(
            &terminal,
            &stroke("up", None, Modifiers::default()),
            key::Action::Release
        )?
        .is_none()
    );
    Ok(())
}

#[test]
fn modify_other_keys_and_alt_are_not_hand_encoded() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    let ctrl = Modifiers {
        control: true,
        ..Default::default()
    };
    let alt = Modifiers {
        alt: true,
        ..Default::default()
    };
    assert_eq!(
        send(&terminal, &stroke("a", Some("a"), alt), key::Action::Press)?.unwrap(),
        b"\x1ba"
    );
    terminal.vt_write(b"\x1b[>4;2m");
    assert_eq!(
        send(&terminal, &stroke("a", None, ctrl), key::Action::Press)?.unwrap(),
        b"\x1b[27;5;97~"
    );
    assert_eq!(
        send(&terminal, &stroke("a", Some("a"), alt), key::Action::Press)?.unwrap(),
        b"\x1b[27;3;97~"
    );
    terminal.vt_write(b"\x1b[>4;0m");
    assert_eq!(
        send(&terminal, &stroke("a", None, ctrl), key::Action::Press)?,
        Some(vec![1])
    );
    Ok(())
}

#[test]
fn kitty_flags_encode_press_repeat_release_alternates_and_associated_text() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    terminal.vt_write(b"\x1b[>31u");
    let plain = stroke("a", Some("a"), Modifiers::default());
    for (action, expected) in [
        (key::Action::Press, "\x1b[97;;97u"),
        (key::Action::Repeat, "\x1b[97;1:2;97u"),
        (key::Action::Release, "\x1b[97;1:3u"),
    ] {
        assert_eq!(
            send(&terminal, &plain, action)?.unwrap(),
            expected.as_bytes()
        );
    }
    let shift = Modifiers {
        shift: true,
        ..Default::default()
    };
    assert_eq!(
        send(
            &terminal,
            &stroke("a", Some("A"), shift),
            key::Action::Press
        )?
        .unwrap(),
        b"\x1b[97:65;2;65u"
    );
    let ctrl = Modifiers {
        control: true,
        ..Default::default()
    };
    assert_eq!(
        send(&terminal, &stroke("a", None, ctrl), key::Action::Press)?.unwrap(),
        b"\x1b[97;5u"
    );
    let super_mod = Modifiers {
        platform: true,
        ..Default::default()
    };
    assert_eq!(
        send(&terminal, &stroke("a", None, super_mod), key::Action::Press)?.unwrap(),
        b"\x1b[97;9u"
    );
    assert_eq!(
        send(
            &terminal,
            &stroke("é", Some("é"), Modifiers::default()),
            key::Action::Press
        )?
        .unwrap(),
        b"\x1b[233;;233u"
    );
    let caps = encode(
        &terminal,
        &mut key::Encoder::new()?,
        &plain,
        key::Action::Press,
        false,
        true,
    )?
    .unwrap();
    assert_eq!(caps, b"\x1b[97;65;97u");
    let shift_key = stroke("shift", None, shift);
    assert_eq!(
        send(&terminal, &shift_key, key::Action::Press)?.unwrap(),
        b"\x1b[57441;2u"
    );
    assert_eq!(
        send(
            &terminal,
            &stroke("enter", None, Modifiers::default()),
            key::Action::Press
        )?
        .unwrap(),
        b"\x1b[13u"
    );
    Ok(())
}

#[test]
fn negotiated_modes_survive_snapshot_and_pop_back_to_legacy() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    terminal.vt_write(b"\x1b[>1u\x1b[>31u");
    let escape = stroke("escape", None, Modifiers::default());
    let mut restored = crate::tests::roundtrip(&terminal)?;
    assert_eq!(
        send(&terminal, &escape, key::Action::Press)?,
        send(&restored, &escape, key::Action::Press)?
    );
    restored.vt_write(b"\x1b[<u");
    assert_eq!(
        send(&restored, &escape, key::Action::Press)?.unwrap(),
        b"\x1b[27u"
    );
    restored.vt_write(b"\x1b[<u");
    assert_eq!(
        send(&restored, &escape, key::Action::Press)?.unwrap(),
        b"\x1b"
    );
    Ok(())
}

#[test]
fn native_text_and_composition_are_not_sent_twice() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    for (name, text, modifiers) in [
        ("a", "a", Modifiers::default()),
        (
            "a",
            "A",
            Modifiers {
                shift: true,
                ..Default::default()
            },
        ),
        ("space", " ", Modifiers::default()),
        ("e", "é", Modifiers::default()),
    ] {
        assert!(
            send(
                &terminal,
                &stroke(name, Some(text), modifiers),
                key::Action::Press
            )?
            .is_none()
        );
    }
    for mode in [b"\x1b[>31u".as_slice(), b"\x1b[<u".as_slice()] {
        terminal.vt_write(mode);
        assert!(
            send(
                &terminal,
                &stroke("'", None, Modifiers::default()),
                key::Action::Press
            )?
            .is_none()
        );
        let dead = stroke(
            "e",
            None,
            Modifiers {
                alt: true,
                ..Default::default()
            },
        );
        assert!(send(&terminal, &dead, key::Action::Press)?.is_none());
        for name in ["a", "enter", "escape"] {
            assert!(
                encode(
                    &terminal,
                    &mut key::Encoder::new()?,
                    &stroke(name, Some("a"), Modifiers::default()),
                    key::Action::Press,
                    true,
                    false
                )?
                .is_none()
            );
        }
    }
    assert!(
        send(
            &terminal,
            &stroke("unknown", None, Modifiers::default()),
            key::Action::Press
        )?
        .is_none()
    );
    Ok(())
}

#[test]
fn releases_follow_the_original_tab_and_only_forwarded_presses() {
    let mut keyboard = Keyboard::default();
    let press = stroke(
        "a",
        Some("A"),
        Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    keyboard.remember(7, &press);
    keyboard.remember(9, &press);
    assert_eq!(keyboard.target(&press), Some(7));
    let release = stroke("a", None, Modifiers::default());
    let (id, held) = keyboard.release(&release).unwrap();
    assert_eq!(id, 7);
    assert_eq!(held.modifiers, Modifiers::default());
    assert!(keyboard.release(&release).is_none());
    keyboard.remember(9, &press);
    assert_eq!(keyboard.clear().len(), 1);
    assert!(keyboard.release(&release).is_none());
}

#[test]
fn modifier_transitions_and_extended_function_keys() {
    let mut keyboard = Keyboard::default();
    let event = ModifiersChangedEvent {
        modifiers: Modifiers {
            control: true,
            ..Default::default()
        },
        capslock: Capslock { on: true },
    };
    let changes = keyboard.modifiers_changed(&event);
    assert_eq!(changes.len(), 3);
    assert_eq!(changes[1].0.key, "capslock");
    assert_eq!(changes[1].1, key::Action::Press);
    assert_eq!(changes[2].1, key::Action::Release);
    assert_eq!(changes[0].0.key, "control");
    assert_eq!(changes[0].1, key::Action::Press);
    assert!(keyboard.caps_lock);
    assert!(!keyboard.modifiers.shift);
    assert!(keyboard.modifiers_changed(&event).is_empty());
    assert_eq!(
        keyboard.modifiers_changed(&ModifiersChangedEvent::default())[0].1,
        key::Action::Release
    );
    assert_eq!(terminal_key("f13"), Some(key::Key::F13));
    assert_eq!(terminal_key("f25"), Some(key::Key::F25));
    assert_eq!(terminal_key("f26"), None);
    assert_eq!(terminal_key("menu"), Some(key::Key::ContextMenu));
}

#[test]
fn kitty_event_reporting_is_opt_in_and_keeps_release_identity() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    let escape = stroke("escape", None, Modifiers::default());
    terminal.vt_write(b"\x1b[>1u");
    assert_eq!(
        send(&terminal, &escape, key::Action::Press)?.unwrap(),
        b"\x1b[27u"
    );
    assert!(send(&terminal, &escape, key::Action::Release)?.is_none());
    terminal.vt_write(b"\x1b[>3u");
    assert_eq!(
        send(&terminal, &escape, key::Action::Repeat)?.unwrap(),
        b"\x1b[27;1:2u"
    );
    assert_eq!(
        send(&terminal, &escape, key::Action::Release)?.unwrap(),
        b"\x1b[27;1:3u"
    );
    assert!(
        send(
            &terminal,
            &stroke("enter", None, Modifiers::default()),
            key::Action::Release
        )?
        .is_none()
    );
    terminal.vt_write(b"\x1b[>31u");
    let mut keyboard = Keyboard::default();
    keyboard.remember(
        7,
        &stroke(
            "a",
            Some("A"),
            Modifiers {
                shift: true,
                ..Default::default()
            },
        ),
    );
    let (_, released) = keyboard
        .release(&stroke("a", None, Modifiers::default()))
        .unwrap();
    assert_eq!(
        send(&terminal, &released, key::Action::Release)?.unwrap(),
        b"\x1b[97;1:3u"
    );
    let alt = Modifiers {
        alt: true,
        ..Default::default()
    };
    assert_eq!(
        send(&terminal, &stroke("a", None, alt), key::Action::Release)?.unwrap(),
        b"\x1b[97;3:3u"
    );
    Ok(())
}

#[test]
fn gpui_named_text_and_kitty_f26_through_f35() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    let named = stroke("udiaeresis", Some("ü"), Modifiers::default());
    assert!(send(&terminal, &named, key::Action::Press)?.is_none());
    assert!(
        send(
            &terminal,
            &stroke("f26", None, Modifiers::default()),
            key::Action::Press
        )?
        .is_none()
    );
    terminal.vt_write(b"\x1b[>31u");
    assert_eq!(
        send(&terminal, &named, key::Action::Press)?.unwrap(),
        b"\x1b[252;;252u"
    );
    for n in 26..=35 {
        let stroke = stroke(&format!("f{n}"), None, Modifiers::default());
        assert_eq!(
            send(&terminal, &stroke, key::Action::Press)?.unwrap(),
            format!("\x1b[{}u", 57364 + n - 1).as_bytes()
        );
        assert_eq!(
            send(&terminal, &stroke, key::Action::Release)?.unwrap(),
            format!("\x1b[{};1:3u", 57364 + n - 1).as_bytes()
        );
    }
    terminal.vt_write(b"\x1b[<u\x1b[>1u");
    let f26 = stroke("f26", None, Modifiers::default());
    assert_eq!(
        send(&terminal, &f26, key::Action::Press)?.unwrap(),
        b"\x1b[57389u"
    );
    assert!(send(&terminal, &f26, key::Action::Release)?.is_none());
    assert!(
        send(
            &terminal,
            &stroke("f36", None, Modifiers::default()),
            key::Action::Press
        )?
        .is_none()
    );
    assert_eq!(terminal_key("xf86homepage"), Some(key::Key::BrowserHome));
    assert_eq!(terminal_key("xf86audiomedia"), Some(key::Key::MediaSelect));
    Ok(())
}

#[test]
fn shifted_text_uses_gpui_consumed_modifiers() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    // GPUI supplies shifted punctuation as the key itself, with Shift consumed.
    for ch in "~!@#$%^&*()_+{}|:\"<>?".chars() {
        let text = ch.to_string();
        let stroke = stroke(&text, Some(&text), Modifiers::default());
        assert!(
            send(&terminal, &stroke, key::Action::Press)?.is_none(),
            "{ch} must use native text insertion"
        );
    }
    let upper = stroke(
        "a",
        Some("A"),
        Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    assert!(send(&terminal, &upper, key::Action::Press)?.is_none());
    terminal.vt_write(b"\x1b[>31u");
    let tilde = stroke("~", Some("~"), Modifiers::default());
    assert_eq!(
        send(&terminal, &tilde, key::Action::Press)?.unwrap(),
        b"\x1b[126;;126u"
    );
    assert_eq!(
        send(&terminal, &tilde, key::Action::Release)?.unwrap(),
        b"\x1b[126;1:3u"
    );
    Ok(())
}

#[test]
fn window_modifier_sync_does_not_invent_key_presses() {
    let mut keyboard = Keyboard::default();
    let modifiers = Modifiers {
        shift: true,
        ..Default::default()
    };
    keyboard.sync(modifiers, true);
    assert!(keyboard.modifiers.shift);
    assert!(keyboard.caps_lock);
    assert!(
        keyboard
            .modifiers_changed(&ModifiersChangedEvent {
                modifiers,
                capslock: Capslock { on: true }
            })
            .is_empty()
    );
    assert!(keyboard.clear().is_empty());
    assert!(!keyboard.modifiers.shift);
}

#[test]
fn caps_lock_toggles_produce_balanced_kitty_events() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    terminal.vt_write(b"\x1b[>31u");
    let mut keyboard = Keyboard::default();
    for on in [true, false] {
        let changes = keyboard.modifiers_changed(&ModifiersChangedEvent {
            capslock: Capslock { on },
            ..Default::default()
        });
        assert_eq!(changes.len(), 2);
        for (stroke, action) in changes {
            let bytes = encode(
                &terminal,
                &mut key::Encoder::new()?,
                &stroke,
                action,
                false,
                keyboard.caps_lock,
            )?
            .unwrap();
            assert!(String::from_utf8(bytes)?.contains("57358"));
        }
    }
    Ok(())
}

#[test]
fn available_keypad_operators_follow_application_mode() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    let add = stroke("add", None, Modifiers::default());
    assert_eq!(send(&terminal, &add, key::Action::Press)?.unwrap(), b"+");
    terminal.vt_write(b"\x1b=");
    assert!(terminal.mode(ghostty_vt::terminal::Mode::KEYPAD_KEYS)?);
    assert_eq!(send(&terminal, &add, key::Action::Press)?.unwrap(), b"+");
    terminal.vt_write(b"\x1b[?1035l");
    assert_eq!(
        send(&terminal, &add, key::Action::Press)?.unwrap(),
        b"\x1bOk"
    );
    terminal.vt_write(b"\x1b>");
    assert_eq!(send(&terminal, &add, key::Action::Press)?.unwrap(), b"+");
    Ok(())
}
