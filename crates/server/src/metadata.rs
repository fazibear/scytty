use base64::{Engine, engine::general_purpose::STANDARD};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Metadata {
    pub mouse_shape: Option<String>,
    pub values: BTreeMap<String, String>,
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
                    changed |= self.apply(&osc);
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
                }
                self.escape = byte == 27;
            }
        }
        changed
    }
    fn apply(&mut self, bytes: &[u8]) -> bool {
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
mod tests {
    use super::*;

    #[test]
    fn mouse_shapes_are_bounded_fragmented_and_resettable() {
        let mut metadata = Metadata::default();
        assert!(!metadata.feed(b"\x1b]22;poi"));
        assert!(metadata.feed(b"nter\x1b\\"));
        assert_eq!(metadata.mouse_shape.as_deref(), Some("pointer"));
        assert!(!metadata.feed(b"\x1b]22;pointer\x07"));
        assert!(!metadata.feed(b"\x1b]22;invalid\x07"));
        assert!(!metadata.feed(b"\x1b]22;text\x18\x07"));
        assert_eq!(metadata.mouse_shape.as_deref(), Some("pointer"));
        assert!(metadata.feed(b"\x1b]22;\x07"));
        assert_eq!(metadata.mouse_shape, None);
        assert!(metadata.feed(b"\x1b]22;default\x07"));
        assert_eq!(metadata.mouse_shape.as_deref(), Some("default"));
        assert!(metadata.feed(b"\x1bc"));
        assert_eq!(metadata.mouse_shape, None);
        let mut oversized = b"\x1b]22;".to_vec();
        oversized.extend(std::iter::repeat_n(b'x', 9000));
        oversized.extend_from_slice(b"\x07");
        assert!(!metadata.feed(&oversized));
        assert!(metadata.feed(b"\x1b]22;crosshair\x07"));
    }
    #[test]
    fn split_sequences_and_validation() {
        let mut vars = Metadata::default();
        let bytes = b"text\x1b]1337;SetUserVar=right_status=aGVsbG8=\x1b\\";
        for &b in bytes {
            vars.feed(&[b]);
        }
        assert_eq!(vars.values["right_status"], "hello");
        assert!(!vars.feed(bytes));
        assert!(
            vars.feed(b"\x1b]1337;SetUserVar=right_status_timestamp=MTc5NDAw\r\nOTM3MA==\r\x07")
        );
        assert_eq!(vars.values["right_status_timestamp"], "1794009370");
        assert!(vars.feed(b"\x1b]1337;SetUserVar=right_status=\x07"));
        assert_eq!(vars.values["right_status"], "");
        assert!(!vars.feed(b"\x1b]1337;SetUserVar=bad=***\x07"));
        vars.feed(&[b' '; 10000]);
        assert!(vars.feed(b"\x1b]1337;SetUserVar=x=eQ==\x07"));
        assert_eq!(vars.values["x"], "y");
        let mut oversized = b"\x1b]1337;SetUserVar=x=".to_vec();
        oversized.extend([b'A'; 10000]);
        oversized.push(7);
        vars.feed(&oversized);
        assert_eq!(vars.values["x"], "y");
        assert!(vars.feed(b"\x1b]1337;SetUserVar=x=eg==\x07"));
    }
}
