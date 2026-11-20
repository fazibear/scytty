use anyhow::{Context, Result, bail};

pub(super) const HELP: &str =
    "Usage: scytty [connect [--host HOST] [--session NAME] [--bin EXECUTABLE]]

Without arguments, connects to the default local session.

  --host HOST       SSH destination (user@host or SSH config alias); omit for local
  --session NAME    Session to create or attach (default: default)
  --bin EXECUTABLE  Program for a new session; not a shell command or argument list
  -h, --help        Show this help";

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Options {
    pub host: Option<String>,
    pub session: String,
    pub bin: Option<String>,
}

pub(super) fn parse(mut args: impl Iterator<Item = String>) -> Result<Option<Options>> {
    match args.next().as_deref() {
        None => {
            return Ok(Some(Options {
                host: None,
                session: "default".into(),
                bin: None,
            }));
        }
        Some("-h" | "--help") => {
            if args.next().is_some() {
                bail!("unexpected argument after help\n{HELP}")
            }
            return Ok(None);
        }
        Some("connect") => {}
        Some(command) => bail!("unknown command: {command}\n{HELP}"),
    }
    let (mut host, mut session, mut bin) = (None, None, None);
    while let Some(flag) = args.next() {
        let slot = match flag.as_str() {
            "--host" => &mut host,
            "--session" => &mut session,
            "--bin" => &mut bin,
            "-h" | "--help" => return Ok(None),
            _ => bail!("unknown connect argument: {flag}\n{HELP}"),
        };
        if slot.is_some() {
            bail!("duplicate option: {flag}")
        }
        *slot = Some(
            args.next()
                .filter(|value| !value.starts_with("--"))
                .with_context(|| format!("{flag} requires a value\n{HELP}"))?,
        );
    }
    if let Some(host) = &host {
        crate::transport::validate_host(host)?;
    }
    if let Some(bin) = &bin {
        scytty_common::protocol::validate_bin(bin)?;
    }
    let session = session.unwrap_or_else(|| "default".into());
    if session.is_empty() || session.len() > 128 || session.chars().any(char::is_control) {
        bail!("session must be nonempty, at most 128 bytes, without control characters");
    }
    Ok(Some(Options { host, session, bin }))
}

#[cfg(test)]
#[path = "../tests/unit/cli.rs"]
mod tests;
