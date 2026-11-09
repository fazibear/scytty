use anyhow::{Context, Result};
use portable_pty::{Child, CommandBuilder, MasterPty};
use std::{
    fs::File,
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::PathBuf,
};

#[path = "../cwd.rs"]
mod cwd;
#[path = "../daemon.rs"]
pub mod daemon;

pub fn shell() -> CommandBuilder {
    let mut command =
        CommandBuilder::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()));
    command.arg("-l");
    command
}

pub fn tab_cwd(master: &dyn MasterPty, child: &dyn Child) -> Option<PathBuf> {
    let directory = |pid| cwd::process_cwd(pid).filter(|path| path.is_absolute() && path.is_dir());
    master
        .process_group_leader()
        .and_then(|pid| directory(pid as u32))
        .or_else(|| child.process_id().and_then(directory))
}

pub fn listen() -> Result<(File, UnixListener)> {
    let path = scytty_common::connection::socket_path()?;
    // Keep the lock alive so competing auto-starts cannot unlink a live socket.
    let lock = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("lock"))?;
    lock.try_lock().context("server already running")?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok((lock, listener))
}

pub fn accept(
    _stream: &mut scytty_common::stream::Stream,
) -> Result<scytty_common::connection::Accepted> {
    Ok(scytty_common::connection::Accepted::Session)
}
