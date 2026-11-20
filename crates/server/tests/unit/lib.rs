use super::*;

#[test]
fn idle_timeout_and_eof_both_release_the_client() -> Result<()> {
    for disconnect in [false, true] {
        let (server, mut client) = scytty_common::stream::pair()?;
        server.set_read_timeout(Some(Duration::from_millis(25)))?;
        let (tx, events) = mpsc::sync_channel(4);
        let reader = thread::spawn(move || read_requests(server, 42, tx));
        protocol::send(&mut client, &Request::Ping)?;
        assert!(matches!(
            events.recv_timeout(Duration::from_secs(5))?,
            Event::Request(42, Request::Ping)
        ));
        if disconnect {
            client.shutdown(std::net::Shutdown::Both)?;
        }
        assert!(matches!(
            events.recv_timeout(Duration::from_secs(5))?,
            Event::Detached(42)
        ));
        reader.join().expect("client reader panicked");
    }
    Ok(())
}

#[test]
fn pty_pixel_geometry_uses_total_dimensions_and_saturates() {
    let size = pty_size(80, 24, 9, 18);
    assert_eq!(
        (size.cols, size.rows, size.pixel_width, size.pixel_height),
        (80, 24, 720, 432)
    );
    let size = pty_size(500, 300, 256, 256);
    assert_eq!((size.pixel_width, size.pixel_height), (u16::MAX, u16::MAX));
}

#[cfg(unix)]
#[test]
fn pty_ioctl_reports_gui_pixel_geometry_after_resize() -> Result<()> {
    let pair = native_pty_system().openpty(pty_size(80, 24, 0, 0))?;
    pair.master.resize(pty_size(153, 46, 9, 18))?;
    let size = pair.master.get_size()?;
    assert_eq!(
        (size.cols, size.rows, size.pixel_width, size.pixel_height),
        (153, 46, 1377, 828)
    );
    Ok(())
}

#[test]
fn terminal_answers_pixel_size_queries_from_cell_geometry() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    terminal.resize(153, 46, 9, 18)?;
    terminal.vt_write(b"\x1b[14t\x1b[16t\x1b[18t");
    let reply: Vec<u8> = terminal
        .events()
        .filter_map(|event| match event {
            VtEvent::PtyWrite(bytes) => Some(bytes),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(reply, b"\x1b[4;828;1377t\x1b[6;18;9t\x1b[8;46;153t");
    Ok(())
}
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
