use super::*;
use ghostty_vt::{TerminalOptions, terminal::Event};

#[test]
fn writes_both_protocols_but_never_reads() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    configure(&mut terminal)?;
    terminal.vt_write(b"\x1b]52;c;aGVs");
    assert!(
        !terminal
            .events()
            .any(|event| matches!(event, Event::ClipboardWrite(_)))
    );
    terminal.vt_write(b"bG8=\x07");
    let write = terminal
        .events()
        .find_map(|event| match event {
            Event::ClipboardWrite(write) => Some(write),
            _ => None,
        })
        .expect("OSC 52 write");
    assert_eq!(write.contents[0].data, b"hello");
    terminal.vt_write(b"\x1b]5522;type=write\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;d29ybGQ=\x1b\\\x1b]5522;type=wdata\x1b\\");
    let events: Vec<_> = terminal.events().collect();
    let write = events
        .iter()
        .find_map(|event| match event {
            Event::ClipboardWrite(write) => Some(write),
            _ => None,
        })
        .expect("OSC 5522 write");
    assert_eq!(write.contents[0].mime, "text/plain");
    assert_eq!(write.contents[0].data, b"world");
    assert!(events.iter().any(|event| matches!(event, Event::PtyWrite(bytes) if bytes.windows(11).any(|s| s == b"status=DONE"))));
    terminal.vt_write(b"\x1b]52;c;?\x07\x1b]5522;type=read;dGV4dC9wbGFpbg==\x1b\\");
    let events: Vec<_> = terminal.events().collect();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::ClipboardWrite(_)))
    );
    assert!(events.iter().any(|event| matches!(event, Event::PtyWrite(bytes) if bytes == b"\x1b]5522;type=read:status=EPERM\x1b\\")));
    Ok(())
}
