use anyhow::{Context, Result, bail};
use std::{
    io,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use scytty_common::stream::{self, Stream};

use crate::{platform, reconnect, remote};
use scytty_common::protocol::{self, Request, Update};

pub struct Connected {
    pub connection: Connection,
    pub writer: Stream,
    pub updates: Receiver<Result<Update>>,
    pub wakeups: async_channel::Receiver<()>,
}

pub fn connect_session(
    host: Option<String>,
    session: String,
    bin: Option<String>,
) -> Result<Connected> {
    connect_session_choice(host, session, bin, None)
}

pub fn connect_session_choice(
    host: Option<String>,
    session: String,
    bin: Option<String>,
    existing_path: Option<String>,
) -> Result<Connected> {
    let connection = match host.as_deref() {
        Some(host) if existing_path.is_some() => {
            remote::connect_existing(host, existing_path.as_deref().unwrap())?
        }
        Some(host) => connect(host)?,
        None => connect_local()?,
    };
    attach(connection, session, bin).map_err(|error| {
        match remote::restart_after_mismatch(host.as_deref()) {
            Err(warning) if warning.downcast_ref::<remote::RestartRequired>().is_some() => {
                warning.context(format!("{error:#}"))
            }
            _ => error,
        }
    })
}

pub fn attach(connection: Connection, session: String, bin: Option<String>) -> Result<Connected> {
    let mut writer = connection.socket.try_clone()?;
    writer.set_write_timeout(Some(Duration::from_secs(5)))?;
    writer.set_read_timeout(Some(Duration::from_secs(5)))?;
    protocol::handshake(&mut writer).map_err(|error| {
        if reconnect::transport_error(&error) {
            error
        } else {
            error.context(reconnect::IncompatiblePeer)
        }
    })?;
    writer.set_read_timeout(None)?;
    protocol::send(&mut writer, &Request::Attach { session, bin })?;
    let mut reader = writer.try_clone()?;
    let (tx, updates) = mpsc::sync_channel(256);
    let (wake, wakeups) = async_channel::bounded(1);
    thread::spawn(move || {
        loop {
            let update = protocol::receive(&mut reader).context("Server connection closed or sent an incomplete frame; check SSH output and ~/.scytty/server.log on the server");
            let failed = update.is_err();
            if tx.send(update).is_err() {
                break;
            }
            let _ = wake.try_send(());
            if failed {
                break;
            }
        }
    });
    Ok(Connected {
        connection,
        writer,
        updates,
        wakeups,
    })
}

pub struct Connection {
    pub socket: Stream,
    child: Child,
}
impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn connect_local() -> Result<Connection> {
    let mut command = Command::new(scytty_common::connection::server_executable()?);
    command.arg("--stdio");
    platform::hide_console(&mut command);
    bridge(command)
}

pub fn connect(host: &str) -> Result<Connection> {
    crate::remote::connect(host)
}

pub(crate) fn bridge(mut command: Command) -> Result<Connection> {
    let (socket, mut bridge) = stream::pair()?;
    let mut outgoing = bridge.try_clone()?;
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("could not start terminal stdio bridge")?;
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    thread::spawn(move || {
        let _ = io::copy(&mut outgoing, &mut stdin);
        // Dropping stdin signals EOF remotely without interrupting remaining output.
    });
    thread::spawn(move || {
        let _ = io::copy(&mut stdout, &mut bridge);
        let _ = bridge.shutdown(std::net::Shutdown::Both);
    });
    Ok(Connection { socket, child })
}

pub(crate) fn validate_host(host: &str) -> Result<()> {
    if host.is_empty()
        || host.starts_with('-')
        || host.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        bail!("invalid SSH destination; use user@host or an SSH config alias");
    }
    Ok(())
}
