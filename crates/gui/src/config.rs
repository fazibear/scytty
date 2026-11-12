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
mod tests {
    use super::*;
    #[test]
    fn configurable_plus_menu() -> Result<()> {
        let config = Config::parse(
            "return {plus_menu={{label='New',action='new_tab'},{label='Window',action='new_window'},{label='Prod',action='connect',host='user@host',session='work'}}}",
        )?;
        assert_eq!(config.plus_menu.len(), 3);
        assert_eq!(
            config.plus_menu[2].action,
            MenuAction::Connect {
                host: "user@host".into(),
                session: "work".into(),
                bin: None,
            }
        );
        let custom = Config::parse(
            "return {plus_menu={{label='Fish',action='connect',host='192.168.1.2',bin='/usr/bin/fish'}}}",
        )?;
        assert!(
            matches!(&custom.plus_menu[0].action, MenuAction::Connect { bin: Some(bin), .. } if bin == "/usr/bin/fish")
        );
        assert!(Config::parse("return {plus_menu={}}")?.plus_menu.is_empty());
        assert!(
            Config::default()
                .plus_menu
                .iter()
                .any(|entry| entry.action == MenuAction::ReloadConfig)
        );
        assert_eq!(
            Config::parse("return {plus_menu={{label='Reload config',action='reload_config'}}}")?
                .plus_menu[0]
                .action,
            MenuAction::ReloadConfig
        );
        assert!(
            Config::parse(
                "return {plus_menu={{label='Reload',action='reload_config',host='host'}}}"
            )
            .is_err()
        );
        for source in [
            "return {plus_menu={{label='x',action='connect',host='host',bin=''}}}",
            "return {plus_menu={{label='x',action='connect',host='host',bin='bad\\nprogram'}}}",
            "return {plus_menu={{label='x',action='new_tab',bin='/bin/sh'}}}",
            "return {plus_menu=42}",
            "return {plus_menu={{label='',action='new_tab'}}}",
            "return {plus_menu={{label='x',action='oops'}}}",
            "return {plus_menu={{label='x',action='connect',host='-bad'}}}",
            "return {plus_menu={{label='x',action='connect'}}}",
            "return {plus_menu={{label='x',action='new_tab',host='host'}}}",
        ] {
            assert!(Config::parse(source).is_err(), "{source}");
        }
        Ok(())
    }
    #[test]
    fn cursor_style_configuration() -> Result<()> {
        use ghostty_vt::terminal::CursorStyle;
        use ghostty_vt::{RenderState, Terminal, TerminalOptions};
        assert_eq!(Config::default().default_cursor_style, CursorStyle::Block);
        for name in ["block", "bar", "underline", "block_hollow"] {
            let config = Config::parse(&format!("return {{default_cursor_style='{name}'}}"))?;
            let mut terminal = Terminal::new(TerminalOptions::default())?;
            terminal.set_default_cursor_style(Some(config.default_cursor_style))?;
            let mut render = RenderState::new()?;
            let expected = render.update(&terminal)?.cursor_visual_style()?;
            assert_eq!(expected as i32, config.default_cursor_style as i32);
            terminal.vt_write(b"\x1b[6 q");
            terminal.set_default_cursor_style(Some(config.default_cursor_style))?;
            assert_eq!(
                render.update(&terminal)?.cursor_visual_style()?,
                ghostty_vt::render::CursorVisualStyle::Bar
            );
            terminal.vt_write(b"\x1b[0 q");
            assert_eq!(render.update(&terminal)?.cursor_visual_style()?, expected);
        }
        for value in ["'beam'", "''", "42", "false", "{}"] {
            assert!(Config::parse(&format!("return {{default_cursor_style={value}}}")).is_err());
        }
        Ok(())
    }

