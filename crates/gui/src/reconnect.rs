use std::{
    io,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub struct IncompatiblePeer;

impl std::fmt::Display for IncompatiblePeer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Server protocol is incompatible")
    }
}
impl std::error::Error for IncompatiblePeer {}

pub struct Reconnect {
    pub attempts: u32,
    next: Option<Instant>,
}

impl Reconnect {
    pub fn new(now: Instant) -> Self {
        Self {
            attempts: 1,
            next: Some(now + delay(1)),
        }
    }

    pub fn failed(&mut self, now: Instant) {
        self.attempts = self.attempts.saturating_add(1);
        self.next = Some(now + delay(self.attempts));
    }

    pub fn start_if_due(&mut self, now: Instant) -> bool {
        if self.next.is_some_and(|next| now >= next) {
            self.next = None;
            true
        } else {
            false
        }
    }
}

fn delay(attempt: u32) -> Duration {
    Duration::from_secs((1u64 << attempt.saturating_sub(1).min(5)).min(30))
}

pub fn transport_error(error: &anyhow::Error) -> bool {
    error.chain().any(|source| {
        source.downcast_ref::<io::Error>().is_some_and(|error| {
            matches!(
                error.kind(),
                io::ErrorKind::UnexpectedEof
                    | io::ErrorKind::BrokenPipe
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::NotConnected
                    | io::ErrorKind::TimedOut
                    | io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::NetworkUnreachable
                    | io::ErrorKind::HostUnreachable
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_back_off_and_only_one_attempt_runs_at_a_time() {
        let mut now = Instant::now();
        let mut retry = Reconnect::new(now);
        for seconds in [1, 2, 4, 8, 16, 30, 30, 30] {
            assert!(!retry.start_if_due(now));
            assert!(!retry.start_if_due(now + Duration::from_millis(seconds * 1000 - 1)));
            now += Duration::from_secs(seconds);
            assert!(retry.start_if_due(now));
            assert!(!retry.start_if_due(now + Duration::from_secs(100)));
            retry.failed(now);
        }
        assert_eq!(delay(u32::MAX), Duration::from_secs(30));
    }

    #[test]
    fn retries_connection_loss_but_not_invalid_data_or_application_errors() {
        let error = anyhow::Error::from(io::Error::from(io::ErrorKind::UnexpectedEof))
            .context("frame failed");
        assert!(transport_error(&error));
        assert!(!transport_error(&anyhow::anyhow!("Session rejected")));
        assert!(!transport_error(&anyhow::Error::from(io::Error::from(
            io::ErrorKind::InvalidData
        ))));
    }
}
