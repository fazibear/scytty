fn main() -> anyhow::Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("--stdio") => scytty_server::bridge_stdio(),
        None | Some("server") => scytty_server::run(),
        _ => anyhow::bail!("usage: scytty-server [--stdio]"),
    }
}
