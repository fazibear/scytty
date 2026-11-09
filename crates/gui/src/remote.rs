use anyhow::{Context, Result, bail};
use std::{
    ffi::OsStr,
    io::Write,
    process::{Command, Stdio},
};

pub(crate) struct Server {
    pub platform: &'static str,
    pub digest: &'static str,
    pub bytes: &'static [u8],
}
include!(concat!(env!("OUT_DIR"), "/servers.rs"));

#[derive(Clone, Debug)]
pub(crate) struct RestartRequired {
    host: String,
    path: String,
    pid: u32,
    digest: String,
}
impl std::fmt::Display for RestartRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "A different Scytty daemon is running on {} (PID {}). Restarting with the bundled server will end ALL remote sessions and shells. Close this window to leave it running.",
            self.host, self.pid
        )
    }
}
impl std::error::Error for RestartRequired {}
impl RestartRequired {
    pub fn restart(&self) -> Result<()> {
        let script = format!(
            "exec \"{}\" --restart-daemon {} {}",
            self.path, self.pid, self.digest
        );
        let result = ssh(OsStr::new("ssh"), &self.host, &script)
            .stdin(Stdio::null())
            .output()
            .context("could not restart remote daemon")?;
        if !result.status.success() {
            bail!(
                "remote daemon restart failed: {}",
                String::from_utf8_lossy(&result.stderr).trim()
            );
        }
        Ok(())
    }
}

const DETECT: &str = "uname -s && uname -m";

fn platform(output: &[u8]) -> Result<&'static str> {
    let text = std::str::from_utf8(output).context("invalid remote platform response")?;
    let lines: Vec<_> = text.lines().map(str::trim).collect();
    match lines.as_slice() {
        ["Linux", "x86_64" | "amd64"] => Ok("linux-x86_64"),
        ["Linux", "aarch64" | "arm64"] => Ok("linux-arm64"),
        ["Darwin", "arm64" | "aarch64"] => Ok("macos-arm64"),
        ["Darwin", "x86_64"] => Ok("macos-x86_64"),
        _ => bail!("unsupported remote platform: {text:?}; supported: Linux/macOS on x86_64/ARM64"),
    }
}

fn ssh(program: &OsStr, host: &str, script: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        // ssh.exe is a console program; without this flag each child of the
        // windows-subsystem GUI allocates its own visible console window.
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let script = format!("sh -c '{}'", script.replace('\'', "'\\''"));
    command.args([
        "-T",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
        host,
        &script,
    ]);
    command
}

pub(crate) fn command(host: &str) -> Result<Command> {
    prepare(host, OsStr::new("ssh"), SERVERS)
}

