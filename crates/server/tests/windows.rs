#![cfg(windows)]

use anyhow::{Context, Result, bail};
use scytty_common::{
    connection,
    protocol::{self, Request, Update},
};
use std::{
    net::TcpStream,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Processes {
    server: Child,
    worker: Option<Child>,
    directory: PathBuf,
}
impl Drop for Processes {
    fn drop(&mut self) {
        if let Some(worker) = &mut self.worker {
            let _ = worker.kill();
            let _ = worker.wait();
        }
        let _ = self.server.kill();
        let _ = self.server.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn windows_server_sessions_and_lifecycle() -> Result<()> {
    let directory = std::env::temp_dir().join(format!(
        "scytty-windows-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    std::fs::create_dir(&directory)?;
    let server = Command::new(env!("CARGO_BIN_EXE_scytty-server"))
        .env("LOCALAPPDATA", &directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()?;
    let mut processes = Processes {
        server,
        worker: None,
        directory: directory.clone(),
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    while !directory.join("scytty/server.endpoint").exists() {
        if let Some(status) = processes.server.try_wait()? {
            bail!("server exited: {status}");
        }
        if Instant::now() > deadline {
            bail!("Windows server startup timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // A subprocess isolates environment variables; never mutate the parallel test runner's env.
    processes.worker = Some(
        Command::new(std::env::current_exe()?)
            .args(["--exact", "isolated_worker", "--nocapture"])
            .env("SCYTTY_WINDOWS_WORKER", "1")
            .env("LOCALAPPDATA", &directory)
            .spawn()?,
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = processes.worker.as_mut().unwrap().try_wait()? {
            assert!(status.success(), "Windows worker failed: {status}");
            break;
        }
        if Instant::now() > deadline {
            bail!("Windows session test timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn state(stream: &mut TcpStream) -> Result<(Vec<u64>, u64)> {
    for _ in 0..1000 {
        match protocol::receive(stream)? {
            Update::State { tabs, active } => {
                return Ok((tabs.into_iter().map(|tab| tab.id).collect(), active));
            }
            Update::Error(error) => bail!(error),
            _ => {}
        }
    }
    bail!("no state update")
}

fn attach() -> Result<TcpStream> {
    let mut stream = connection::connect()?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    protocol::handshake(&mut stream)?;
    protocol::send(
        &mut stream,
        &Request::Attach {
            session: "windows-test".into(),
            bin: None,
        },
    )?;
    Ok(stream)
}

#[test]
fn isolated_worker() -> Result<()> {
    if std::env::var_os("SCYTTY_WINDOWS_WORKER").is_none() {
        return Ok(());
    }
    let (pid, digest) = connection::daemon_status()?.context("missing Windows daemon")?;
    assert!(connection::restart_daemon(pid, [0; 32]).is_err());
    assert_eq!(connection::daemon_status()?.unwrap().0, pid);
    let mut stream = attach()?;
    let (tabs, first) = state(&mut stream)?;
    assert_eq!(tabs, [first]);
    protocol::send(
        &mut stream,
        &Request::Input {
            id: first,
            bytes: b"title %SCYTTY_SESSION%\r".to_vec(),
        },
    )?;
    let mut title_seen = false;
    for _ in 0..1000 {
        if let Update::State { tabs, .. } = protocol::receive(&mut stream)?
            && tabs
                .iter()
                .any(|tab| tab.id == first && tab.title == "windows-test")
        {
            title_seen = true;
            break;
        }
    }
    assert!(
        title_seen,
        "ConPTY shell did not report the session environment"
    );
    protocol::send(
        &mut stream,
        &Request::Resize {
            id: first,
            cols: 100,
            rows: 30,
            width: 8,
            height: 16,
        },
    )?;
    let mut resized = false;
    for _ in 0..1000 {
        if let Update::Resized { id, .. } = protocol::receive(&mut stream)?
            && id == first
        {
            resized = true;
            break;
        }
    }
    assert!(resized);
    protocol::send(&mut stream, &Request::NewTab)?;
    let (tabs, second) = state(&mut stream)?;
    assert_eq!(tabs.len(), 2);
    assert_ne!(first, second);
    protocol::send(&mut stream, &Request::CloseTab { id: second })?;
    let (tabs, active) = state(&mut stream)?;
    assert_eq!(tabs, [first]);
    assert_eq!(active, first);
    drop(stream);
    let mut stream = attach()?;
    let (tabs, _) = state(&mut stream)?;
    assert_eq!(tabs, [first]);
    protocol::send(&mut stream, &Request::CloseTab { id: first })?;
    let _ = state(&mut stream)?;
    drop(stream);
    connection::restart_daemon(pid, digest)?;
    assert!(connection::daemon_status()?.is_none());
    Ok(())
}
