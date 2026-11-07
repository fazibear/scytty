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
    validate_host(host)?;
    let mut command = Command::new("ssh");
    command.args([
        "-T",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
        host,
        "scytty-server --stdio",
    ]);
    bridge(command)
}

#[cfg(windows)]
pub fn connect_local() -> Result<Connection> {
    bail!("local sessions are not supported on Windows; use scytty --ssh user@host [session]")
}

#[cfg(unix)]
pub fn connect_local() -> Result<Connection> {
    let mut command = Command::new(scytty_common::connection::server_executable()?);
    command.arg("--stdio");
    bridge(command)
}

fn bridge(mut command: Command) -> Result<Connection> {
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