fn prepare(host: &str, program: &OsStr, servers: &[Server]) -> Result<Command> {
    super::transport::validate_host(host)?;
    let detected = ssh(program, host, DETECT)
        .stdin(Stdio::null())
        .output()
        .context("could not run SSH platform detection")?;
    if !detected.status.success() {
        bail!(
            "SSH platform detection failed: {}",
            String::from_utf8_lossy(&detected.stderr).trim()
        );
    }
    let platform = platform(&detected.stdout)?;
    let server = servers.iter().find(|server| server.platform == platform)
        .with_context(|| format!("this build does not embed a {platform} server; use an official release or build with SCYTTY_SERVER_BUNDLE (see README)"))?;
    // Only build-generated platform names and SHA-256 hex enter the remote shell.
    let directory = format!(
        "$HOME/.scytty/servers/{}-{}",
        server.platform, server.digest
    );
    let path = format!("{directory}/scytty-server");
    let check = ssh(program, host, &format!("test -x \"{path}\""))
        .stdin(Stdio::null())
        .output()
        .context("could not check remote server cache")?;
    if !check.status.success() {
        if check.status.code() != Some(1) {
            bail!(
                "SSH server cache check failed: {}",
                String::from_utf8_lossy(&check.stderr).trim()
            );
        }
        let script = format!(
            r#"set -eu
umask 077
for dir in "$HOME/.scytty" "$HOME/.scytty/servers" "{directory}"; do
    if [ ! -e "$dir" ]; then mkdir -p "$dir"; fi
    [ -d "$dir" ] && [ ! -L "$dir" ] && [ -O "$dir" ] || {{ echo 'unsafe Scytty cache directory' >&2; exit 1; }}
    chmod 700 "$dir"
done
tmp=$(mktemp "{directory}/.upload.XXXXXXXX")
trap 'rm -f "$tmp"' EXIT HUP INT TERM
cat > "$tmp"
chmod 700 "$tmp"
mv -f "$tmp" "{path}"
"#
        );
        let mut child = ssh(program, host, &script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("could not start remote server upload")?;
        let written = child.stdin.take().unwrap().write_all(server.bytes);
        let result = child
            .wait_with_output()
            .context("could not finish remote server upload")?;
        if !result.status.success() {
            bail!(
                "remote server installation failed: {}",
                String::from_utf8_lossy(&result.stderr).trim()
            );
        }
        written.context("could not upload bundled server")?;
    }
    let status = ssh(program, host, &format!("exec \"{path}\" --check-daemon"))
        .stdin(Stdio::null())
        .output()
        .context("could not inspect remote daemon")?;
    if !status.status.success() {
        bail!(
            "remote daemon check failed: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        );
    }
    let text = std::str::from_utf8(&status.stdout).context("invalid remote daemon status")?;
    match text.trim() {
        "same" | "stopped" => {}
        other => {
            let fields: Vec<_> = other.split_whitespace().collect();
            if let ["old", pid, digest] = fields.as_slice()
                && let Ok(pid) = pid.parse::<u32>()
                && pid > 1
                && pid <= i32::MAX as u32
                && digest.len() == 64
                && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(RestartRequired {
                    host: host.into(),
                    path,
                    pid,
                    digest: (*digest).into(),
                }
                .into());
            }
            bail!("invalid remote daemon status: {text:?}");
        }
    }
    Ok(ssh(program, host, &format!("exec \"{path}\" --stdio")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_platforms() {
        for (response, expected) in [
            ("Linux\nx86_64\n", "linux-x86_64"),
            ("Linux\naarch64\n", "linux-arm64"),
            ("Darwin\narm64\n", "macos-arm64"),
            ("Darwin\nx86_64\n", "macos-x86_64"),
        ] {
            assert_eq!(platform(response.as_bytes()).unwrap(), expected);
        }
        for response in [
            "FreeBSD\nx86_64",
            "Linux\narmv7l",
            "banner\nLinux\nx86_64",
            "Linux",
        ] {
            assert!(platform(response.as_bytes()).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn deploy_and_reuse_without_touching_daemon() -> Result<()> {
        use std::{fs, os::unix::fs::PermissionsExt};
        struct Temp(std::path::PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let root = Temp(std::env::temp_dir().join(format!(
                "scytty-deploy ' {}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_nanos()
            )));
        fs::create_dir(&root.0)?;
        let home = root.0.to_str().unwrap().replace('\'', "'\\''");
        let ssh = root.0.join("ssh");
        fs::write(
            &ssh,
            format!(
                "#!/bin/sh\nexport HOME='{home}'\nfor arg; do :; done\nprintf '%s\\n' \"$arg\" >> \"$HOME/commands\"\ncase \"$arg\" in *'uname -s && uname -m'*) printf 'Linux\\nx86_64\\n';; *) exec /bin/sh -c \"$arg\";; esac\n"
            ),
        )?;
        fs::set_permissions(&ssh, fs::Permissions::from_mode(0o700))?;
        let servers = [Server { platform: "linux-x86_64", digest: "test", bytes: b"#!/bin/sh\nif [ \"$1\" = --check-daemon ]; then if [ -f \"$HOME/status\" ]; then cat \"$HOME/status\"; else echo stopped; fi; exit; fi\n[ \"$1\" = --stdio ] || exit 2\nexec cat\n" }];
        fs::create_dir(root.0.join(".scytty"))?;
        fs::write(root.0.join(".scytty/server.sock"), "existing daemon")?;
        for _ in 0..2 {
            let command = prepare("host", ssh.as_os_str(), &servers)?;
            let mut connection = crate::transport::bridge(command)?;
            connection
                .socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
            connection.socket.write_all(b"terminal bytes")?;
            let mut bytes = [0; 14];
            std::io::Read::read_exact(&mut connection.socket, &mut bytes)?;
            assert_eq!(&bytes, b"terminal bytes");
        }
        let installed = root
            .0
            .join(".scytty/servers/linux-x86_64-test/scytty-server");
        assert_eq!(fs::read(installed)?, servers[0].bytes);
        assert_eq!(
            fs::read_to_string(root.0.join(".scytty/server.sock"))?,
            "existing daemon"
        );
        assert_eq!(
            fs::read_to_string(root.0.join("commands"))?
                .matches("tmp=$(mktemp")
                .count(),
            1
        );
        fs::write(
            root.0.join("status"),
            format!("old 123 {}\n", "a".repeat(64)),
        )?;
        let error = prepare("host", ssh.as_os_str(), &servers).unwrap_err();
        assert!(error.downcast_ref::<RestartRequired>().is_some());
        assert!(error.to_string().contains("ALL remote sessions"));
        assert!(!fs::read_to_string(root.0.join("commands"))?.contains("--restart-daemon"));
        fs::write(root.0.join("status"), "old 123 not-a-digest\n")?;
        assert!(
            prepare("host", ssh.as_os_str(), &servers)
                .unwrap_err()
                .to_string()
                .contains("invalid remote daemon status")
        );
        fs::write(root.0.join("status"), "same\n")?;
        assert!(prepare("host", ssh.as_os_str(), &servers).is_ok());
        assert!(
            prepare("host", ssh.as_os_str(), &[])
                .unwrap_err()
                .to_string()
                .contains("does not embed")
        );
        // Refuse cache-directory symlinks instead of writing through them.
        fs::remove_dir_all(root.0.join(".scytty/servers"))?;
        let elsewhere = root.0.join("elsewhere");
        fs::create_dir(&elsewhere)?;
        std::os::unix::fs::symlink(&elsewhere, root.0.join(".scytty/servers"))?;
        assert!(
            prepare("host", ssh.as_os_str(), &servers)
                .unwrap_err()
                .to_string()
                .contains("unsafe Scytty cache directory")
        );
        assert_eq!(fs::read_dir(elsewhere)?.count(), 0);
        Ok(())
    }
}
