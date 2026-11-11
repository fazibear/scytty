use base64::{
    Engine,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
};
use ghostty_vt::ffi;
use scytty_common::protocol::ProgramStatus;
use std::collections::{BTreeMap, VecDeque};

#[derive(Default)]
pub struct Records {
    values: BTreeMap<String, ProgramStatus>,
    updated: VecDeque<String>,
}
impl Records {
    pub fn state(&self) -> Option<String> {
        self.updated
            .back()
            .and_then(|id| self.values.get(id))
            .map(|record| record.state.clone())
    }
    pub fn snapshot(&self) -> BTreeMap<String, ProgramStatus> {
        self.values
            .iter()
            .map(|(id, value)| {
                let mut value = value.clone();
                let mut ancestor = id.as_str();
                while value.app.is_none() {
                    if let Some(parent) = self.values.get(ancestor) {
                        value.app = parent.app.clone();
                    }
                    if ancestor.is_empty() {
                        break;
                    }
                    ancestor = ancestor.rsplit_once('/').map_or("", |(parent, _)| parent);
                }
                (id.clone(), value)
            })
            .collect()
    }
    pub fn clear(&mut self) -> bool {
        let changed = !self.values.is_empty();
        self.values.clear();
        self.updated.clear();
        changed
    }
    pub fn prompt(&mut self) -> bool {
        let previous = self.values.len();
        self.values
            .retain(|_, value| matches!(value.state.as_str(), "done" | "error"));
        self.updated.retain(|id| self.values.contains_key(id));
        previous != self.values.len()
    }
    pub fn apply(&mut self, bytes: &[u8], terminator: u8) -> bool {
        // The safe OSC wrapper panics on invalid input; the C API returns null instead.
        // SAFETY: the parser is owned here, checked before use, and freed after its last access.
        let valid = unsafe {
            let mut parser = std::ptr::null_mut();
            if ffi::ghostty_osc_new(std::ptr::null(), &mut parser) != ffi::Result::SUCCESS
                || parser.is_null()
            {
                return false;
            }
            for &byte in bytes {
                ffi::ghostty_osc_next(parser, byte);
            }
            let command = ffi::ghostty_osc_end(parser, terminator);
            let valid = !command.is_null()
                && ffi::ghostty_osc_command_type(command) == ffi::OscCommandType::PROGRAM_STATUS;
            ffi::ghostty_osc_free(parser);
            valid
        };
        if !valid {
            return false;
        }
        let Ok(body) = std::str::from_utf8(bytes.strip_prefix(b"7501;").unwrap_or_default()) else {
            return false;
        };
        let value = |key| {
            body.split(':')
                .filter_map(|pair| pair.split_once('='))
                .filter(|(name, value)| {
                    name.trim_ascii() == key
                        && value
                            .trim_ascii()
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_.+-,/=".contains(&b))
                })
                .map(|(_, value)| value.trim_ascii())
                .next_back()
        };
        let Some(state) = value("state") else {
            return false;
        };
        let id = value("id").unwrap_or("").to_owned();
        if state == "clear" {
            if id.is_empty() {
                return self.clear();
            }
            let previous = self.values.len();
            let prefix = id.clone() + "/";
            self.values
                .retain(|key, _| key != &id && !key.starts_with(&prefix));
            self.updated.retain(|key| self.values.contains_key(key));
            return previous != self.values.len();
        }
        let text = |key| -> Option<String> {
            let encoded = value(key).filter(|s| !s.is_empty())?;
            let bytes = STANDARD
                .decode(encoded)
                .or_else(|_| STANDARD_NO_PAD.decode(encoded))
                .ok()?;
            String::from_utf8(bytes).ok()
        };
        let record = ProgramStatus {
            state: state.to_owned(),
            app: value("app")
                .filter(|s| {
                    !s.is_empty()
                        && s.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_.+-".contains(&b))
                })
                .map(str::to_owned),
            kind: (state == "blocked")
                .then(|| {
                    value("kind").filter(|kind| matches!(*kind, "permission" | "question" | "auth"))
                })
                .flatten()
                .map(str::to_owned),
            progress: matches!(state, "working" | "blocked")
                .then(|| {
                    value("progress")
                        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                        .and_then(|s| s.parse::<u8>().ok())
                        .filter(|&n| n <= 100)
                })
                .flatten(),
            title: text("title"),
            message: text("msg"),
        };
        let previous_state = self.state();
        let mut changed = self.values.get(&id) != Some(&record);
        self.updated.retain(|key| key != &id);
        if self.values.len() == 256
            && !self.values.contains_key(&id)
            && let Some(oldest) = self.updated.pop_front()
        {
            self.values.remove(&oldest);
            changed = true;
        }
        self.values.insert(id.clone(), record);
        self.updated.push_back(id);
        changed || previous_state != self.state()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report(records: &mut Records, body: &str) -> bool {
        records.apply(format!("7501;{body}").as_bytes(), 7)
    }

    #[test]
    fn replaces_inherits_clears_and_bounds_records() {
        let mut records = Records::default();
        assert!(report(
            &mut records,
            "state=working:app=cargo:progress=40:msg=SGVsbG8="
        ));
        assert_eq!(records.state().as_deref(), Some("working"));
        assert_eq!(records.snapshot()[""].message.as_deref(), Some("Hello"));
        assert!(report(
            &mut records,
            "state=blocked:id=build/test:kind=permission:progress=50:title=VGVzdA"
        ));
        assert_eq!(
            records.snapshot()["build/test"].app.as_deref(),
            Some("cargo")
        );
        assert_eq!(
            records.snapshot()["build/test"].title.as_deref(),
            Some("Test")
        );
        assert!(report(&mut records, "state=done:id=build/test"));
        assert_eq!(records.snapshot()["build/test"].kind, None);
        assert_eq!(records.snapshot()["build/test"].progress, None);
        assert_eq!(records.snapshot()["build/test"].title, None);
        assert!(records.prompt());
        assert_eq!(records.state().as_deref(), Some("done"));
        assert!(report(&mut records, "state=clear:id=build"));
        assert_eq!(records.state(), None);
        for n in 0..256 {
            report(&mut records, &format!("state=working:id=task{n}"));
        }
        assert!(!report(&mut records, "state=working:id=task0"));
        assert!(report(&mut records, "state=working:id=task256"));
        assert_eq!(records.values.len(), 256);
        assert!(records.values.contains_key("task0"));
        assert!(!records.values.contains_key("task1"));
        assert!(report(&mut records, "state=clear"));
        assert!(!records.clear());
    }
    #[test]
    fn clear_is_subtree_scoped_and_latest_state_is_not_an_aggregate() {
        let mut records = Records::default();
        report(&mut records, "state=working:app=cargo");
        report(&mut records, "state=working:id=builder");
        report(&mut records, "state=blocked:id=build/test");
        assert_eq!(records.state().as_deref(), Some("blocked"));
        assert!(report(&mut records, "state=working:app=cargo"));
        assert_eq!(records.state().as_deref(), Some("working"));
        report(&mut records, "state=clear:id=build");
        assert!(records.values.contains_key("builder"));
        assert!(!records.values.contains_key("build/test"));
        assert!(records.values.contains_key(""));
    }
    #[test]
    fn engine_validates_untrusted_reports_and_duplicate_keys() {
        let mut records = Records::default();
        for body in [
            "state=unknown",
            "state=clear:id=bad//id",
            "state=done:msg=A",
            "state=done:msg=AA==",
            "app=cargo",
        ] {
            assert!(!report(&mut records, body), "{body}");
        }
        assert_eq!(records.state(), None);
        assert!(report(
            &mut records,
            "state=working:state=blocked:kind=auth:progress=999:app=cargo"
        ));
        assert_eq!(records.state().as_deref(), Some("blocked"));
        assert_eq!(records.snapshot()[""].kind.as_deref(), Some("auth"));
        assert_eq!(records.snapshot()[""].progress, None);
    }
}
