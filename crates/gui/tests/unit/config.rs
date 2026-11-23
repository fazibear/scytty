use super::*;

#[test]
fn font_arrays_configure_glyph_fallbacks() -> Result<()> {
    let config = Config::parse("return {font_family={'Menlo','FiraCode Nerd Font Mono'}}")?;
    assert_eq!(config.font_family, "Menlo");
    assert_eq!(
        config.font().fallbacks.unwrap().fallback_list(),
        &["FiraCode Nerd Font Mono"]
    );
    assert!(
        Config::parse("return {font_family='Menlo'}")?
            .font()
            .fallbacks
            .is_none()
    );
    for value in [
        "{}",
        "{'Menlo',false}",
        "{'Menlo',''}",
        "{[1]='Menlo',[3]='Other'}",
        "{name='Menlo'}",
    ] {
        assert!(Config::parse(&format!("return {{font_family={value}}}")).is_err());
    }
    Ok(())
}

#[test]
fn tab_bar_labels_are_configurable_and_validated() -> Result<()> {
    let defaults = Config::default();
    assert_eq!(
        (
            &*defaults.tab_left_indicator.text,
            &*defaults.tab_right_indicator.text,
            &*defaults.new_tab_button.text
        ),
        ("<", ">", "+")
    );
    assert!(defaults.new_tab_button.color.is_none());
    let config = Config::parse(
        "return {tab_left_indicator='←',tab_right_indicator='More →',new_tab_button='＋'}",
    )?;
    assert_eq!(config.tab_left_indicator.text, "←");
    assert_eq!(config.tab_right_indicator.text, "More →");
    assert_eq!(config.new_tab_button.text, "＋");
    for key in [
        "tab_left_indicator",
        "tab_right_indicator",
        "new_tab_button",
    ] {
        let colored = Config::parse(&format!(
            "return {{{key}={{text='← More',color='#a6e3a1'}}}}"
        ))?;
        let label = match key {
            "tab_left_indicator" => colored.tab_left_indicator,
            "tab_right_indicator" => colored.tab_right_indicator,
            _ => colored.new_tab_button,
        };
        assert_eq!(label.text, "← More");
        assert_eq!(label.color, Some(gpui::rgb(0xa6e3a1)));
        for value in [
            "false",
            "42",
            "''",
            "'   '",
            "'a\\nb'",
            "'\\027'",
            "{}",
            "{text=42}",
            "{text='X',color=false}",
            "{text='X',color='oops'}",
            "{text='X',colour='#ffffff'}",
        ] {
            assert!(
                Config::parse(&format!("return {{{key}={value}}}")).is_err(),
                "{key}={value}"
            );
        }
    }
    Ok(())
}

