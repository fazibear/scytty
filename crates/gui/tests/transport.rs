// Compile the production modules together so tests can exercise private bridge state.
#[allow(dead_code)]
#[path = "../src/platform/mod.rs"]
mod platform;
#[path = "../src/reconnect.rs"]
mod reconnect;
#[allow(dead_code)]
#[path = "../src/remote.rs"]
mod remote;

#[allow(dead_code)]
mod transport {
    include!("../src/transport.rs");

    #[test]
    fn ssh_connection_failure_is_returned_from_worker() {
        let result =
            thread::spawn(|| connect_session(Some("-invalid-host".into()), "test".into(), None))
                .join()
                .expect("connection worker panicked");
        let error = result.err().expect("invalid SSH host must fail");
        assert!(error.to_string().contains("invalid SSH destination"));
    }

    #[test]
    fn dropping_connection_closes_outstanding_stream_clones() -> Result<()> {
        use std::io::Read;
        #[cfg(unix)]
        let command = Command::new("/bin/cat");
        #[cfg(windows)]
        let command = {
            let mut command = Command::new("cmd.exe");
            command.args(["/D", "/C", "more"]);
            command
        };
        let connection = bridge(command)?;
        let mut reader = connection.socket.try_clone()?;
        reader.set_read_timeout(Some(Duration::from_secs(2)))?;
        drop(connection);
        let mut byte = [0];
        match reader.read(&mut byte) {
            Ok(0) => {}
            Err(error) => assert!(matches!(
                error.kind(),
                io::ErrorKind::NotConnected
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::BrokenPipe
            )),
            other => panic!("connection still readable after drop: {other:?}"),
        }
        Ok(())
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
