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
    let error =
        anyhow::Error::from(io::Error::from(io::ErrorKind::UnexpectedEof)).context("frame failed");
    assert!(transport_error(&error));
    assert!(!transport_error(&anyhow::anyhow!("Session rejected")));
    assert!(!transport_error(&anyhow::Error::from(io::Error::from(
        io::ErrorKind::InvalidData
    ))));
}
