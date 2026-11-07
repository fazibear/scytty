use anyhow::{Context, Result, bail};
use ghostty_vt::{
    Terminal, TerminalOptions,
    terminal::{ClipboardWritePolicy, Event as VtEvent, ProgressState},
};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use scytty_common::{
    connection::{connect, socket_path},
    protocol::{self, Progress, Request, TabInfo, Update},
};
use std::{
    collections::HashMap,
    io::{Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    sync::mpsc::{self, SyncSender},
    thread,
    time::Duration,
};

/// Bridge one multiplexed session connection; stdout must contain only protocol bytes.
pub fn bridge_stdio() -> Result<()> {
    let mut socket = connect()?;
    let mut input_socket = socket.try_clone()?;
    thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut input_socket);
        let _ = input_socket.shutdown(std::net::Shutdown::Write);
    });
    copy_flushed(&mut socket, &mut std::io::stdout().lock())?;
    Ok(())
}

fn copy_flushed(reader: &mut impl Read, writer: &mut impl Write) -> std::io::Result<()> {
    let mut bytes = [0; 16384];
    loop {
        let count = match reader.read(&mut bytes) {
            Ok(0) => return Ok(()),
            Ok(count) => count,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        writer.write_all(&bytes[..count])?;
        // SSH carries binary frames, so stdout cannot wait for a newline.
        writer.flush()?;
    }
}

mod cwd;
mod user_vars;

struct Tab {
    progress: Option<Progress>,
    user_vars: user_vars::UserVars,
    id: u64,
    terminal: Terminal,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    exited: bool,
}
impl Drop for Tab {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
struct Client {
    id: u64,
    output: SyncSender<Update>,
    socket: UnixStream,
}
impl Client {
    fn send(&self, update: Update) -> Result<()> {
        // Backpressure image bursts instead of dropping a healthy connection.
        // shortcut: stalled clients can pause other sessions until write timeout; isolate queues if needed.
        Ok(self.output.send(update)?)
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        // Stop requests, but let the writer drain final updates before closing output.
        let _ = self.socket.shutdown(std::net::Shutdown::Read);
    }
}
struct Session {
    scrollback_mb: u16,
    tabs: Vec<Tab>,
    active: u64,
    client: Option<Client>,
}
impl Session {
    fn close_tab(&mut self, id: u64) {
        let index = self.tabs.iter().position(|t| t.id == id);
        self.tabs.retain(|t| t.id != id);
        if self.active == id {
            self.active = index
                .and_then(|index| self.tabs.get(index.saturating_sub(1)))
                .map_or(0, |t| t.id);
        }
        self.state();
    }

    fn emit(&mut self, update: Update) {
        if let Some(client) = &self.client
            && let Err(error) = client.send(update)
        {
            eprintln!(
                "disconnecting client {}: output queue unavailable: {error:#}",
                client.id
            );
            self.client = None;
        }
    }
    fn state(&mut self) {
        let tabs = self
            .tabs
            .iter()
            .map(|t| TabInfo {
                id: t.id,
                title: t.terminal.title().unwrap_or("").to_string(),
                exited: t.exited,
                progress: t.progress.clone(),
                user_vars: t.user_vars.values.clone(),
            })
            .collect();
        self.emit(Update::State {
            tabs,
            active: self.active,
        });
    }
}
enum Event {
    Attach(Client, String),
    Request(u64, Request),
    Detached(u64),
    Output(String, u64, Vec<u8>),
    End(String, u64, Option<String>),
}
fn tab_cwd(tab: &Tab) -> Option<std::path::PathBuf> {
    if tab.exited {
        return None;
    }
    let directory = |pid| cwd::process_cwd(pid).filter(|path| path.is_absolute() && path.is_dir());
    tab.master
        .process_group_leader()
        .and_then(|pid| directory(pid as u32))
        .or_else(|| tab.child.process_id().and_then(directory))
}

fn new_tab(
    session: &str,
    id: u64,
    tx: &SyncSender<Event>,
    scrollback_mb: u16,
    cwd: Option<&std::path::Path>,
) -> Result<Tab> {
    let pair = native_pty_system().openpty(PtySize {
        cols: 80,
        rows: 24,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut command =
        CommandBuilder::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()));
    command.arg("-l");
    if let Some(cwd) = cwd {
        command.cwd(cwd);
    }
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("SCYTTY_SESSION", session);
    let writer = pair.master.take_writer()?;
    let mut reader = pair.master.try_clone_reader()?;
    let mut terminal = Terminal::new(TerminalOptions {
        cols: 80,
        rows: 24,
        ..Default::default()
    })?;
    terminal.set_scrollback_max_bytes(Some(usize::from(scrollback_mb) * 1024 * 1024))?;
    scytty_common::images::configure(&mut terminal)?;
    terminal.set_clipboard_write_policy(ClipboardWritePolicy::Deny);
    let child = pair.slave.spawn_command(command)?;
    drop(pair.slave);
    let name = session.to_string();
    let tx = tx.clone();
    thread::spawn(move || {
        let mut buf = [0; 16384];
        let error = loop {
            match reader.read(&mut buf) {
                Ok(0) => break None,
                Ok(n) => {
                    if tx
                        .send(Event::Output(name.clone(), id, buf[..n].to_vec()))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => break Some(e.to_string()),
            }
        };
        let _ = tx.send(Event::End(name, id, error));
    });
    Ok(Tab {
        progress: None,
        user_vars: user_vars::UserVars::default(),
        id,
        terminal,
        master: pair.master,
        writer,
        child,
        exited: false,
    })
}

fn progress_report(state: ProgressState, percent: Option<u8>) -> Option<Progress> {
    let state = match state {
        ProgressState::Remove => return None,
        ProgressState::Set => "normal",
        ProgressState::Error => "error",
        ProgressState::Indeterminate => "indeterminate",
        ProgressState::Pause => "paused",
    };
    Some(Progress {
        state: state.into(),
        percent,
    })
}

fn terminal_reply(bytes: &[u8]) -> &[u8] {
    // OSC 52 writes are denied, so do not advertise the Ms clipboard capability.
    if bytes.starts_with(b"\x1bP1+r4D73=") {
        b"\x1bP0+r4D73\x1b\\"
    } else {
        bytes
    }
}

fn snapshot(tab: &Tab) -> Result<Update> {
    let mut bytes = Vec::new();
    ghostty_vt::snapshot::encode_to(&tab.terminal, &mut bytes)?;
    Ok(Update::Snapshot { id: tab.id, bytes })
}

pub fn run() -> Result<()> {
    let path = socket_path()?;
    // A held advisory lock prevents competing auto-starts from unlinking a live socket.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("lock"))?;
    lock.try_lock().context("server already running")?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    let (tx, rx) = mpsc::sync_channel(256);
    let accept_tx = tx.clone();
    thread::spawn(move || {
        for (id, stream) in listener.incoming().enumerate() {
            let Ok(mut stream) = stream else {
                continue;
            };
            let tx = accept_tx.clone();
            thread::spawn(move || {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                if let Err(error) = protocol::handshake(&mut stream) {
                    eprintln!("client handshake: {error:#}");
                    return;
                }
                let Ok(Request::Attach { session }) = protocol::receive(&mut stream) else {
                    return;
                };
                if session.is_empty()
                    || session.len() > 128
                    || session.chars().any(char::is_control)
                {
                    return;
                }
                let _ = stream.set_read_timeout(None);
                let Ok(mut writer) = stream.try_clone() else {
                    return;
                };
                let Ok(socket) = stream.try_clone() else {
                    return;
                };
                let _ = writer.set_write_timeout(Some(Duration::from_secs(5)));
                let (output, updates) = mpsc::sync_channel(256);
                let client = Client {
                    id: id as u64,
                    output,
                    socket,
                };
                if tx.send(Event::Attach(client, session)).is_err() {
                    return;
                }
                thread::spawn(move || {
                    for update in updates {
                        if let Err(error) = protocol::send(&mut writer, &update) {
                            eprintln!("client {id}: writing update: {error:#}");
                            break;
                        }
                    }
                    let _ = writer.shutdown(std::net::Shutdown::Both);
                });
                loop {
                    match protocol::receive(&mut stream) {
                        Ok(request) => {
                            if tx.send(Event::Request(id as u64, request)).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            eprintln!("client {id}: request stream ended: {error:#}");
                            break;
                        }
                    }
                }
                let _ = tx.send(Event::Detached(id as u64));
            });
        }
    });
    let mut sessions: HashMap<String, Session> = HashMap::new();
    let mut next_tab = 1;
    for event in rx {
        let result: Result<()> = (|| {
            match event {
                Event::Attach(mut client, name) => {
                    if !sessions.contains_key(&name) {
                        let tab = new_tab(&name, next_tab, &tx, 16, None)?;
                        next_tab += 1;
                        sessions.insert(
                            name.clone(),
                            Session {
                                scrollback_mb: 16,
                                active: tab.id,
                                tabs: vec![tab],
                                client: None,
                            },
                        );
                    }
                    let session = sessions.get_mut(&name).unwrap();
                    if session.client.is_some() {
                        // Send before dropping Client, which shuts down its socket.
                        protocol::send(&mut client.socket, &Update::Error(
                            "Session is already attached to another window; close it before reattaching".into(),
                        ))?;
                        return Ok(());
                    }
                    if session.tabs.is_empty() {
                        let tab = new_tab(&name, next_tab, &tx, session.scrollback_mb, None)?;
                        next_tab += 1;
                        session.active = tab.id;
                        session.tabs.push(tab);
                    }
                    for tab in &session.tabs {
                        client.send(snapshot(tab)?)?;
                    }
                    session.client = Some(client);
                    session.state();
                }
                Event::Detached(id) => {
                    for session in sessions.values_mut() {
                        if session.client.as_ref().is_some_and(|c| c.id == id) {
                            session.client = None;
                        }
                    }
                }
                Event::Output(name, id, bytes) => {
                    if let Some(session) = sessions.get_mut(&name)
                        && let Some(tab) = session.tabs.iter_mut().find(|t| t.id == id)
                    {
                        let old_title = tab.terminal.title()?.to_string();
                        tab.terminal.vt_write(&bytes);
                        // Only the server answers terminal queries; client responses are discarded.
                        let old_progress = tab.progress.clone();
                        for event in tab.terminal.events() {
                            match event {
                                VtEvent::PtyWrite(reply) => {
                                    tab.writer.write_all(terminal_reply(&reply))?
                                }
                                VtEvent::ProgressReport { state, progress } => {
                                    tab.progress = progress_report(state, progress);
                                }
                                _ => {}
                            }
                        }
                        let vars_changed = tab.user_vars.feed(&bytes);
                        let changed = old_title != tab.terminal.title()?
                            || vars_changed
                            || old_progress != tab.progress;
                        session.emit(Update::Output { id, bytes });
                        if changed {
                            session.state();
                        }
                    }
                }
                Event::End(name, id, error) => {
                    if let Some(session) = sessions.get_mut(&name) {
                        if let Some(error) = error {
                            eprintln!("session {name}, tab {id}: {error}");
                        }
                        session.close_tab(id);
                    }
                }
                Event::Request(client_id, request) => {
                    if matches!(request, Request::CloseSession) {
                        let name = sessions
                            .iter()
                            .find(|(_, s)| s.client.as_ref().is_some_and(|c| c.id == client_id))
                            .map(|(name, _)| name.clone());
                        if let Some(name) = name {
                            sessions.remove(&name);
                        }
                        return Ok(());
                    }
                    let Some((name, session)) = sessions
                        .iter_mut()
                        .find(|(_, s)| s.client.as_ref().is_some_and(|c| c.id == client_id))
                    else {
                        return Ok(());
                    };
                    match request {
                        Request::SetScrollback { megabytes } => {
                            if megabytes > 64 {
                                bail!("scrollback_mb must be between 0 and 64");
                            }
                            for tab in &mut session.tabs {
                                tab.terminal.set_scrollback_max_bytes(Some(
                                    usize::from(megabytes) * 1024 * 1024,
                                ))?;
                            }
                            session.scrollback_mb = megabytes;
                        }
                        Request::NewTab => {
                            let cwd = session
                                .tabs
                                .iter()
                                .find(|tab| tab.id == session.active)
                                .and_then(tab_cwd);
                            let tab = new_tab(
                                name,
                                next_tab,
                                &tx,
                                session.scrollback_mb,
                                cwd.as_deref(),
                            )?;
                            next_tab += 1;
                            session.emit(snapshot(&tab)?);
                            session.active = tab.id;
                            session.tabs.push(tab);
                            session.state();
                        }
                        Request::Activate { id } => {
                            if session.tabs.iter().any(|t| t.id == id) {
                                session.active = id;
                                session.state();
                            }
                        }
                        Request::CloseTab { id } => session.close_tab(id),
                        Request::Input { id, bytes } => {
                            if let Some(tab) =
                                session.tabs.iter_mut().find(|t| t.id == id && !t.exited)
                            {
                                tab.writer.write_all(&bytes)?;
                                tab.writer.flush()?;
                            }
                        }
                        Request::Resize {
                            id,
                            cols,
                            rows,
                            width,
                            height,
                        } => {
                            if !(1..=500).contains(&cols)
                                || !(1..=300).contains(&rows)
                                || width > 256
                                || height > 256
                            {
                                bail!("invalid terminal dimensions");
                            }
                            if let Some(tab) = session.tabs.iter_mut().find(|t| t.id == id) {
                                tab.master.resize(PtySize {
                                    cols,
                                    rows,
                                    pixel_width: 0,
                                    pixel_height: 0,
                                })?;
                                tab.terminal.resize(cols, rows, width, height)?;
                                session.emit(Update::Resized {
                                    id,
                                    cols,
                                    rows,
                                    width,
                                    height,
                                });
                            }
                        }
                        Request::Attach { .. } => bail!("already attached"),
                        Request::CloseSession => unreachable!(),
                    }
                }
            }
            sessions.retain(|_, session| !session.tabs.is_empty());
            Ok(())
        })();
        if let Err(e) = result {
            eprintln!("{e:#}");
            for session in sessions.values_mut() {
                session.emit(Update::Error(e.to_string()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_bursts_wait_for_queue_capacity() -> Result<()> {
        let (socket, _peer) = UnixStream::pair()?;
        let (output, updates) = mpsc::sync_channel(1);
        let client = Client {
            id: 1,
            socket,
            output,
        };
        client.send(Update::Output {
            id: 1,
            bytes: vec![0],
        })?;
        let (started, waiting) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        let producer = thread::spawn(move || {
            started.send(()).unwrap();
            let result = client.send(Update::Output {
                id: 1,
                bytes: vec![1],
            });
            finished.send(result).unwrap();
        });
        waiting.recv()?;
        assert!(matches!(
            done.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(matches!(updates.recv()?, Update::Output { bytes, .. } if bytes == [0]));
        done.recv_timeout(Duration::from_secs(2))??;
        assert!(matches!(updates.recv()?, Update::Output { bytes, .. } if bytes == [1]));
        producer.join().unwrap();
        Ok(())
    }

    #[test]
    fn clipboard_capability_matches_denied_policy() -> Result<()> {
        let mut terminal = Terminal::new(TerminalOptions::default())?;
        terminal.set_clipboard_write_policy(ClipboardWritePolicy::Deny);
        terminal.vt_write(b"\x1bP+q4D73\x1b\\");
        let replies: Vec<_> = terminal
            .events()
            .filter_map(|event| match event {
                VtEvent::PtyWrite(bytes) => Some(terminal_reply(&bytes).to_vec()),
                _ => None,
            })
            .collect();
        assert_eq!(replies, [b"\x1bP0+r4D73\x1b\\".to_vec()]);
        assert_eq!(terminal_reply(b"\x1b[1;1R"), b"\x1b[1;1R");
        Ok(())
    }

    #[test]
    fn bridge_flushes_without_newlines() -> std::io::Result<()> {
        let mut output = Vec::new();
        {
            let mut buffered = std::io::BufWriter::new(&mut output);
            copy_flushed(&mut &b"\0\x01\x02"[..], &mut buffered)?;
            assert!(buffered.buffer().is_empty());
        }
        assert_eq!(output, b"\0\x01\x02");
        Ok(())
    }
}
