use base64::{Engine, engine::general_purpose::STANDARD};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Metadata {
    pub mouse_shape: Option<String>,
    pub values: BTreeMap<String, String>,
    pub program_status: crate::program_status::Records,
    pub replies: Vec<u8>,
    osc: Option<Vec<u8>>,
    escape: bool,
}
impl Metadata {
    pub fn feed(&mut self, bytes: &[u8]) -> bool {
        let mut changed = false;
        for &byte in bytes {
            if let Some(mut osc) = self.osc.take() {
                if matches!(byte, 0x18 | 0x1a) {
                    self.escape = false;
                    continue;
                }
                if byte == 7 || (self.escape && byte == b'\\') {
                    changed |= self.apply(&osc, if byte == 7 { 7 } else { b'\\' });
                    self.escape = false;
                    continue;
                }
                if self.escape {
                    osc.push(27);
                }
                self.escape = byte == 27;
                if !self.escape {
                    osc.push(byte);
                }
                // Bound memory for malformed or untrusted OSC output.
                if osc.len() <= 8192 {
                    self.osc = Some(osc);
                }
            } else {
                if self.escape && byte == b']' {
                    self.osc = Some(Vec::new());
                } else if self.escape && byte == b'c' {
                    changed |= self.mouse_shape.take().is_some();
                    changed |= self.program_status.clear();
                }
                self.escape = byte == 27;
            }
        }
        changed
    }
    fn apply(&mut self, bytes: &[u8], terminator: u8) -> bool {
        if bytes == b"7501;?" {
            // Bound query-response amplification from untrusted programs.
            if self.replies.len() < 65536 {
                self.replies.extend_from_slice(b"\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth");
                self.replies
                    .extend_from_slice(if terminator == 7 { b"\x07" } else { b"\x1b\\" });
            }
            return false;
        }
        if bytes.starts_with(b"7501;") {
            return self.program_status.apply(bytes, terminator);
        }
        if bytes == b"133;A" || bytes.starts_with(b"133;A;") {
            return self.program_status.prompt();
        }
        // The VT binding exposes neither mouse shape nor OSC 22 events. Keep this
        // small piece of presentation state alongside the snapshot for reattachment.
        if let Some(value) = bytes.strip_prefix(b"22;") {
            let Ok(shape) = std::str::from_utf8(value) else {
                return false;
            };
            if !matches!(
                shape,
                "" | "default"
                    | "context-menu"
                    | "help"
                    | "pointer"
                    | "progress"
                    | "wait"
                    | "cell"
                    | "crosshair"
                    | "text"
                    | "vertical-text"
                    | "alias"
                    | "copy"
                    | "move"
                    | "no-drop"
                    | "not-allowed"
                    | "grab"
                    | "grabbing"
                    | "all-scroll"
                    | "col-resize"
                    | "row-resize"
                    | "n-resize"
                    | "e-resize"
                    | "s-resize"
                    | "w-resize"
                    | "ne-resize"
                    | "nw-resize"
                    | "se-resize"
                    | "sw-resize"
                    | "ew-resize"
                    | "ns-resize"
                    | "nesw-resize"
                    | "nwse-resize"
                    | "zoom-in"
                    | "zoom-out"
            ) {
                return false;
            }
            let next = if shape.is_empty() {
                None
            } else {
                Some(shape.to_owned())
            };
            if self.mouse_shape == next {
                return false;
            }
            self.mouse_shape = next;
            return true;
        }
        let Some(payload) = bytes.strip_prefix(b"1337;SetUserVar=") else {
            return false;
        };
        let Some(eq) = payload.iter().position(|&b| b == b'=') else {
            return false;
        };
        let Ok(name) = std::str::from_utf8(&payload[..eq]) else {
            return false;
        };
        if name.is_empty()
            || name.len() > 128
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return false;
        }
        let encoded: Vec<_> = payload[eq + 1..]
            .iter()
            .copied()
            .filter(|b| !b.is_ascii_whitespace())
            .collect();
        let Ok(value) = STANDARD.decode(&encoded) else {
            return false;
        };
        let Ok(value) = String::from_utf8(value) else {
            return false;
        };
        if value.len() > 4096 || (self.values.len() >= 64 && !self.values.contains_key(name)) {
            return false;
        }
        if self.values.get(name) == Some(&value) {
            return false;
        }
        self.values.insert(name.to_owned(), value);
        true
    }
}

#[cfg(test)]
#[path = "../tests/unit/metadata.rs"]
mod tests;
