use anyhow::{Context, Result, bail};
use std::{
    io,
    process::{Child, Command, Stdio},
    thread,
};

use scytty_common::stream::{self, Stream};

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

pub fn connect(host: &str) -> Result<Connection> {
    bridge(crate::remote::command(host)?)
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
        let connected = crate::Gui::attach(connection, "test".into(), None)?;
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
