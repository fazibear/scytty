use super::*;

fn parse_args(args: &[&str]) -> Result<Option<Options>> {
    parse(args.iter().map(|arg| (*arg).to_owned()))
}

#[test]
fn local_and_remote_connect_options() -> Result<()> {
    let default = Options {
        host: None,
        session: "default".into(),
        bin: None,
    };
    assert_eq!(parse_args(&[])?, Some(default));
    assert_eq!(parse_args(&["connect"])?, parse_args(&[])?);
    assert_eq!(
        parse_args(&[
            "connect",
            "--host",
            "user@host",
            "--bin",
            "/usr/bin/fish",
            "--session",
            "work"
        ])?,
        Some(Options {
            host: Some("user@host".into()),
            session: "work".into(),
            bin: Some("/usr/bin/fish".into()),
        })
    );
    assert_eq!(
        parse_args(&[
            "connect",
            "--bin",
            r"C:\Program Files\shell.exe",
            "--session",
            "local work"
        ])?,
        Some(Options {
            host: None,
            session: "local work".into(),
            bin: Some(r"C:\Program Files\shell.exe".into()),
        })
    );
    assert_eq!(
        parse_args(&["connect", "--host", "my-ssh-alias"])?
            .unwrap()
            .session,
        "default"
    );
    for args in [
        &["--help"][..],
        &["-h"],
        &["connect", "--help"],
        &["connect", "-h"],
    ] {
        assert_eq!(parse_args(args)?, None);
    }
    Ok(())
}

#[test]
fn rejects_invalid_unknown_and_legacy_arguments() {
    for args in [
        vec!["work"],
        vec!["--ssh", "user@host"],
        vec!["--host", "user@host"],
        vec!["connect", "work"],
        vec!["connect", "--unknown"],
        vec!["connect", "--host"],
        vec!["connect", "--bin"],
        vec!["connect", "--session"],
        vec!["connect", "--host", "--bin", "/bin/sh"],
        vec!["connect", "--host", "host", "--host", "other"],
        vec!["connect", "--session", "a", "--session", "b"],
        vec!["connect", "--bin", "sh", "--bin", "fish"],
        vec!["connect", "--host", "-invalid"],
        vec!["connect", "--host", "bad host"],
        vec!["connect", "--host", ""],
        vec!["connect", "--bin", ""],
        vec!["connect", "--bin", "bad\nprogram"],
        vec!["connect", "--session", ""],
        vec!["connect", "--session", "bad\nsession"],
        vec!["--help", "unexpected"],
    ] {
        assert!(parse_args(&args).is_err(), "{args:?}");
    }
    assert!(parse_args(&["connect", "--session", &"x".repeat(129)]).is_err());
}
