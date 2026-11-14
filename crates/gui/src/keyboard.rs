use anyhow::Result;
use ghostty_vt::{Terminal, key};
use gpui::{Keystroke, Modifiers, ModifiersChangedEvent};

#[derive(Default)]
pub(super) struct Keyboard {
    pressed: Vec<(u64, Keystroke)>,
    modifiers: Modifiers,
    pub(super) caps_lock: bool,
}

impl Keyboard {
    pub(super) fn sync(&mut self, modifiers: Modifiers, caps_lock: bool) {
        self.modifiers = modifiers;
        self.caps_lock = caps_lock;
    }

    pub(super) fn remember(&mut self, id: u64, stroke: &Keystroke) {
        if !self.pressed.iter().any(|(_, held)| same_key(held, stroke)) {
            self.pressed.push((id, stroke.clone()));
        }
    }

    pub(super) fn shift(&self) -> bool {
        self.modifiers.shift
    }

    pub(super) fn target(&self, stroke: &Keystroke) -> Option<u64> {
        self.pressed
            .iter()
            .find(|(_, held)| same_key(held, stroke))
            .map(|(id, _)| *id)
    }

    pub(super) fn release(&mut self, stroke: &Keystroke) -> Option<(u64, Keystroke)> {
        let index = self
            .pressed
            .iter()
            .position(|(_, held)| same_key(held, stroke))?;
        let (id, mut held) = self.pressed.remove(index);
        held.modifiers = stroke.modifiers;
        Some((id, held))
    }

    pub(super) fn clear(&mut self) -> Vec<(u64, Keystroke)> {
        self.modifiers = Modifiers::default();
        std::mem::take(&mut self.pressed)
    }

    pub(super) fn modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
    ) -> Vec<(Keystroke, key::Action)> {
        let old = std::mem::replace(&mut self.modifiers, event.modifiers);
        let caps_changed = self.caps_lock != event.capslock.on;
        self.caps_lock = event.capslock.on;
        let mut changes: Vec<_> = [
            ("shift", old.shift, event.modifiers.shift),
            ("control", old.control, event.modifiers.control),
            ("alt", old.alt, event.modifiers.alt),
            ("super", old.platform, event.modifiers.platform),
            ("fn", old.function, event.modifiers.function),
        ]
        .into_iter()
        .filter(|(_, before, after)| before != after)
        .map(|(name, _, pressed)| {
            (
                Keystroke {
                    key: name.into(),
                    modifiers: event.modifiers,
                    key_char: None,
                },
                if pressed {
                    key::Action::Press
                } else {
                    key::Action::Release
                },
            )
        })
        .collect();
        if caps_changed {
            // shortcut: GPUI exposes only Caps Lock toggles; use physical releases when its API provides them.
            let stroke = Keystroke {
                key: "capslock".into(),
                key_char: None,
                modifiers: event.modifiers,
            };
            changes.push((stroke.clone(), key::Action::Press));
            changes.push((stroke, key::Action::Release));
        }
        changes
    }
}

fn same_key(a: &Keystroke, b: &Keystroke) -> bool {
    a.key == b.key || terminal_key(&a.key).is_some_and(|key| Some(key) == terminal_key(&b.key))
}

pub(super) fn encode(
    terminal: &Terminal,
    encoder: &mut key::Encoder,
    stroke: &Keystroke,
    action: key::Action,
    composing: bool,
    caps_lock: bool,
) -> Result<Option<Vec<u8>>> {
    let m = stroke.modifiers;
    let logical = terminal_key(&stroke.key);
    let codepoint = if stroke.key == "space" {
        Some(' ')
    } else {
        single_char(&stroke.key).or_else(|| {
            logical
                .is_none()
                .then(|| stroke.key_char.as_deref().and_then(single_char))
                .flatten()
                .filter(|ch| !ch.is_control())
        })
    };
    let kitty = terminal.kitty_keyboard_flags()?;
    // Ghostty's key enum ends at F25; Kitty defines F1–F35 at 57364–57398.
    let extended_function = (!kitty.is_empty())
        .then(|| {
            stroke
                .key
                .strip_prefix('f')?
                .parse::<u32>()
                .ok()
                .filter(|n| (26..=35).contains(n))
                .and_then(|n| char::from_u32(57364 + n - 1))
        })
        .flatten();
    if logical.is_none() && codepoint.is_none() && extended_function.is_none() {
        return Ok(None);
    }
    // Leave dead keys and IME composition to the native input handler.
    if action != key::Action::Release
        && codepoint.is_some()
        && (composing
            || (!m.control
                && !m.platform
                && stroke.key != "space"
                && stroke.key_char.as_deref().is_none_or(str::is_empty)))
    {
        return Ok(None);
    }
    let mut event = key::Event::new()?;
    let mut mods = key::Mods::empty();
    if m.shift {
        mods |= key::Mods::SHIFT;
    }
    if m.control {
        mods |= key::Mods::CTRL;
    }
    if m.alt {
        mods |= key::Mods::ALT;
    }
    if m.platform {
        mods |= key::Mods::SUPER;
    }
    if caps_lock {
        mods |= key::Mods::CAPS_LOCK;
    }
    event
        .set_key(logical.unwrap_or(key::Key::Unidentified))
        .set_action(action)
        .set_mods(mods)
        .set_composing(composing);
    if let Some(ch) = codepoint.or(extended_function) {
        event.set_unshifted_codepoint(ch);
    }
    let text = stroke.key_char.clone().or_else(|| {
        codepoint.map(|ch| if m.shift { ch.to_ascii_uppercase() } else { ch }.to_string())
    });
    if codepoint.is_some() {
        event.set_utf8(text.clone());
    }
    let mut bytes = Vec::new();
    encoder
        .set_options_from_terminal(terminal)
        .set_macos_option_as_alt(key::OptionAsAlt::True)
        .encode_to_vec(&event, &mut bytes)?;
    if bytes.is_empty() {
        return Ok(None);
    }
    // Native text insertion preserves dead-key/layout handling; protocol sequences must bypass it.
    if action != key::Action::Release
        && !m.control
        && !m.alt
        && !m.platform
        && kitty.is_empty()
        && text.as_deref().is_some_and(|text| bytes == text.as_bytes())
    {
        return Ok(None);
    }
    Ok(Some(bytes))
}

