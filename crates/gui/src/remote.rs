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
    local: bool,
    pub(crate) existing_path: Option<String>,
}
impl std::fmt::Display for RestartRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.existing_path.is_some() {
            return write!(
                f,
                "A protocol-compatible Scytty daemon with a different build is running on {} (PID {}). Connect to it to preserve sessions, or upload the bundled server and restart. Restarting ends ALL sessions and shells.",
                self.host, self.pid
            );
        }
        write!(
            f,
            "A different Scytty daemon is running on {} (PID {}). Restarting with the bundled server will end ALL sessions and shells. Close this window to leave it running.",
            self.host, self.pid
        )
    }
}
impl std::error::Error for RestartRequired {}
impl RestartRequired {
    pub fn restart(&self) -> Result<()> {
        if self.existing_path.is_some() {
            let error = prepare_inspecting(&self.host, OsStr::new("ssh"), SERVERS, true)
                .err()
                .context("daemon changed; reconnect before restarting")?;
            let warning = error.downcast::<RestartRequired>()?;
            if warning.pid != self.pid || warning.digest != self.digest {
                bail!("daemon changed since confirmation; reconnect and confirm again");
            }
            return warning.restart();
        }
        let script = format!(
            "exec \"{}\" --restart-daemon {} {}",
            self.path, self.pid, self.digest
        );
        let mut command = if self.local {
            let mut command = Command::new(&self.path);
            command.args(["--restart-daemon", &self.pid.to_string(), &self.digest]);
            crate::platform::hide_console(&mut command);
            command
        } else {
            ssh(OsStr::new("ssh"), &self.host, &script)
        };
        let result = command
            .stdin(Stdio::null())
            .output()
            .context("could not restart remote daemon")?;
        if !result.status.success() {
            bail!(
                "daemon restart failed: {}",
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
    crate::platform::hide_console(&mut command);
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

pub(crate) fn connect_existing(host: &str, path: &str) -> Result<super::transport::Connection> {
    super::transport::validate_host(host)?;
    let quoted = path.replace('\'', "'\\''");
    super::transport::bridge(ssh(
        OsStr::new("ssh"),
        host,
        &format!("exec '{quoted}' --stdio"),
    ))
}

pub(crate) fn connect(host: &str) -> Result<super::transport::Connection> {
    bootstrap(host, OsStr::new("ssh"), SERVERS)
}

fn bootstrap(
    host: &str,
    program: &OsStr,
    servers: &[Server],
) -> Result<super::transport::Connection> {
    use base64::Engine;
    use std::io::Read;
    super::transport::validate_host(host)?;
    let mut choices = String::new();
    for server in servers {
        choices.push_str(&format!(
            "{} ) digest={};;\n",
            server.platform, server.digest
        ));
    }
    let script = format!(
        r#"set -eu
case "$(uname -s):$(uname -m)" in
 Linux:x86_64|Linux:amd64) platform=linux-x86_64;;
 Linux:aarch64|Linux:arm64) platform=linux-arm64;;
 Darwin:arm64|Darwin:aarch64) platform=macos-arm64;;
 Darwin:x86_64) platform=macos-x86_64;;
 *) echo 'unsupported remote platform' >&2; exit 1;;
esac
digest=
case "$platform" in
{choices}
esac
for server in "$HOME"/.scytty/servers/"$platform"-*/scytty-server; do
 [ -x "$server" ] || continue
 if [ "$("$server" --check-daemon 2>/dev/null)" = same ]; then
  identity=$("$server" --daemon-identity)
  if [ -n "$digest" ] && [ "${{identity##* }}" != "$digest" ]; then
   printf 'CHOICE %s\n%s\n' "$server" "$identity"
   exit 0
  fi
  echo READY
  exec "$server" --stdio
 fi
done
case "$platform" in
{choices}
 *) echo 'this build does not embed a server for this platform' >&2; exit 1;;
esac
directory="$HOME/.scytty/servers/$platform-$digest"
server="$directory/scytty-server"
if [ ! -x "$server" ]; then
 umask 077
 for dir in "$HOME/.scytty" "$HOME/.scytty/servers" "$directory"; do
  if [ ! -e "$dir" ]; then mkdir -p "$dir"; fi
  [ -d "$dir" ] && [ ! -L "$dir" ] && [ -O "$dir" ] || {{ echo 'unsafe Scytty cache directory' >&2; exit 1; }}
  chmod 700 "$dir"
 done
 tmp=$(mktemp "$directory/.upload.XXXXXXXX")
 trap 'rm -f "$tmp" "$tmp.b64"' EXIT HUP INT TERM
 printf 'UPLOAD %s\n' "$platform"
 complete=false
 while IFS= read -r line; do
  if [ "$line" = SCYTTY_UPLOAD_END ]; then complete=true; break; fi
  printf '%s\n' "$line" >> "$tmp.b64"
 done
 [ "$complete" = true ] || exit 1
 /usr/bin/base64 -d < "$tmp.b64" > "$tmp"
 chmod 700 "$tmp"
 mv -f "$tmp" "$server"
 rm -f "$tmp.b64"
 trap - EXIT HUP INT TERM
fi
status=$("$server" --check-daemon)
case "$status" in
 same|stopped) echo READY; exec "$server" --stdio;;
 *) printf 'STATUS %s\n%s\n' "$server" "$status";;
