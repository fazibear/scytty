use crate::platform;
use anyhow::{Context, Result, bail};
use gpui::Keystroke;
use mlua::{Lua, Table, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    NewTab,
    NewWindow,
    CloseTab,
    ReloadConfig,
    Copy,
    Paste,
    Find,
    PreviousTab,
    NextTab,
    PreviousPrompt,
    NextPrompt,
    ZoomIn,
    ZoomOut,
    ResetZoom,
}
impl Action {
    pub(super) fn parse(name: &str) -> Result<Self> {
        Ok(match name {
            "new_tab" => Self::NewTab,
            "new_window" => Self::NewWindow,
            "close_tab" => Self::CloseTab,
            "reload_config" => Self::ReloadConfig,
            "copy" => Self::Copy,
            "paste" => Self::Paste,
            "find" => Self::Find,
            "previous_tab" => Self::PreviousTab,
            "next_tab" => Self::NextTab,
            "previous_prompt" => Self::PreviousPrompt,
            "next_prompt" => Self::NextPrompt,
            "zoom_in" => Self::ZoomIn,
            "zoom_out" => Self::ZoomOut,
            "reset_zoom" => Self::ResetZoom,
            _ => bail!("unknown key binding action: {name}"),
        })
    }
    pub(super) fn font_size(self, current: f32, initial: f32) -> Option<f32> {
        match self {
            Self::ZoomIn => Some((current + 1.).clamp(6., 96.)),
            Self::ZoomOut => Some((current - 1.).clamp(6., 96.)),
            Self::ResetZoom => Some(initial),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(super) struct Binding {
    pub keys: Keystroke,
    pub actions: Vec<Action>,
}

pub(super) fn install(lua: &Lua) -> Result<()> {
    let keys = lua.create_table()?;
    for ch in 'A'..='Z' {
        keys.set(ch.to_string(), ch.to_ascii_lowercase().to_string())?;
    }
    for n in 0..=9 {
        keys.set(format!("DIGIT{n}"), n.to_string())?;
    }
    for n in 1..=35 {
        keys.set(format!("F{n}"), format!("f{n}"))?;
    }
    for (name, value) in [
        ("SUPER", "super"),
        ("CTRL", "ctrl"),
        ("ALT", "alt"),
        ("SHIFT", "shift"),
        ("FN", "fn"),
        ("ENTER", "enter"),
        ("ESCAPE", "escape"),
        ("TAB", "tab"),
        ("SPACE", "space"),
        ("BACKSPACE", "backspace"),
        ("DELETE", "delete"),
        ("INSERT", "insert"),
        ("HOME", "home"),
        ("END", "end"),
        ("PAGE_UP", "pageup"),
        ("PAGE_DOWN", "pagedown"),
        ("UP", "up"),
        ("DOWN", "down"),
        ("LEFT", "left"),
        ("RIGHT", "right"),
        ("PLUS", "+"),
        ("MINUS", "-"),
        ("EQUAL", "="),
        ("BACKQUOTE", "`"),
        ("BACKSLASH", "\\"),
        ("BRACKET_LEFT", "["),
        ("BRACKET_RIGHT", "]"),
        ("COMMA", ","),
        ("PERIOD", "."),
        ("SEMICOLON", ";"),
        ("QUOTE", "'"),
        ("SLASH", "/"),
    ] {
        keys.set(name, value)?;
    }
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(|_, (_, name): (Table, String)| -> mlua::Result<Value> {
            Err(mlua::Error::RuntimeError(format!(
                "unknown key constant: key.{name}"
            )))
        })?,
    )?;
    keys.set_metatable(Some(meta))?;
    lua.globals().set("key", keys)?;
    Ok(())
}

fn array(value: Value) -> Result<Vec<Value>> {
    let Value::Table(table) = value else {
        bail!("must be an array")
    };
    let len = table.raw_len();
    let mut count = 0;
    for pair in table.clone().pairs::<usize, Value>() {
        let (index, _) = pair?;
        if index == 0 || index > len {
            bail!("must be a contiguous array")
        }
        count += 1;
    }
    if count != len {
        bail!("must be a contiguous array")
    }
    Ok(table.sequence_values().collect::<mlua::Result<_>>()?)
}

pub(super) fn parse(value: Value) -> Result<Vec<Binding>> {
    let mut bindings: Vec<Binding> = Vec::new();
    for (index, value) in array(value)
        .context("key_bindings")?
        .into_iter()
        .enumerate()
    {
        let Value::Table(entry) = value else {
            bail!("key binding {} must be a table", index + 1)
        };
        for pair in entry.clone().pairs::<String, Value>() {
            let (field, _) = pair?;
            if !matches!(field.as_str(), "keys" | "actions") {
                bail!("unknown key binding field: {field}")
            }
        }
        let mut parts = Vec::new();
        for value in array(entry.raw_get("keys")?).context("binding keys")? {
            let Value::String(value) = value else {
                bail!("binding keys must contain key constants")
            };
            let part = value.to_str()?.to_owned();
            if part.is_empty() || (part.contains('-') && part != "-") || parts.contains(&part) {
                bail!("invalid or duplicate binding key: {part}");
            }
            parts.push(part);
        }
        if parts
            .iter()
            .filter(|part| !matches!(part.as_str(), "super" | "ctrl" | "alt" | "shift" | "fn"))
            .count()
            != 1
        {
            bail!("binding keys must contain exactly one non-modifier key");
        }
        // GPUI's parser expects a literal minus at the end of the chord.
        parts.sort_by_key(|part| {
            !matches!(part.as_str(), "super" | "ctrl" | "alt" | "shift" | "fn")
        });
        let name = parts.join("-");
        let keys =
            Keystroke::parse(&name).with_context(|| format!("invalid key binding: {name}"))?;
        if keys.key.is_empty()
            || keys.key.chars().any(char::is_whitespace)
            || keys.key_char.is_some()
            || matches!(
                keys.key.as_str(),
                "shift" | "control" | "alt" | "platform" | "function"
            )
        {
            bail!("binding keys must describe one non-modifier keystroke");
        }
        if bindings
            .iter()
            .any(|binding| binding.keys.key == keys.key && binding.keys.modifiers == keys.modifiers)
        {
            bail!("duplicate key binding: {name}");
        }
        let actions = array(entry.raw_get("actions")?)
            .context("binding actions")?
            .into_iter()
            .map(|value| {
                let Value::String(name) = value else {
                    bail!("binding actions must contain action strings")
                };
                Action::parse(&name.to_str()?)
            })
            .collect::<Result<_>>()?;
        bindings.push(Binding { keys, actions });
    }
    Ok(bindings)
}

pub(super) fn run_actions(
    actions: &[Action],
    mut run: impl FnMut(Action) -> Result<()>,
) -> Result<()> {
    for &action in actions {
        run(action)?;
    }
    Ok(())
}

pub(super) fn default_action(stroke: &Keystroke) -> Option<Action> {
    let m = stroke.modifiers;
    if !platform::command_modifier(m) {
        return None;
    }
    Some(match stroke.key.as_str() {
        "+" | "=" if !m.alt => Action::ZoomIn,
        "-" | "_" if !m.alt => Action::ZoomOut,
        "0" if !m.alt => Action::ResetZoom,
        "f" if platform::clipboard_modifier(m) => Action::Find,
        "c" if platform::clipboard_modifier(m) => Action::Copy,
        "v" if platform::clipboard_modifier(m) => Action::Paste,
        "up" if platform::clipboard_modifier(m) => Action::PreviousPrompt,
        "down" if platform::clipboard_modifier(m) => Action::NextPrompt,
        "t" => Action::NewTab,
        "left" => Action::PreviousTab,
        "right" => Action::NextTab,
        _ => return None,
    })
}
