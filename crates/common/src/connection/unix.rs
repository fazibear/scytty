use anyhow::{Context, Result, bail};
use std::{
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        net::UnixStream,
    },
    path::PathBuf,
    thread,
    time::Duration,
};

pub fn socket_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    let dir = PathBuf::from(&home).join(".scytty");
    match std::fs::create_dir(&dir) {
        Ok(()) => std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let meta = std::fs::symlink_metadata(&dir)?;
    let owner = std::fs::metadata(home)?.uid();
    if !meta.is_dir() || meta.uid() != owner || meta.mode() & 0o077 != 0 {
        bail!("~/.scytty must be an owned private directory (chmod 700)");
    }
    Ok(dir.join("server.sock"))
}

pub fn server_executable() -> Result<PathBuf> {
    let sibling = std::env::current_exe()?.with_file_name("scytty-server");
    Ok(if sibling.is_file() {
        sibling
    } else {
        PathBuf::from("scytty-server")
    })
}

pub fn connect() -> Result<UnixStream> {
    let path = socket_path()?;
    if let Ok(stream) = UnixStream::connect(&path) {
        return Ok(stream);
    }
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.with_extension("log"))?;
    let executable = server_executable()?;
    let mut child = std::process::Command::new(&executable)
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()
        .with_context(|| {
            format!(
                "cannot launch {}; build/install scytty-server beside scytty or on PATH",
                executable.display()
            )
        })?;
    for _ in 0..100 {
        if let Ok(stream) = UnixStream::connect(&path) {
            thread::spawn(move || {
                let _ = child.wait();
            });
            return Ok(stream);
        }
        thread::sleep(Duration::from_millis(50));
    }
    thread::spawn(move || {
        let _ = child.wait();
    });
    bail!(
        "server did not start; see {}",
        path.with_extension("log").display()
    )
}
