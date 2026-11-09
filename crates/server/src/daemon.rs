use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read},
    os::{fd::AsRawFd, unix::net::UnixStream},
    path::PathBuf,
    time::Duration,
};

fn hash(path: &std::path::Path) -> Result<String> {
    let mut digest = Sha256::new();
    let mut file =
        File::open(path).with_context(|| format!("read daemon executable {}", path.display()))?;
    let mut buffer = [0; 16384];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(target_os = "linux")]
fn peer(socket: &UnixStream) -> Result<(libc::pid_t, PathBuf)> {
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of_val(&credentials) as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error().into());
    }
    if credentials.uid != unsafe { libc::geteuid() } || credentials.pid <= 1 {
        bail!("daemon is not owned by this user");
    }
    let executable = PathBuf::from(format!("/proc/{}/exe", credentials.pid));
    let name = std::fs::read_link(&executable)?;
    if !matches!(
        name.file_name().and_then(|name| name.to_str()),
        Some("scytty-server" | "scytty-server (deleted)")
    ) {
        bail!("socket peer is not scytty-server");
    }
    Ok((credentials.pid, executable))
}

#[cfg(target_os = "macos")]
fn peer(socket: &UnixStream) -> Result<(libc::pid_t, PathBuf)> {
    use std::os::unix::ffi::OsStringExt;
    let (mut uid, mut gid) = (0, 0);
    if unsafe { libc::getpeereid(socket.as_raw_fd(), &mut uid, &mut gid) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    if uid != unsafe { libc::geteuid() } {
        bail!("daemon is not owned by this user");
    }
    let mut pid: libc::pid_t = 0;
    let mut length = std::mem::size_of_val(&pid) as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            socket.as_raw_fd(),
            0,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error().into());
    }
    if pid <= 1 {
        bail!("invalid daemon PID");
    }
    let mut bytes = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    if unsafe { libc::proc_pidpath(pid, bytes.as_mut_ptr().cast(), bytes.len() as u32) } <= 0 {
        return Err(io::Error::last_os_error().into());
    }
    bytes.truncate(
        bytes
            .iter()
            .position(|byte| *byte == 0)
            .context("invalid daemon executable path")?,
    );
    let executable = PathBuf::from(std::ffi::OsString::from_vec(bytes));
    if executable.file_name().and_then(|name| name.to_str()) != Some("scytty-server") {
        bail!("socket peer is not scytty-server");
    }
    Ok((pid, executable))
}

fn running() -> Result<Option<(libc::pid_t, String)>> {
    let socket = match UnixStream::connect(scytty_common::connection::socket_path()?) {
        Ok(socket) => socket,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    let (pid, executable) = peer(&socket)?;
    Ok(Some((pid, hash(&executable)?)))
}

pub fn identity() -> Result<()> {
    match running()? {
        None => println!("stopped"),
        Some((pid, digest)) => println!("old {pid} {digest}"),
    }
    Ok(())
}

pub fn check() -> Result<()> {
    match running()? {
        None => println!("stopped"),
        Some((_, digest)) if digest == hash(&std::env::current_exe()?)? => println!("same"),
        Some((pid, digest)) => println!("old {pid} {digest}"),
    }
    Ok(())
}

/// Recheck the exact daemon the user approved; never kill a replacement daemon.
pub fn restart(pid: libc::pid_t, digest: &str) -> Result<()> {
    if pid <= 1 || digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("restart requires a valid daemon PID and SHA-256 digest");
    }
    let Some(current) = running()? else {
        return Ok(());
    };
    // macOS may report the replacement file's hash while an older image is running.
    if current.1 == hash(&std::env::current_exe()?)? && current.1 != digest {
        return Ok(());
    }
    if current != (pid, digest.to_owned()) {
        bail!("daemon changed since confirmation; reconnect and confirm again");
    }
    if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    let path = scytty_common::connection::socket_path()?;
    for _ in 0..100 {
        match UnixStream::connect(&path) {
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) =>
            {
                return Ok(());
            }
            Err(error) => return Err(error.into()),
            Ok(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    bail!("daemon socket is still active; reconnect before retrying")
}