    #[test]
    fn scrollback_configuration() -> Result<()> {
        assert_eq!(Config::default().scrollback_mb, 16);
        for mb in [0, 16, 64] {
            assert_eq!(
                Config::parse(&format!("return {{scrollback_mb={mb}}}"))?.scrollback_mb,
                mb
            );
        }
        for value in ["-1", "65", "1.5", "0/0", "'32'"] {
            assert!(Config::parse(&format!("return {{scrollback_mb={value}}}")).is_err());
        }
        Ok(())
    }

    #[test]
    fn config_modules_resolve_beside_config() -> Result<()> {
        let dir = std::env::temp_dir().join(format!(
            "scytty-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("theme"))?;
        let result = (|| -> Result<()> {
            std::fs::write(dir.join("theme/init.lua"), "return {color='#ff0000'}")?;
            std::fs::write(
                dir.join("tabs.lua"),
                "local theme=require('theme'); return function(t) return {{text=t.title, color=theme.color}} end",
            )?;
            let config = Config::parse_at("return {format_tab=require('tabs')}", Some(&dir))?;
            let tabs = config.status_text("format_tab", "test", 0, true, false, None)?;
            assert_eq!(tabs[0], ("test".into(), Some(gpui::rgb(0xff0000))));
            Ok(())
        })();
        std::fs::remove_dir_all(dir)?;
        result
    }
    #[test]
    fn window_title_callback() -> Result<()> {
        assert_eq!(
            Config::default().window_title("work", "fish", false)?,
            "work - fish"
        );
        let config = Config::parse(
            "return {format_window_title=function(c) return (c.remote and 'SSH ' or '') .. c.title .. ' [' .. c.session .. ']' end}",
        )?;
        assert_eq!(
            config.window_title("work", "nvim", true)?,
            "SSH nvim [work]"
        );
        assert!(Config::parse("return {format_window_title=42}").is_err());
        let bad = Config::parse("return {format_window_title=function() return {} end}")?;
        assert!(bad.window_title("work", "fish", false).is_err());
        Ok(())
    }
    #[test]
    fn lua_controls_status_segments() -> Result<()> {
        let config = Config::parse(include_str!("../../../config.example.lua"))?;
        let right = config.status_text("right_status", "", 0, false, false, None)?;
        assert_eq!(right[0].0, "");
        Ok(())
    }
    #[test]
    fn program_status_context_is_per_tab_and_optional() -> Result<()> {
        let config = Config::parse(
            "return {format_tab=function(c) local r=c.program_status['build']; return (c.state or 'none') .. ':' .. (r and (r.app .. ':' .. r.message .. ':' .. r.progress) or '-') end}",
        )?;
        let mut tab = scytty_common::protocol::TabInfo::default();
        assert_eq!(
            config.status_text("format_tab", "", 0, false, false, Some(&tab))?[0].0,
            "none:-"
        );
        tab.state = Some("blocked".into());
        tab.program_status.insert(
            "build".into(),
            scytty_common::protocol::ProgramStatus {
                state: "blocked".into(),
                app: Some("cargo".into()),
                kind: Some("permission".into()),
                progress: Some(40),
                title: None,
                message: Some("Approve?".into()),
            },
        );
        assert_eq!(
            config.status_text("format_tab", "", 0, false, false, Some(&tab))?[0].0,
            "blocked:cargo:Approve?:40"
        );
        assert_eq!(
            config.status_text("format_tab", "", 0, false, false, None)?[0].0,
            "none:-"
        );
        Ok(())
    }
    #[test]
    fn progress_context() -> Result<()> {
        use scytty_common::protocol::{Progress, TabInfo};
        let config = Config::parse(
            "return {right_status=function(c) local p=c.progress; return p and (p.state .. ':' .. (p.percent or '?')) or 'none' end}",
        )?;
        let mut tab = TabInfo::default();
        assert_eq!(
            config.status_text("right_status", "", 0, false, false, Some(&tab))?[0].0,
            "none"
        );
        for percent in [Some(42), None] {
            tab.progress = Some(Progress {
                state: "normal".into(),
                percent,
            });
            assert_eq!(
                config.status_text("right_status", "", 0, false, false, Some(&tab))?[0].0,
                if percent.is_some() {
                    "normal:42"
                } else {
                    "normal:?"
                }
            );
        }
        Ok(())
    }
    #[test]
    fn custom_status_opacity() -> Result<()> {
        let config = Config::parse(
            "return {right_status = function(ctx) local age = 1000 - tonumber(ctx.user_vars.right_status_timestamp); local alpha = math.max(0, math.min(255, 255 - age)); return {{text='icon', color=string.format('#ffff00%02x', alpha)}} end}",
        )?;
        for (timestamp, alpha) in [(1000, 255), (872, 127), (745, 0), (700, 0)] {
            let vars = std::collections::BTreeMap::from([(
                "right_status_timestamp".into(),
                timestamp.to_string(),
            )]);
            let tab = scytty_common::protocol::TabInfo {
                mouse_shape: None,
                user_vars: vars,
                ..Default::default()
            };
            let right = config.status_text("right_status", "", 0, false, false, Some(&tab))?;
            assert_eq!(right[0].1.unwrap(), gpui::rgba(0xffff0000 | alpha as u32));
        }
        Ok(())
    }
    #[test]
    fn status_callbacks() -> Result<()> {
        let config = Config::parse(
            "return { left_status = function(c) return c.remote and 'ssh' or 'local' end, format_tab = function(t) return {{text=t.index .. ':' .. t.title, color='#ff0000'}} end }",
        )?;
        assert_eq!(
            config.status_text("left_status", "", 0, false, true, None)?[0].0,
            "ssh"
        );
        let text = config.status_text("format_tab", "nvim test", 1, true, false, None)?;
        assert_eq!(text[0], ("2:nvim test".into(), Some(gpui::rgb(0xff0000))));
        let defaults = Config::default();
        let tab = defaults.status_text("format_tab", "nvim /tmp/test.rs", 0, true, false, None)?;
        assert_eq!(tab[0].0, " nvim /tmp/test.rs ");
        assert_eq!(tab[0].1, Some(gpui::rgb(0xffffff)));
        let vars = std::collections::BTreeMap::from([
            ("right_status".into(), "building ".into()),
            ("right_status_timestamp".into(), "9999999999".into()),
        ]);
        let tab = scytty_common::protocol::TabInfo {
            mouse_shape: None,
            user_vars: vars,
            ..Default::default()
        };
        let right = defaults.status_text("right_status", "", 0, false, false, Some(&tab))?;
        assert_eq!(right[0].0, "");
        assert_eq!(
            defaults.status_text("left_status", "", 0, false, false, None)?[0].0,
            ""
        );
        assert!(Config::parse("return {format_tab=42}").is_err());
        let bad = Config::parse(
            "return {right_status=function() return {{text='x', color='oops'}} end}",
        )?;
        assert!(
            bad.status_text("right_status", "", 0, false, false, None)
                .is_err()
        );
        Ok(())
    }
    #[test]
    fn lua_config_and_validation() -> Result<()> {
        let config = Config::parse(
            "local size = 9 * 2; return {font_size = size, background_opacity = 0.5}",
        )?;
        assert_eq!(config.font_size, 18.0);
        assert_eq!(config.font_family, "monospace");
        assert_eq!(
            Config::parse("return {font_family='Menlo'}")?.font_family,
            "Menlo"
        );
        assert_eq!(config.icon_scale, 1.5);
        assert_eq!(Config::parse("return {icon_scale=2}")?.icon_scale, 2.0);
        assert_eq!(config.window_width, 960.0);
        for source in [
            "return {font_size=0}",
            "return {font_family='  '}",
            "return {font_family=42}",
            "return {icon_scale=0}",
            "return {icon_scale=4}",
            "return {icon_scale=0/0}",
            "return {background_opacity=2}",
            "return {font_size=0/0}",
            "return {unknown=1}",
            "return {font_size={}}",
            "return 42",
            "not lua",
        ] {
            assert!(Config::parse(source).is_err(), "{source}");
        }
        Ok(())
    }
}
