use base64::{Engine, engine::general_purpose::STANDARD};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct UserVars {
    pub values: BTreeMap<String, String>,
    osc: Option<Vec<u8>>,
    escape: bool,
}
impl UserVars {
    pub fn feed(&mut self, bytes: &[u8]) -> bool {
        let mut changed = false;
        for &byte in bytes {
            if let Some(mut osc) = self.osc.take() {
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
                }
                self.escape = byte == 27;
            }
        }
        changed
    }
    fn apply(&mut self, bytes: &[u8]) -> bool {
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
    fn split_sequences_and_validation() {
        let mut vars = UserVars::default();
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
