use anyhow::{Result, bail};
use scytty_common::protocol::{self, Request, Update};
use std::{
    os::unix::net::UnixStream,
    process::{Child, Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Server(Child, std::path::PathBuf);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = std::fs::remove_dir_all(&self.1);
    }
}
fn state(stream: &mut UnixStream) -> Result<(Vec<u64>, u64)> {
    for _ in 0..100 {
        match protocol::receive(stream)? {
            Update::State { tabs, active } => {
                return Ok((tabs.iter().map(|t| t.id).collect(), active));
            }
            Update::Error(e) => bail!(e),
            _ => {}
        }
    }
    bail!("no state update")
}
fn assert_session_env(stream: &mut UnixStream, id: u64) -> Result<()> {
    protocol::send(
        stream,
        &Request::Input {
            id,
            bytes: b"printf '\\033]0;%s\\007' \"$SCYTTY_SESSION\"\n".to_vec(),
        },
    )?;
    loop {
        if let Update::State { tabs, .. } = protocol::receive(stream)?
            && tabs.iter().any(|t| t.id == id && t.title == "test")
        {
            return Ok(());
        }
    }
}
#[test]
fn tabs_and_reattach() -> Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "scytty-test-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    std::fs::create_dir(&dir)?;
    // A deterministic shell without user startup scripts; no child jobs to leak on failure.
    let child = Command::new(env!("CARGO_BIN_EXE_scytty-server"))
        .arg("server")
        .env("HOME", &dir)
        .env("SHELL", "/bin/sh")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let _server = Server(child, dir.clone());
    let path = dir.join(".scytty/server.sock");
    let mut connection = None;
    for _ in 0..100 {
        if let Ok(stream) = UnixStream::connect(&path) {
            connection = Some(stream);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut stream = connection.ok_or_else(|| anyhow::anyhow!("server failed to start"))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    protocol::handshake(&mut stream)?;
    protocol::send(
        &mut stream,
        &Request::Attach {
            session: "test".into(),
        },
    )?;
    let (tabs, first) = state(&mut stream)?;
    assert_eq!(tabs, [first]);
    assert_session_env(&mut stream, first)?;
    protocol::send(
        &mut stream,
        &Request::Input {
            id: first,
            bytes: b"printf '\\033]1337;SetUserVar=right_status=aGVsbG8=\\007'\n".to_vec(),
        },
    )?;
    loop {
        if let Update::State { tabs, .. } = protocol::receive(&mut stream)?
            && tabs.iter().any(|t| {
                t.user_vars
                    .get("right_status")
                    .is_some_and(|s| s == "hello")
            })
        {
            break;
        }
    }
    for (code, expected) in [
        (1, Some("normal")),
        (2, Some("error")),
        (3, Some("indeterminate")),
        (4, Some("paused")),
        (0, None),
        (1, Some("normal")),
    ] {
        protocol::send(
            &mut stream,
            &Request::Input {
                id: first,
                bytes: format!("printf '\\033]9;4;{code};42\\007'\n").into_bytes(),
            },
        )?;
        loop {
            if let Update::State { tabs, .. } = protocol::receive(&mut stream)? {
                let progress = &tabs.iter().find(|t| t.id == first).unwrap().progress;
                assert_eq!(progress.as_ref().map(|p| p.state.as_str()), expected);
                if let Some(p) = progress {
                    assert_eq!(p.percent, if code == 3 { None } else { Some(42) });
                }
                break;
            }
        }
    }
    let mut duplicate = UnixStream::connect(&path)?;
    duplicate.set_read_timeout(Some(Duration::from_secs(5)))?;
    protocol::handshake(&mut duplicate)?;
    protocol::send(
        &mut duplicate,
        &Request::Attach {
            session: "test".into(),
        },
    )?;
    assert!(
        matches!(protocol::receive::<Update>(&mut duplicate)?, Update::Error(message) if message.contains("already attached"))
    );
    drop(duplicate);
    protocol::send(&mut stream, &Request::SetScrollback { megabytes: 65 })?;
    loop {
        if let Update::Error(message) = protocol::receive(&mut stream)? {
            assert!(message.contains("scrollback_mb"));
            break;
        }
    }
    protocol::send(&mut stream, &Request::SetScrollback { megabytes: 32 })?;
    protocol::send(&mut stream, &Request::NewTab)?;
    let (tabs, second) = loop {
        let state = state(&mut stream)?;
        if state.0.len() == 2 {
            break state;
        }
    };
    assert_ne!(first, second);
    assert_session_env(&mut stream, second)?;
    protocol::send(&mut stream, &Request::Activate { id: first })?;
    loop {
        if state(&mut stream)?.1 == first {
            break;
        }
    }
    protocol::send(&mut stream, &Request::Input {
        id: first,
        bytes: b"printf '\\033_Ga=T,q=2,f=24,s=1,v=1,i=7,c=2,r=2;/wAA\\033\\134\\033]0;image-ready\\007'\n".to_vec(),
    })?;
    loop {
        if let Update::State { tabs, .. } = protocol::receive(&mut stream)?
            && tabs
                .iter()
                .any(|t| t.id == first && t.title == "image-ready")
        {
            break;
        }
    }
    stream.shutdown(std::net::Shutdown::Both)?;
    drop(stream);
    std::thread::sleep(Duration::from_millis(100));
    let mut stream = UnixStream::connect(&path)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    protocol::handshake(&mut stream)?;
    protocol::send(
        &mut stream,
        &Request::Attach {
            session: "test".into(),
        },
    )?;
    let mut restored_image = false;
    let (restored, active) = loop {
        let update = protocol::receive(&mut stream)?;
        if let Update::Snapshot { bytes, .. } = &update {
            let terminal = ghostty_vt::snapshot::Decoder::from_slice(bytes).decode()?;
            assert_eq!(terminal.scrollback_max_bytes()?, Some(32 * 1024 * 1024));
        }
        if let Update::Snapshot { id, bytes } = &update
            && *id == first
        {
            let terminal = ghostty_vt::snapshot::Decoder::from_slice(bytes).decode()?;
            assert_eq!(
                terminal.kitty_graphics()?.image(7).unwrap().data()?,
                [255, 0, 0]
            );
            restored_image = true;
        }
        if let Update::State { tabs, active } = update {
            assert_eq!(
                tabs.iter().find(|t| t.id == first).unwrap().user_vars["right_status"],
                "hello"
            );
            let progress = tabs
                .iter()
                .find(|t| t.id == first)
                .unwrap()
                .progress
                .as_ref()
                .unwrap();
            assert_eq!(progress.state, "normal");
            assert_eq!(progress.percent, Some(42));
            break (tabs.iter().map(|t| t.id).collect::<Vec<_>>(), active);
        }
    };
    assert!(restored_image);
    assert_eq!(restored, tabs);
    assert_eq!(active, first);
    protocol::send(
        &mut stream,
        &Request::Input {
            id: first,
            bytes: b"exit\n".to_vec(),
        },
    )?;
    loop {
        let (remaining, active) = state(&mut stream)?;
        if remaining == [second] {
            assert_eq!(active, second);
            break;
        }
    }
    protocol::send(
        &mut stream,
        &Request::Input {
            id: second,
            bytes: vec![4],
        },
    )?;
    loop {
        let (remaining, active) = state(&mut stream)?;
        if remaining.is_empty() {
            assert_eq!(active, 0);
            break;
        }
    }
    // Keep the old client open: EOF alone must free the name, without a GUI
    // CloseSession request or waiting for a detach event.
    let mut replacement = UnixStream::connect(&path)?;
    replacement.set_read_timeout(Some(Duration::from_secs(5)))?;
    protocol::handshake(&mut replacement)?;
    protocol::send(
        &mut replacement,
        &Request::Attach {
            session: "test".into(),
        },
    )?;
    let (tabs, active) = state(&mut replacement)?;
    drop(stream);
    let mut stream = replacement;
    assert_eq!(tabs, [active]);
    assert_ne!(active, first);
    assert_ne!(active, second);
    protocol::send(&mut stream, &Request::CloseSession)?;
    // Closing a window destroys its session rather than leaving a detached shell.
    loop {
        if protocol::receive::<Update>(&mut stream).is_err() {
            break;
        }
    }
    let mut stream = UnixStream::connect(&path)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    protocol::handshake(&mut stream)?;
    protocol::send(
        &mut stream,
        &Request::Attach {
            session: "test".into(),
        },
    )?;
    let (tabs, fresh) = state(&mut stream)?;
    assert_eq!(tabs, [fresh]);
    assert_ne!(fresh, active);
    Ok(())
}