fn single_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    chars.next().is_none().then_some(ch)
}

pub(super) fn terminal_key(k: &str) -> Option<key::Key> {
    use key::Key as K;
    // shortcut: GPUI omits physical key codes, keypad identity and modifier sides; use its logical keys until GPUI exposes them.
    if let Some(ch) = single_char(k) {
        let ch = ch.to_ascii_lowercase();
        if ch.is_ascii_lowercase() {
            return K::try_from(K::A as i32 + i32::from(ch as u8 - b'a')).ok();
        }
        if ch.is_ascii_digit() {
            return K::try_from(K::Digit0 as i32 + i32::from(ch as u8 - b'0')).ok();
        }
    }
    if let Some(n) = k.strip_prefix('f').and_then(|n| n.parse::<i32>().ok())
        && (1..=25).contains(&n)
    {
        return K::try_from(K::F1 as i32 + n - 1).ok();
    }
    Some(match k {
        "enter" => K::Enter,
        "backspace" => K::Backspace,
        "tab" | "iso_left_tab" => K::Tab,
        "escape" => K::Escape,
        "up" => K::ArrowUp,
        "down" => K::ArrowDown,
        "left" => K::ArrowLeft,
        "right" => K::ArrowRight,
        "home" => K::Home,
        "end" => K::End,
        "pageup" | "prior" => K::PageUp,
        "pagedown" | "next" => K::PageDown,
        "insert" => K::Insert,
        "delete" => K::Delete,
        "space" | " " => K::Space,
        "`" => K::Backquote,
        "\\" => K::Backslash,
        "[" => K::BracketLeft,
        "]" => K::BracketRight,
        "," => K::Comma,
        "=" => K::Equal,
        "-" => K::Minus,
        "." => K::Period,
        "'" => K::Quote,
        ";" => K::Semicolon,
        "/" => K::Slash,
        "shift" => K::ShiftLeft,
        "control" => K::ControlLeft,
        "alt" => K::AltLeft,
        "super" => K::MetaLeft,
        "fn" => K::Fn,
        "capslock" | "caps_lock" => K::CapsLock,
        "numlock" | "num_lock" => K::NumLock,
        "scrolllock" | "scroll_lock" => K::ScrollLock,
        "printscreen" | "print" => K::PrintScreen,
        "pause" => K::Pause,
        "menu" => K::ContextMenu,
        "add" => K::NumpadAdd,
        "subtract" => K::NumpadSubtract,
        "multiply" => K::NumpadMultiply,
        "divide" => K::NumpadDivide,
        "decimal" => K::NumpadDecimal,
        "separator" => K::NumpadSeparator,
        "equal" => K::NumpadEqual,
        "begin" => K::NumpadBegin,
        "back" | "xf86back" => K::BrowserBack,
        "forward" | "xf86forward" => K::BrowserForward,
        "copy" => K::Copy,
        "cut" => K::Cut,
        "paste" => K::Paste,
        "xf86audioplay" | "xf86audiopause" => K::MediaPlayPause,
        "xf86audiostop" => K::MediaStop,
        "xf86audionext" => K::MediaTrackNext,
        "xf86audioprev" => K::MediaTrackPrevious,
        "xf86audioraisevolume" => K::AudioVolumeUp,
        "xf86audiolowervolume" => K::AudioVolumeDown,
        "xf86audiomute" => K::AudioVolumeMute,
        "help" => K::Help,
        "clear" => K::NumpadClear,
        "xf86refresh" | "xf86reload" => K::BrowserRefresh,
        "xf86stop" => K::BrowserStop,
        "xf86search" => K::BrowserSearch,
        "xf86homepage" => K::BrowserHome,
        "xf86favorites" => K::BrowserFavorites,
        "xf86mail" => K::LaunchMail,
        "xf86launch1" => K::LaunchApp1,
        "xf86launch2" => K::LaunchApp2,
        "xf86audiomedia" => K::MediaSelect,
        "xf86eject" => K::Eject,
        "xf86poweroff" => K::Power,
        "xf86sleep" => K::Sleep,
        "xf86wakeup" => K::WakeUp,
        "henkan" | "henkan_mode" => K::Convert,
        "muhenkan" => K::NonConvert,
        "kana_lock" | "kana_shift" => K::KanaMode,
        _ => return None,
    })
}

#[cfg(test)]
#[path = "../tests/unit/keyboard.rs"]
mod tests;
