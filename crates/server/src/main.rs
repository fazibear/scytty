fn main() -> anyhow::Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("--stdio") => scytty_server::bridge_stdio(),
        Some("--check-daemon") => scytty_server::daemon::check(),
        Some("--restart-daemon") => {
            let mut args = std::env::args().skip(2);
            let pid = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("missing daemon PID"))?
                .parse()?;
            let digest = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("missing daemon digest"))?;
            scytty_server::daemon::restart(pid, &digest)
        }
        None | Some("server") => scytty_server::run(),
        _ => anyhow::bail!(
            "usage: scytty-server [--stdio | --check-daemon | --restart-daemon PID SHA256]"
        ),
    }
}
