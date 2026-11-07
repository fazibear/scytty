#[cfg(windows)]
pub use std::net::TcpStream as Stream;
#[cfg(unix)]
pub use std::os::unix::net::UnixStream as Stream;

#[cfg(unix)]
pub fn pair() -> std::io::Result<(Stream, Stream)> {
    Stream::pair()
}

#[cfg(windows)]
pub fn pair() -> std::io::Result<(Stream, Stream)> {
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    let client = Stream::connect(listener.local_addr()?)?;
    loop {
        let (server, peer) = listener.accept()?;
        // Only accept our own connection, not another local process racing us.
        if peer == client.local_addr()? {
            client.set_nodelay(true)?;
            server.set_nodelay(true)?;
            return Ok((client, server));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bidirectional_pair() -> std::io::Result<()> {
        use std::io::{Read, Write};
        let (mut a, mut b) = super::pair()?;
        a.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
        b.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
        a.write_all(b"a")?;
        let mut byte = [0];
        b.read_exact(&mut byte)?;
        assert_eq!(byte, *b"a");
        b.write_all(b"b")?;
        a.read_exact(&mut byte)?;
        assert_eq!(byte, *b"b");
        Ok(())
    }
}
