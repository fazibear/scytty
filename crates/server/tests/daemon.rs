#![cfg(unix)]

use anyhow::{Result, bail};
use std::{
    fs,
    os::unix::process::{CommandExt, ExitStatusExt},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Daemon {
    child: Option<Child>,
    home: std::path::PathBuf,
}
impl Drop for Daemon {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.home);
    }
}
fn command(home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_scytty-server"));
    command.env("HOME", home);
    command
}
fn status(home: &Path) -> Result<String> {
    let result = command(home).arg("--check-daemon").output()?;
    if !result.status.success() {
        bail!("check failed: {}", String::from_utf8_lossy(&result.stderr));
    }
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}
fn wait(home: &Path) -> Result<()> {
    for _ in 0..100 {
        if home.join(".scytty/server.sock").exists() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    bail!("test daemon did not start")
}

#[test]
fn daemon_upgrade_requires_confirmation() -> Result<()> {
    let home = Path::new("/tmp").join(format!(
        "scytty-upgrade-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir(&home)?;
    let mut daemon = Daemon { child: None, home };
    assert_eq!(status(&daemon.home)?, "stopped");
    let old = daemon.home.join("old");
    fs::create_dir(&old)?;
    let old = old.join("scytty-server");
    fs::copy(env!("CARGO_BIN_EXE_scytty-server"), &old)?;
    // Same behavior, distinct executable identity, without another Cargo build.
    #[cfg(target_os = "macos")]
    assert!(
        Command::new("codesign")
            .args([
                "--force",
                "--sign",
                "-",
                "--identifier",
                "app.scytty.test-old-server"
            ])
            .arg(&old)
            .status()?
            .success()
    );
    #[cfg(target_os = "linux")]
    {
        use std::io::Write;
        fs::OpenOptions::new()
            .append(true)
            .open(&old)?
            .write_all(b"old-test-build")?;
    }
    let mut old_command = Command::new(old);
    // Reproduce a launcher passing an ignored SIGTERM through exec.
    unsafe {
        old_command.pre_exec(|| {
            libc::signal(libc::SIGTERM, libc::SIG_IGN);
            Ok(())
        });
    }
    daemon.child = Some(
        old_command
            .arg("server")
            .env("HOME", &daemon.home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    wait(&daemon.home)?;
    // A different executable with the same protocol must preserve running sessions.
    assert_eq!(status(&daemon.home)?, "same");
    let identity = command(&daemon.home).arg("--daemon-identity").output()?;
    assert!(identity.status.success());
    let old_status = String::from_utf8(identity.stdout)?;
    let fields: Vec<_> = old_status.split_whitespace().collect();
    assert_eq!(fields[0], "old");
    assert_eq!(
        fields[1].parse::<u32>()?,
        daemon.child.as_ref().unwrap().id()
    );
    assert_eq!(fields[2].len(), 64);
    // Checking never terminates the existing daemon.
    assert!(daemon.child.as_mut().unwrap().try_wait()?.is_none());
    let rejected = command(&daemon.home)
        .args(["--restart-daemon", fields[1], &"0".repeat(64)])
        .output()?;
    assert!(!rejected.status.success());
    assert!(daemon.child.as_mut().unwrap().try_wait()?.is_none());
    let approved = command(&daemon.home)
        .args(["--restart-daemon", fields[1], fields[2]])
        .output()?;
    assert!(
        approved.status.success(),
        "{}",
        String::from_utf8_lossy(&approved.stderr)
    );
    assert_eq!(
        daemon.child.as_mut().unwrap().wait()?.signal(),
        Some(libc::SIGKILL)
    );
    daemon.child = None;
    assert_eq!(status(&daemon.home)?, "stopped");
    daemon.child = Some(
        command(&daemon.home)
            .arg("server")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    wait(&daemon.home)?;
    // A stale socket can exist briefly before the replacement binds.
    for _ in 0..100 {
        if status(&daemon.home)? == "same" {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(status(&daemon.home)?, "same");
    assert!(
        command(&daemon.home)
            .args([
                "--restart-daemon",
                &daemon.child.as_ref().unwrap().id().to_string(),
                &"0".repeat(64)
            ])
            .status()?
            .success()
    );
    assert!(daemon.child.as_mut().unwrap().try_wait()?.is_none());
    let identity = command(&daemon.home).arg("--daemon-identity").output()?;
    assert!(identity.status.success());
    let text = String::from_utf8(identity.stdout)?;
    let fields: Vec<_> = text.split_whitespace().collect();
    assert_eq!(fields[0], "old");
    assert_eq!(fields[1], daemon.child.as_ref().unwrap().id().to_string());
    assert!(
        command(&daemon.home)
            .args(["--restart-daemon", fields[1], fields[2]])
            .status()?
            .success()
    );
    daemon.child.as_mut().unwrap().wait()?;
    daemon.child = None;
    assert_eq!(status(&daemon.home)?, "stopped");
    Ok(())
}
