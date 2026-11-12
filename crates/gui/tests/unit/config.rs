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
