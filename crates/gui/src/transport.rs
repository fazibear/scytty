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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ssh_connection_failure_is_returned_from_worker() {
        let result =
            thread::spawn(|| connect_session(Some("-invalid-host".into()), "test".into(), None))
                .join()
                .expect("connection worker panicked");
        let error = result.err().expect("invalid SSH host must fail");
        assert!(error.to_string().contains("invalid SSH destination"));
    }

    #[cfg(unix)]
    #[test]
    fn stdio_bridge_roundtrip() -> Result<()> {
        use std::io::{Read, Write};
        let mut connection = bridge(Command::new("/bin/cat"))?;
        connection
            .socket
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
        let message = b"\x00terminal\x1b[31m\xff";
        connection.socket.write_all(message)?;
        let mut response = vec![0; message.len()];
        connection.socket.read_exact(&mut response)?;
        assert_eq!(response, message);
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn rejected_attach_has_no_pipelined_configuration() -> Result<()> {
        use scytty_common::protocol::{self, Request, Update};
        use std::{io::Read, time::Duration};
        let (socket, mut server) = stream::pair()?;
        let connection = Connection {
            socket,
            child: Command::new("/bin/sleep").arg("30").spawn()?,
        };
        let peer = thread::spawn(move || -> Result<()> {
            protocol::handshake(&mut server)?;
            assert!(matches!(
                protocol::receive::<Request>(&mut server)?,
                Request::Attach { .. }
            ));
            protocol::send(&mut server, &Update::Error("attach rejected".into()))?;
            server.set_read_timeout(Some(Duration::from_millis(100)))?;
            let error = server
                .read(&mut [0])
                .expect_err("configuration was sent before attach succeeded");
            assert!(matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ));
            Ok(())
        });
        let connected = attach(connection, "test".into(), None)?;
        assert!(
            matches!(connected.updates.recv_timeout(Duration::from_secs(5))??, Update::Error(message) if message == "attach rejected")
        );
        // The UI receives a wakeup without waiting for a polling timer.
        connected.wakeups.recv_blocking()?;
        peer.join().expect("protocol peer panicked")?;
        Ok(())
    }

    #[test]
    fn ssh_destinations() {
        for host in ["user@host", "my-server", "user@::1"] {
            assert!(validate_host(host).is_ok());
        }
        for host in ["", "-oProxyCommand=bad", "host\ncommand", "host name"] {
            assert!(validate_host(host).is_err());
        }
    }
}
