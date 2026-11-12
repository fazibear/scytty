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
#[path = "../tests/unit/notifications.rs"]
mod tests;
