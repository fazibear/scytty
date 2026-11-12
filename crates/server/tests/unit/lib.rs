use super::*;
#[test]
fn event_errors_only_reach_the_responsible_client() -> Result<()> {
    let mut sessions = HashMap::new();
    let mut receivers = Vec::new();
    let mut peers = Vec::new();
    for id in [1, 2] {
        let (socket, peer) = scytty_common::stream::pair()?;
        peers.push(peer);
        let (output, updates) = mpsc::sync_channel(4);
        receivers.push(updates);
        sessions.insert(
            id.to_string(),
            Session {
                bin: None,
                scrollback_mb: 16,
                tabs: vec![],
                active: 0,
                client: Some(Client { id, output, socket }),
            },
        );
    }
    for event in [
        Event::Request(1, Request::NewTab),
        Event::Output("1".into(), 0, vec![]),
        Event::End("1".into(), 0, None),
    ] {
        error_output(&event, &sessions)
            .unwrap()
            .send(Update::Error("failed".into()))?;
        assert!(matches!(receivers[0].try_recv()?, Update::Error(message) if message == "failed"));
        assert!(matches!(
            receivers[1].try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }
    assert!(error_output(&Event::Request(99, Request::NewTab), &sessions).is_none());
    assert!(error_output(&Event::Shutdown, &sessions).is_none());
    Ok(())
}

#[test]
fn output_bursts_wait_for_queue_capacity() -> Result<()> {
    let (socket, _peer) = scytty_common::stream::pair()?;
    let (output, updates) = mpsc::sync_channel(1);
    let client = Client {
        id: 1,
        socket,
        output,
    };
    client.send(Update::Output {
        id: 1,
        bytes: vec![0],
    })?;
    let (started, waiting) = mpsc::channel();
    let (finished, done) = mpsc::channel();
    let producer = thread::spawn(move || {
        started.send(()).unwrap();
        let result = client.send(Update::Output {
            id: 1,
            bytes: vec![1],
        });
        finished.send(result).unwrap();
    });
    waiting.recv()?;
    assert!(matches!(
        done.recv_timeout(Duration::from_millis(50)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert!(matches!(updates.recv()?, Update::Output { bytes, .. } if bytes == [0]));
    done.recv_timeout(Duration::from_secs(2))??;
    assert!(matches!(updates.recv()?, Update::Output { bytes, .. } if bytes == [1]));
    producer.join().unwrap();
    Ok(())
}

#[test]
fn clipboard_capability_is_advertised() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    scytty_common::clipboard::configure(&mut terminal)?;
    terminal.vt_write(b"\x1bP+q4D73\x1b\\");
    let replies: Vec<_> = terminal
        .events()
        .filter_map(|event| match event {
            VtEvent::PtyWrite(bytes) => Some(bytes),
            _ => None,
        })
        .collect();
    assert!(
        replies
            .iter()
            .any(|bytes| bytes.starts_with(b"\x1bP1+r4D73="))
    );
    Ok(())
}

#[test]
fn bridge_flushes_without_newlines() -> std::io::Result<()> {
    let mut output = Vec::new();
    {
        let mut buffered = std::io::BufWriter::new(&mut output);
        copy_flushed(&mut &b"\0\x01\x02"[..], &mut buffered)?;
        assert!(buffered.buffer().is_empty());
    }
    assert_eq!(output, b"\0\x01\x02");
    Ok(())
}
