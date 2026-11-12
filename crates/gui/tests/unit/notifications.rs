use super::*;
use ghostty_vt::{Terminal, TerminalOptions};

#[test]
fn osc_notifications_survive_split_output() -> anyhow::Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    terminal.vt_write(b"\x1b]9;4;1;50\x07\x1b]777;notify;Build");
    assert!(terminal.events().filter_map(message).next().is_none());
    terminal.vt_write(b";Finished\x07\x1b]9;Hello\x1b\\");
    let messages: Vec<_> = terminal.events().filter_map(message).collect();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].title, "Build");
    assert_eq!(messages[0].body, "Finished");
    assert_eq!(messages[1].title, "Scytty");
    assert_eq!(messages[1].body, "Hello");
    Ok(())
}

#[test]
fn notification_text_is_bounded_and_controls_removed() {
    assert_eq!(text("a\0\x1b\nb", 128), "ab");
    assert_eq!(text(&"🦀".repeat(200), 128).chars().count(), 128);
    assert!(message(Event::Bell).is_none());
    assert!(
        message(Event::DesktopNotification {
            title: String::new(),
            body: String::new()
        })
        .is_none()
    );
}
