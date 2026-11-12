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
#[path = "../tests/unit/stream.rs"]
mod tests;
