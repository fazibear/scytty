use super::*;

#[cfg(unix)]
#[test]
fn single_connection_bootstrap() -> Result<()> {
    use std::{fs, io::Read, os::unix::fs::PermissionsExt};
    let root = std::env::temp_dir().join(format!(
        "scytty-bootstrap-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    fs::create_dir(&root)?;
    let ssh = root.join("ssh");
    let home = root.to_str().unwrap().replace('\'', "'\\''");
    fs::write(
        &ssh,
        format!(
            "#!/bin/sh\nexport HOME='{home}'\necho connection >> \"$HOME/connections\"\nfor arg; do :; done\nexec /bin/sh -c \"$arg\"\n"
        ),
    )?;
    fs::set_permissions(&ssh, fs::Permissions::from_mode(0o700))?;
    let bytes = b"#!/bin/sh\ncase \"$1\" in\n--check-daemon) echo same;;\n--daemon-identity) echo old 123 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa;;\n*) exec cat;;\nesac\n";
    let servers: Vec<_> = ["linux-x86_64", "linux-arm64", "macos-x86_64", "macos-arm64"]
        .into_iter()
        .map(|platform| Server {
            platform,
            digest: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            bytes,
        })
        .collect();
    for bundle in [&servers[..], &[][..]] {
        let mut connection = bootstrap("host", ssh.as_os_str(), bundle)?;
        connection
            .socket
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
        connection.socket.write_all(b"\x00terminal\xff\n")?;
        let mut response = [0; 11];
        connection.socket.read_exact(&mut response)?;
        assert_eq!(&response, b"\x00terminal\xff\n");
    }
    assert_eq!(
        fs::read_to_string(root.join("connections"))?
            .lines()
            .count(),
        2
    );
    let different: Vec<_> = servers
        .iter()
        .map(|server| Server {
            platform: server.platform,
            digest: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            bytes: b"must not upload before approval",
        })
        .collect();
    let error = match bootstrap("host", ssh.as_os_str(), &different) {
        Ok(_) => panic!("different compatible build must offer a choice"),
        Err(error) => error,
    };
    let warning = error.downcast::<RestartRequired>()?;
    assert_eq!(warning.pid, 123);
    assert!(warning.existing_path.is_some());
    assert!(warning.to_string().contains("protocol-compatible"));
    assert!(warning.to_string().contains("ALL sessions"));
    for entry in fs::read_dir(root.join(".scytty/servers"))? {
        assert!(!entry?.file_name().to_string_lossy().contains("bbbbbbbb"));
    }
    fs::remove_dir_all(root)?;
    Ok(())
}

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
    let servers = [Server { platform: "linux-x86_64", digest: "test", bytes: b"#!/bin/sh\nif [ \"$1\" = --check-daemon ] || [ \"$1\" = --daemon-identity ]; then if [ -f \"$HOME/status\" ]; then cat \"$HOME/status\"; else echo stopped; fi; exit; fi\n[ \"$1\" = --stdio ] || exit 2\nexec cat\n" }];
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
    assert!(error.to_string().contains("ALL sessions"));
    let forced = prepare_inspecting("host", ssh.as_os_str(), &servers, true).unwrap_err();
    assert!(forced.downcast_ref::<RestartRequired>().is_some());
    assert!(fs::read_to_string(root.0.join("commands"))?.contains("--daemon-identity"));
    let local = check_status(
        format!("old 123 {}", "a".repeat(64)).as_bytes(),
        "this computer",
        "/server".into(),
        true,
    )
    .unwrap_err();
    assert!(local.downcast_ref::<RestartRequired>().unwrap().local);
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
    // A compatible cached build avoids uploading a different bundled build,
    // and works even if this client has no server bundle for the host.
    assert!(prepare("host", ssh.as_os_str(), &[]).is_ok());
    let different = [Server {
        platform: "linux-x86_64",
        digest: "different",
        bytes: b"must not upload",
    }];
    assert!(prepare("host", ssh.as_os_str(), &different).is_ok());
    assert!(
        !root
            .0
            .join(".scytty/servers/linux-x86_64-different")
            .exists()
    );
    assert_eq!(
        fs::read_to_string(root.0.join("commands"))?
            .matches("tmp=$(mktemp")
            .count(),
        1
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