#[test]
fn configurable_key_bindings_override_defaults_and_pass_through() -> Result<()> {
    use crate::key_bindings::Action;
    let defaults = Config::default();
    for (key, action) in [
        ("t", Action::NewTab),
        ("f", Action::Find),
        ("left", Action::PreviousTab),
        ("right", Action::NextTab),
        ("c", Action::Copy),
        ("v", Action::Paste),
        ("up", Action::PreviousPrompt),
        ("down", Action::NextPrompt),
    ] {
        let modifier = if cfg!(target_os = "windows") {
            if matches!(
                action,
                Action::Find
                    | Action::Copy
                    | Action::Paste
                    | Action::PreviousPrompt
                    | Action::NextPrompt
            ) {
                "ctrl-shift"
            } else {
                "ctrl"
            }
        } else {
            "super"
        };
        assert_eq!(
            defaults.key_actions(&gpui::Keystroke::parse(&format!("{modifier}-{key}"))?),
            vec![action]
        );
    }
    for key in [
        "ctrl-t",
        "ctrl-left",
        "ctrl-right",
        "ctrl-+",
        "ctrl--",
        "ctrl-0",
        "ctrl-shift-f",
        "ctrl-shift-c",
        "ctrl-shift-v",
        "ctrl-shift-up",
        "ctrl-shift-down",
    ] {
        assert_eq!(
            defaults
                .key_actions(&gpui::Keystroke::parse(key)?)
                .is_empty(),
            !cfg!(target_os = "windows"),
            "{key}"
        );
    }
    if cfg!(target_os = "windows") {
        for key in [
            "ctrl-c",
            "ctrl-v",
            "ctrl-f",
            "ctrl-up",
            "ctrl-down",
            "super-c",
            "super-v",
            "super-f",
            "super-t",
            "super-0",
        ] {
            assert!(
                defaults
                    .key_actions(&gpui::Keystroke::parse(key)?)
                    .is_empty(),
                "{key}"
            );
        }
    }
    let custom = Config::parse(
        "return {key_bindings={
        {keys={key.CTRL,key.T},actions={}},
        {keys={key.SUPER,key.T},actions={}},
        {keys={key.CTRL,key.SHIFT,key.N},actions={'new_tab'}},
        {keys={key.SUPER,key.R},actions={'reload_config'}},
        {keys={key.SUPER,key.F},actions={'zoom_in','find'}}
    }}",
    )?;
    let mut new_tab = gpui::Keystroke::parse("super-t")?;
    new_tab.key_char = Some("t".into());
    assert!(custom.key_actions(&new_tab).is_empty());
    assert!(
        custom
            .key_actions(&gpui::Keystroke::parse("ctrl-t")?)
            .is_empty()
    );
    let mut shortcut = gpui::Keystroke::parse("ctrl-shift-n")?;
    shortcut.key_char = Some("N".into());
    assert_eq!(custom.key_actions(&shortcut), vec![Action::NewTab]);
    shortcut.modifiers.alt = true;
    assert!(custom.key_actions(&shortcut).is_empty());
    assert_eq!(
        custom.key_actions(&gpui::Keystroke::parse("super-r")?),
        vec![Action::ReloadConfig]
    );
    assert_eq!(
        custom.key_actions(&gpui::Keystroke::parse("super-f")?),
        vec![Action::ZoomIn, Action::Find]
    );
    assert_eq!(
        Config::parse("return {}")?.key_actions(&new_tab),
        if cfg!(target_os = "windows") {
            vec![]
        } else {
            vec![Action::NewTab]
        }
    );
    Ok(())
}

#[test]
fn key_binding_arrays_and_constants_are_validated() -> Result<()> {
    use crate::key_bindings::Action;
    for (constant, stroke) in [
        ("T", "t"),
        ("DIGIT0", "0"),
        ("F35", "f35"),
        ("MINUS", "-"),
        ("PLUS", "+"),
        ("PAGE_UP", "pageup"),
        ("SPACE", "space"),
        ("BACKSLASH", "\\"),
    ] {
        let config = Config::parse(&format!(
            "return {{key_bindings={{{{keys={{key.{constant},key.SUPER}},actions={{'copy'}}}}}}}}"
        ))?;
        assert_eq!(
            config.key_actions(&gpui::Keystroke::parse(&format!("super-{stroke}"))?),
            vec![Action::Copy]
        );
    }
    for body in [
        "false",
        "{['super-t']='new_tab'}",
        "{false}",
        "{[2]={keys={key.T},actions={}}}",
        "{{keys={key.SUPER,key.T},action={'copy'}}}",
        "{{keys={key.SUPER,key.T}}}",
        "{{actions={'copy'}}}",
        "{{keys={},actions={'copy'}}}",
        "{{keys={key.SUPER},actions={'copy'}}}",
        "{{keys={key.SUPER,key.SUPER,key.T},actions={'copy'}}}",
        "{{keys={key.T,key.N},actions={'copy'}}}",
        "{{keys={key.SUPER,key.UNKNOWN},actions={'copy'}}}",
        "{{keys={[1]=key.SUPER,[3]=key.T},actions={'copy'}}}",
        "{{keys={key.SUPER,key.T},actions=false}}",
        "{{keys={key.SUPER,key.T},actions={false}}}",
        "{{keys={key.SUPER,key.T},actions={'unknown'}}}",
        "{{keys={key.SUPER,key.T},actions={[2]='copy'}}}",
        "{{keys={key.SUPER,key.T},actions={'copy'}},{keys={key.T,key.SUPER},actions={'paste'}}}",
    ] {
        assert!(
            Config::parse(&format!("return {{key_bindings={body}}}")).is_err(),
            "{body}"
        );
    }
    Ok(())
}

