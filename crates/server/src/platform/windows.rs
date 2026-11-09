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

    pub fn check() -> Result<()> {
        match connection::daemon_status()? {
            None => println!("stopped"),
            Some((_, digest)) if digest == connection::executable_digest()? => println!("same"),
            Some((pid, digest)) => println!("old {pid} {}", hex(&digest)),
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
        if running_digest == connection::executable_digest()? {
            return Ok(());
        }
        if running_pid != pid || !hex(&running_digest).eq_ignore_ascii_case(digest) {
            bail!("daemon changed since confirmation; reconnect and confirm again");
        }
        connection::restart_daemon(running_pid, running_digest).context("restart Windows daemon")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_shell_uses_comspec_without_unix_login_flags() {
        let shell = shell();
        assert_eq!(shell.get_argv().len(), 1);
    }
}
