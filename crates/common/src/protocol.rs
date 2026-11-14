use anyhow::{Context, Result, bail};
use bincode::Options;
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

// Bump whenever messages or the Ghostty snapshot format become incompatible.
pub const PROTOCOL_VERSION: u32 = 6;
const MAGIC: &[u8; 8] = b"SCYTTY\r\n";

/// Fixed preamble: both peers write before reading, independent of frame encoding.
pub fn handshake(stream: &mut (impl Read + Write)) -> Result<()> {
    stream
        .write_all(MAGIC)
        .context("sending scytty handshake")?;
    stream.write_all(&PROTOCOL_VERSION.to_be_bytes())?;
    stream.flush()?;
    let mut peer = [0; 12];
    stream.read_exact(&mut peer).context(
        "scytty handshake failed: peer closed or timed out before identifying its protocol; it may be an older server. Update both binaries and restart the daemon when safe (restart ends sessions)",
    )?;
    if &peer[..8] != MAGIC {
        bail!(
            "Not a compatible scytty peer (missing protocol handshake). Update both binaries and restart the daemon when safe (restart ends sessions)"
        );
    }
    let version = u32::from_be_bytes(peer[8..].try_into().unwrap());
    if version != PROTOCOL_VERSION {
        bail!(
            "scytty protocol mismatch: local {PROTOCOL_VERSION}, peer {version}. Update both binaries and restart the daemon when safe (restart ends sessions)"
        );
    }
    Ok(())
}

pub fn validate_bin(bin: &str) -> Result<()> {
    if bin.trim().is_empty() || bin.len() > 4096 || bin.chars().any(char::is_control) {
        bail!(
            "bin must be a nonempty executable path or name, at most 4096 bytes, without control characters"
        );
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Request {
    Attach {
        session: String,
        bin: Option<String>,
    },
    NewTab,
    CloseTab {
        id: u64,
    },
    Activate {
        id: u64,
    },
    Input {
        id: u64,
        bytes: Vec<u8>,
    },
    Resize {
        id: u64,
        cols: u16,
        rows: u16,
        width: u32,
        height: u32,
    },
    CloseSession,
    SetScrollback {
        megabytes: u16,
    },
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub state: String,
    pub percent: Option<u8>,
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct ProgramStatus {
    pub state: String,
    pub app: Option<String>,
    pub kind: Option<String>,
    pub progress: Option<u8>,
    pub title: Option<String>,
    pub message: Option<String>,
}
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct TabInfo {
    pub state: Option<String>,
    pub program_status: std::collections::BTreeMap<String, ProgramStatus>,
    pub id: u64,
    pub title: String,
    pub exited: bool,
    pub progress: Option<Progress>,
    pub user_vars: std::collections::BTreeMap<String, String>,
    pub mouse_shape: Option<String>,
}
#[derive(Serialize, Deserialize, Debug)]
pub enum Update {
    State {
        tabs: Vec<TabInfo>,
        active: u64,
    },
    Snapshot {
        id: u64,
        bytes: Vec<u8>,
    },
    Output {
        id: u64,
        bytes: Vec<u8>,
    },
    Resized {
        id: u64,
        cols: u16,
        rows: u16,
        width: u32,
        height: u32,
    },
    Error(String),
}
// Bound allocations at the socket trust boundary, including snapshot frames.
// Allow 64 MiB scrollback plus both screens' bounded image storage in snapshots.
const MAX_FRAME: usize = 128 * 1024 * 1024;
pub fn send<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<()> {
    let data = bincode::DefaultOptions::new()
        .with_limit(MAX_FRAME as u64)
        .serialize(value)?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&data)?;
    let compressed = encoder.finish()?;
    if compressed.len() > MAX_FRAME {
        bail!("compressed protocol frame too large");
    }
    writer.write_all(&(compressed.len() as u32).to_be_bytes())?;
    writer.write_all(&compressed)?;
    Ok(())
}
pub fn receive<T: for<'a> Deserialize<'a>>(reader: &mut impl Read) -> Result<T> {
    let mut length = [0; 4];
    reader
        .read_exact(&mut length)
        .context("reading protocol frame header")?;
    let length = u32::from_be_bytes(length) as usize;
    if length > MAX_FRAME {
        bail!("protocol frame too large");
    }
    let mut data = vec![0; length];
    reader
        .read_exact(&mut data)
        .with_context(|| format!("reading {length}-byte compressed protocol frame"))?;
    let mut decoder = GzDecoder::new(&data[..]);
    let mut decoded = Vec::new();
    decoder
        .by_ref()
        .take(MAX_FRAME as u64 + 1)
        .read_to_end(&mut decoded)
        .context("decompressing protocol frame")?;
    if decoded.len() > MAX_FRAME {
        bail!("decompressed protocol frame too large");
    }
    if !decoder.into_inner().is_empty() {
        bail!("trailing compressed data");
    }
    bincode::DefaultOptions::new()
        .with_limit(MAX_FRAME as u64)
        .deserialize(&decoded)
        .with_context(|| format!("decoding {}-byte protocol message", decoded.len()))
}
#[cfg(test)]
#[path = "../tests/unit/protocol.rs"]
mod tests;
