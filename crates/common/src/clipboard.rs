use anyhow::Result;
use ghostty_vt::{Terminal, terminal::ClipboardWritePolicy};

pub const MAX_BYTES: usize = 8 * 1024 * 1024;

pub fn configure(terminal: &mut Terminal) -> Result<()> {
    terminal.set_clipboard_write_max_bytes(Some(MAX_BYTES))?;
    terminal.set_clipboard_write_policy(ClipboardWritePolicy::Allow);
    // Clipboard reads intentionally have no callback: remote applications cannot read the host clipboard.
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/clipboard.rs"]
mod tests;