esac
"#
    );
    let mut connection = super::transport::bridge(ssh(program, host, &script))?;
    connection
        .socket
        .set_read_timeout(Some(std::time::Duration::from_secs(60)))?;
    connection
        .socket
        .set_write_timeout(Some(std::time::Duration::from_secs(60)))?;
    fn line(stream: &mut impl Read) -> Result<String> {
        let mut bytes = Vec::new();
        loop {
            let mut byte = [0];
            stream
                .read_exact(&mut byte)
                .context("SSH bootstrap ended before attachment; check SSH output")?;
            if byte[0] == b'\n' {
                return Ok(String::from_utf8(bytes)?);
            }
            if bytes.len() >= 8192 {
                bail!("oversized SSH bootstrap response");
            }
            bytes.push(byte[0]);
        }
    }
    let mut uploaded = false;
    loop {
        let response = line(&mut connection.socket)?;
        if response == "READY" {
            connection.socket.set_read_timeout(None)?;
            connection.socket.set_write_timeout(None)?;
            return Ok(connection);
        }
        if let Some(platform) = response.strip_prefix("UPLOAD ") {
            if uploaded {
                bail!("repeated SSH upload request");
            }
            let server = servers
                .iter()
                .find(|server| server.platform == platform)
                .context("unsupported SSH upload platform")?;
            for chunk in server.bytes.chunks(48 * 1024) {
                let encoded = base64::engine::general_purpose::STANDARD.encode(chunk);
                writeln!(connection.socket, "{encoded}")?;
            }
            writeln!(connection.socket, "SCYTTY_UPLOAD_END")?;
            uploaded = true;
        } else if let Some(path) = response.strip_prefix("CHOICE ") {
            let status = line(&mut connection.socket)?;
            let error = check_status(status.as_bytes(), host, path.into(), false)
                .err()
                .context("invalid compatible daemon identity")?;
            let mut warning = error.downcast::<RestartRequired>()?;
            warning.existing_path = Some(path.into());
            return Err(warning.into());
        } else if let Some(path) = response.strip_prefix("STATUS ") {
            let status = line(&mut connection.socket)?;
            check_status(status.as_bytes(), host, path.into(), false)?;
            bail!("unexpected SSH daemon status");
        } else {
            bail!("invalid SSH bootstrap response: {response:?}");
        }
    }
}

#[cfg(test)]
fn prepare(host: &str, program: &OsStr, servers: &[Server]) -> Result<Command> {
    prepare_inspecting(host, program, servers, false)
}

pub(crate) fn restart_after_mismatch(host: Option<&str>) -> Result<()> {
    if let Some(host) = host {
        prepare_inspecting(host, OsStr::new("ssh"), SERVERS, true)?;
    } else {
        let path = scytty_common::connection::server_executable()?;
        let mut command = Command::new(&path);
        crate::platform::hide_console(&mut command);
        let status = command
            .arg("--daemon-identity")
            .stdin(Stdio::null())
            .output()?;
        if !status.status.success() {
            bail!("could not identify local daemon");
        }
        check_status(
            &status.stdout,
            "this computer",
            path.to_str().context("invalid local server path")?.into(),
            true,
        )?;
    }
    Ok(())
}

fn prepare_inspecting(
    host: &str,
    program: &OsStr,
    servers: &[Server],
    inspect: bool,
) -> Result<Command> {
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
    if !inspect {
        let probe = format!(
            r#"for server in "$HOME"/.scytty/servers/{platform}-*/scytty-server; do
    [ -x "$server" ] || continue
    if [ "$("$server" --check-daemon 2>/dev/null)" = same ]; then
        printf '%s\n' "$server"
        exit 0
    fi
done
exit 1"#
        );
        let cached = ssh(program, host, &probe)
            .stdin(Stdio::null())
            .output()
            .context("could not check compatible cached servers")?;
        if cached.status.success() {
            let path = std::str::from_utf8(&cached.stdout)
                .context("invalid cached server path")?
                .trim_end_matches('\n');
            if path.is_empty() || path.contains(['\n', '\r']) {
                bail!("invalid cached server path");
            }
            let quoted = path.replace('\'', "'\\''");
            return Ok(ssh(program, host, &format!("exec '{quoted}' --stdio")));
        }
        if cached.status.code() != Some(1) {
            bail!(
                "SSH compatible server check failed: {}",
                String::from_utf8_lossy(&cached.stderr).trim()
            );
        }
    }
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
    let operation = if inspect {
        "--daemon-identity"
    } else {
        "--check-daemon"
    };
    let status = ssh(program, host, &format!("exec \"{path}\" {operation}"))
        .stdin(Stdio::null())
        .output()
        .context("could not inspect remote daemon")?;
    if !status.status.success() {
        bail!(
            "remote daemon check failed: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        );
    }
    check_status(&status.stdout, host, path.clone(), false)?;
    Ok(ssh(program, host, &format!("exec \"{path}\" --stdio")))
}

fn check_status(status: &[u8], host: &str, path: String, local: bool) -> Result<()> {
    let text = std::str::from_utf8(status).context("invalid daemon status")?;
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
                    local,
                    existing_path: None,
                }
                .into());
            }
            bail!("invalid remote daemon status: {text:?}");
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/remote.rs"]
mod tests;
