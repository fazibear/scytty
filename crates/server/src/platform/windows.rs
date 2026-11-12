use anyhow::Result;
use portable_pty::{Child, CommandBuilder, MasterPty};
use std::path::PathBuf;

pub use scytty_common::connection::{accept, listen};

pub fn shell() -> CommandBuilder {
    CommandBuilder::new(std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into()))
}

pub fn tab_cwd(_master: &dyn MasterPty, _child: &dyn Child) -> Option<PathBuf> {
    // shortcut: ConPTY exposes no portable foreground cwd; use the daemon cwd until shell cwd reporting is added.
    None
}

pub mod daemon {
    use super::*;
    use anyhow::{Context, bail};
    use scytty_common::connection;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub fn identity() -> Result<()> {
        match connection::daemon_status()? {
            None => println!("stopped"),
            Some((pid, digest)) => println!("old {pid} {}", hex(&digest)),
        }
        Ok(())
    }

    pub fn check() -> Result<()> {
        match connection::daemon_status()? {
            None => println!("stopped"),
            Some((pid, digest)) => {
                let mut stream = connection::connect()?;
                stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
                stream.set_write_timeout(Some(std::time::Duration::from_secs(5)))?;
                if scytty_common::protocol::handshake(&mut stream).is_ok() {
                    println!("same");
                } else {
                    println!("old {pid} {}", hex(&digest));
                }
            }
        }
        Ok(())
    }

    pub fn restart(pid: u32, digest: &str) -> Result<()> {
        if pid <= 1 || digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("restart requires a valid daemon PID and SHA-256 digest");
        }
        let Some((running_pid, running_digest)) = connection::daemon_status()? else {
            return Ok(());
        };
        if running_digest == connection::executable_digest()?
            && !hex(&running_digest).eq_ignore_ascii_case(digest)
        {
            return Ok(());
        }
        if running_pid != pid || !hex(&running_digest).eq_ignore_ascii_case(digest) {
            bail!("daemon changed since confirmation; reconnect and confirm again");
        }
        connection::restart_daemon(running_pid, running_digest).context("restart Windows daemon")
    }
}

#[cfg(test)]
#[path = "../../tests/unit/platform/windows.rs"]
mod tests;
