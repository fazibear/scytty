use std::{
    sync::{OnceLock, mpsc},
    time::{Duration, Instant},
};

use ghostty_vt::terminal::Event;

struct Message {
    title: String,
    body: String,
}

fn text(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(limit)
        .collect()
}

fn message(event: Event) -> Option<Message> {
    let Event::DesktopNotification { title, body } = event else {
        return None;
    };
    let title = text(&title, 128);
    let body = text(&body, 1024);
    if title.is_empty() && body.is_empty() {
        return None;
    }
    Some(Message {
        title: if title.is_empty() {
            "Scytty".into()
        } else {
            title
        },
        body,
    })
}

pub fn handle(event: Event) {
    let Some(message) = message(event) else {
        return;
    };
    static SENDER: OnceLock<Option<mpsc::SyncSender<Message>>> = OnceLock::new();
    let sender = SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Message>(1);
        match std::thread::Builder::new()
            .name("notifications".into())
            .spawn(move || {
                if let Err(error) = crate::platform::init_notifications() {
                    eprintln!("Could not initialize desktop notifications: {error:#}");
                    return;
                }
                let mut last = None;
                for message in rx {
                    let now = Instant::now();
                    if last.is_some_and(|last| now.duration_since(last) < Duration::from_secs(2)) {
                        continue;
                    }
                    last = Some(now);
                    let mut notification = notify_rust::Notification::new();
                    notification
                        .appname("Scytty")
                        .summary(&message.title)
                        .body(&message.body);
                    crate::platform::configure_notification(&mut notification);
                    if let Err(error) = notification.show() {
                        eprintln!("Could not show desktop notification: {error}");
                    }
                }
            }) {
            Ok(_) => Some(tx),
            Err(error) => {
                eprintln!("Could not start desktop notifications: {error}");
                None
            }
        }
    });
    if let Some(sender) = sender {
        // Drop excess requests instead of letting terminal output flood the desktop.
        let _ = sender.try_send(message);
    }
}

#[cfg(test)]
mod tests {
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
}
