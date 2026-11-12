use anyhow::{Context, Result, bail};
use mlua::{Function, Lua, Table, Value};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub enum MenuAction {
    NewTab,
    NewWindow,
    ReloadConfig,
    Connect {
        host: String,
        session: String,
        bin: Option<String>,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub struct MenuEntry {
    pub label: String,
    pub action: MenuAction,
}

#[derive(Debug)]
pub struct Config {
    pub plus_menu: Vec<MenuEntry>,
    pub font_size: f32,
    pub default_cursor_style: ghostty_vt::terminal::CursorStyle,
    pub scrollback_mb: u16,
    pub font_family: String,
    pub icon_scale: f32,
    pub background_opacity: f32,
    pub window_width: f32,
    pub window_height: f32,
    pub status: Table,
    lua: Lua,
}
impl Default for Config {
    fn default() -> Self {
        let lua = Lua::new();
        let status = lua
            .load(include_str!("status.lua"))
            .eval()
            .expect("bundled status Lua");
        Self {
            plus_menu: vec![
                MenuEntry {
                    label: "New tab".into(),
                    action: MenuAction::NewTab,
                },
                MenuEntry {
                    label: "New window".into(),
                    action: MenuAction::NewWindow,
                },
                MenuEntry {
                    label: "Reload config".into(),
                    action: MenuAction::ReloadConfig,
                },
            ],
            font_size: 16.0,
            default_cursor_style: ghostty_vt::terminal::CursorStyle::Block,
            scrollback_mb: 16,
            font_family: "monospace".into(),
            icon_scale: 1.5,
            background_opacity: 0.7,
            window_width: 960.0,
            window_height: 640.0,
            status,
            lua,
        }
    }
}
impl Config {
    pub fn load() -> Result<Self> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(|home| PathBuf::from(home).join(".config"))
            })
            .context("HOME, USERPROFILE or XDG_CONFIG_HOME must be set")?;
        let path = base.join("scytty/config.lua");
        match std::fs::read_to_string(&path) {
            Ok(source) => Self::parse_at(&source, path.parent())
                .with_context(|| format!("invalid config: {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
        }
    }
    pub fn window_title(&self, session: &str, title: &str, remote: bool) -> Result<String> {
        let ctx = self.lua.create_table()?;
        ctx.set("session", session)?;
        ctx.set("title", title)?;
        ctx.set("remote", remote)?;
        let function: Function = self.status.get("format_window_title")?;
        match function
            .call::<Value>(ctx)
            .context("format_window_title callback failed")?
        {
            Value::String(text) => Ok(text.to_str()?.to_owned()),
            _ => bail!("format_window_title must return a string"),
        }
    }
    pub fn status_text(
        &self,
        name: &str,
        title: &str,
        index: usize,
        active: bool,
        remote: bool,
        tab: Option<&scytty_common::protocol::TabInfo>,
    ) -> Result<Vec<(String, Option<gpui::Rgba>)>> {
        let ctx = self.lua.create_table()?;
        ctx.set("title", title)?;
        ctx.set("index", index + 1)?;
        ctx.set("active", active)?;
        ctx.set("remote", remote)?;
        ctx.set(
            "user_vars",
            self.lua.create_table_from(
                tab.map(|t| &t.user_vars)
                    .into_iter()
                    .flatten()
                    .map(|(k, v)| (k.as_str(), v.as_str())),
            )?,
        )?;
        if let Some(progress) = tab.and_then(|t| t.progress.as_ref()) {
            let value = self.lua.create_table()?;
            value.set("state", progress.state.as_str())?;
            value.set("percent", progress.percent)?;
            ctx.set("progress", value)?;
        }
        ctx.set("state", tab.and_then(|tab| tab.state.as_deref()))?;
        let records = self.lua.create_table()?;
        if let Some(tab) = tab {
            for (id, status) in &tab.program_status {
                let value = self.lua.create_table()?;
                value.set("state", status.state.as_str())?;
                value.set("app", status.app.as_deref())?;
                value.set("kind", status.kind.as_deref())?;
                value.set("progress", status.progress)?;
                value.set("title", status.title.as_deref())?;
                value.set("message", status.message.as_deref())?;
                records.set(id.as_str(), value)?;
            }
        }
        ctx.set("program_status", records)?;
        let function: Function = self.status.get(name)?;
        let value: Value = function
            .call(ctx)
            .with_context(|| format!("{name} callback failed"))?;
        let segment = |text: String, color: Option<String>| -> Result<_> {
            let color = color
                .map(|c| {
                    gpui::Rgba::try_from(c.as_str())
                        .map_err(|_| anyhow::anyhow!("invalid status color: {c}; use #RRGGBB"))
                })
                .transpose()?;
            Ok((text.replace(['\n', '\r', '\t'], " "), color))
        };
        let raw: Vec<_> = match value {
            Value::String(text) => Ok(vec![segment(text.to_str()?.to_owned(), None)?]),
            Value::Table(segments) => segments
                .sequence_values::<Table>()
                .map(|s| {
                    let s = s?;
                    segment(s.get("text")?, s.get("color")?)
                })
                .collect(),
            _ => bail!("{name} must return a string or an array of {{text, color}} segments"),
        }?;
        Ok(raw
            .into_iter()
            .flat_map(|(text, color)| crate::ansi::segments(&text, color))
            .collect())
    }
    #[cfg(test)]
    fn parse(source: &str) -> Result<Self> {
        Self::parse_at(source, None)
    }
    fn parse_at(source: &str, directory: Option<&std::path::Path>) -> Result<Self> {
        let lua = Lua::new();
        if let Some(directory) = directory {
            let directory = directory
                .to_str()
                .context("config directory must be UTF-8")?;
            let package: Table = lua.globals().get("package")?;
            let path: String = package.get("path")?;
            package.set(
                "path",
                format!("{directory}/?.lua;{directory}/?/init.lua;{path}"),
            )?;
        }
        let table: Table = lua.load(source).set_name("config.lua").eval()?;
        let mut config = Self {
            status: lua.load(include_str!("status.lua")).eval()?,
            lua,
            ..Self::default()
        };
        for pair in table.pairs::<String, Value>() {
            let (key, raw) = pair?;
            if key == "plus_menu" {
                let Value::Table(entries) = raw else {
                    bail!("plus_menu must be an array")
                };
                let count = entries
                    .clone()
                    .pairs::<usize, Table>()
                    .map(|entry| -> Result<usize> {
                        let (index, _) = entry?;
                        if index == 0 || index > entries.raw_len() {
                            bail!("plus_menu must be a contiguous array");
                        }
                        Ok(index)
                    })
                    .collect::<Result<Vec<_>>>()?
                    .len();
                if count != entries.raw_len() {
                    bail!("plus_menu must be a contiguous array");
                }
                config.plus_menu = entries
                    .sequence_values::<Table>()
                    .map(|entry| -> Result<MenuEntry> {
                        let entry = entry?;
                        for key in entry.clone().pairs::<String, Value>() {
                            let (key, _) = key?;
                            if !matches!(
                                key.as_str(),
                                "label" | "action" | "host" | "session" | "bin"
                            ) {
                                bail!("unknown plus_menu field: {key}");
                            }
                        }
                        let label: String = entry.get("label")?;
                        if label.trim().is_empty() || label.chars().any(char::is_control) {
                            bail!("plus_menu label must be nonempty text");
                        }
                        let name: String = entry.get("action")?;
                        let action = match name.as_str() {
                            "new_tab" | "new_window" | "reload_config" => {
                                if entry.contains_key("host")?
                                    || entry.contains_key("session")?
                                    || entry.contains_key("bin")?
                                {
                                    bail!("host/session/bin require a connect action");
                                }
                                if name == "new_tab" {
                                    MenuAction::NewTab
                                } else if name == "new_window" {
                                    MenuAction::NewWindow
                                } else {
                                    MenuAction::ReloadConfig
                                }
                            }
                            "connect" => {
                                let host: String = entry.get("host")?;
                                crate::transport::validate_host(&host)?;
                                let session = entry
                                    .get::<Option<String>>("session")?
                                    .unwrap_or_else(|| "default".into());
                                if session.trim().is_empty()
                                    || session.len() > 128
                                    || session.chars().any(char::is_control)
                                {
                                    bail!("connect session must be nonempty text");
                                }
                                let bin = entry.get::<Option<String>>("bin")?;
                                if let Some(bin) = &bin {
                                    scytty_common::protocol::validate_bin(bin)?;
                                }
                                MenuAction::Connect { host, session, bin }
                            }
                            _ => bail!("unknown plus_menu action: {name}"),
                        };
                        Ok(MenuEntry { label, action })
                    })
                    .collect::<Result<_>>()?;
                continue;
            }
            if key == "default_cursor_style" {
                use ghostty_vt::terminal::CursorStyle;
                let Value::String(value) = raw else {
                    bail!("default_cursor_style must be a string")
                };
                config.default_cursor_style = match value.to_str()?.as_ref() {
                    "block" => CursorStyle::Block,
                    "bar" => CursorStyle::Bar,
                    "underline" => CursorStyle::Underline,
                    "block_hollow" => CursorStyle::BlockHollow,
                    _ => {
                        bail!("default_cursor_style must be block, bar, underline or block_hollow")
                    }
                };
                continue;
            }
            if key == "font_family" {
                let Value::String(value) = raw else {
                    bail!("font_family must be a string")
                };
                let value = value.to_str()?;
                if value.trim().is_empty() {
                    bail!("font_family must not be empty")
                }
                config.font_family = value.to_owned();
                continue;
            }
            if matches!(
                key.as_str(),
                "left_status" | "right_status" | "format_tab" | "format_window_title"
            ) {
                let Value::Function(_) = raw else {
                    bail!("{key} must be a function")
                };
                config.status.set(key, raw)?;
                continue;
            }
            let value = match raw {
                Value::Number(n) => n as f32,
                Value::Integer(n) => n as f32,
                _ => bail!("{key} must be a number"),
            };
            if key == "scrollback_mb" {
                if !value.is_finite() || !(0.0..=64.0).contains(&value) || value.fract() != 0.0 {
                    bail!("scrollback_mb must be a whole number between 0 and 64");
                }
                config.scrollback_mb = value as u16;
                continue;
            }
            let (target, min, max) = match key.as_str() {
                "font_size" => (&mut config.font_size, 6.0, 96.0),
                "icon_scale" => (&mut config.icon_scale, 1.0, 3.0),
                "background_opacity" => (&mut config.background_opacity, 0.0, 1.0),
                "window_width" => (&mut config.window_width, 200.0, 16384.0),
                "window_height" => (&mut config.window_height, 100.0, 16384.0),
                _ => bail!("unknown option: {key}"),
            };
            if !value.is_finite() || value < min || value > max {
                bail!("{key} must be between {min} and {max}");
            }
            *target = value;
        }
        Ok(config)
    }
}
#[cfg(test)]
#[path = "../tests/unit/config.rs"]
mod tests;