#[test]
fn key_binding_actions_run_in_order_and_stop_on_error() -> Result<()> {
    use crate::key_bindings::{Action, run_actions};
    let actions = [Action::ZoomIn, Action::ZoomOut, Action::Find];
    let mut seen = Vec::new();
    run_actions(&actions, |action| {
        seen.push(action);
        Ok(())
    })?;
    assert_eq!(seen, actions);
    seen.clear();
    assert!(
        run_actions(&actions, |action| {
            seen.push(action);
            if action == Action::ZoomOut {
                bail!("failed")
            }
            Ok(())
        })
        .is_err()
    );
    assert_eq!(seen, actions[..2]);
    run_actions(&[], |_| panic!("empty actions must pass through"))?;
    Ok(())
}
#[test]
fn menu_separators_are_not_selectable() -> Result<()> {
    let config = Config::parse(
        "return {plus_menu={{separator=true},{label='New',action='new_tab'},{separator=true},{separator=true},{label='Close',action='close_tab'},{separator=true}}}",
    )?;
    assert_eq!(config.plus_menu[0].action, MenuAction::Separator);
    assert_eq!(config.next_menu_item(5, true), Some(1));
    assert_eq!(config.next_menu_item(1, true), Some(4));
    assert_eq!(config.next_menu_item(4, true), Some(1));
    assert_eq!(config.next_menu_item(1, false), Some(4));
    assert_eq!(config.next_menu_item(4, false), Some(1));
    assert_eq!(
        Config::parse("return {plus_menu={}}")?.next_menu_item(0, true),
        None
    );
    let separators = Config::parse("return {plus_menu={{separator=true},{separator=true}}}")?;
    assert_eq!(separators.next_menu_item(0, true), None);
    assert_eq!(separators.next_menu_item(0, false), None);
    for entry in [
        "{separator=false}",
        "{separator='true'}",
        "{separator=true,label='No'}",
        "{separator=true,action='new_tab'}",
    ] {
        assert!(Config::parse(&format!("return {{plus_menu={{{entry}}}}}")).is_err());
    }
    Ok(())
}

#[test]
fn close_tab_menu_entry() -> Result<()> {
    assert!(
        Config::default()
            .plus_menu
            .iter()
            .any(|entry| entry.action == MenuAction::CloseTab)
    );
    let config = Config::parse("return {plus_menu={{label='Close tab',action='close_tab'}}}")?;
    assert_eq!(config.plus_menu[0].action, MenuAction::CloseTab);
    assert!(
        Config::parse("return {plus_menu={{label='Close',action='close_tab',host='host'}}}")
            .is_err()
    );
    Ok(())
}

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
        Config::parse("return {plus_menu={{label='Reload',action='reload_config',host='host'}}}")
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
    let config = Config::parse(include_str!("../../../../config.example.lua"))?;
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
    let bad =
        Config::parse("return {right_status=function() return {{text='x', color='oops'}} end}")?;
    assert!(
        bad.status_text("right_status", "", 0, false, false, None)
            .is_err()
    );
    Ok(())
}
#[test]
fn lua_config_and_validation() -> Result<()> {
    let config =
        Config::parse("local size = 9 * 2; return {font_size = size, background_opacity = 0.5}")?;
    assert_eq!(config.font_size, 18.0);
    assert_eq!(config.font_family, "monospace");
    assert_eq!(
        Config::parse("return {font_family='Menlo'}")?.font_family,
        "Menlo"
    );
    assert_eq!(config.icon_scale, 1.0);
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
