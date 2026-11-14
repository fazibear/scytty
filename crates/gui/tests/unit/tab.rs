use super::*;
use crate::{keyboard::terminal_key, selection_point};
use ghostty_vt::TerminalOptions;
use ghostty_vt::selection::Selection;
use ghostty_vt::terminal::{Point as TerminalPoint, PointCoordinate};
use gpui::{point, px, size};
#[test]
fn full_styles_and_resets_survive_snapshots() -> Result<()> {
    let mut tab = Tab::new(Terminal::new(TerminalOptions::default())?)?;
    tab.terminal
        .vt_write(b"\x1b[1;2;3;5;53mA \x1b[22;23;25;55mB\x1b[1;3;5;53;8mC\x1b[0mD");
    tab.snapshot()?;
    let first = &tab.cells[0];
    assert!(first.bold && first.italic && first.blink && first.overline);
    let blank = &tab.cells[1];
    assert!(blank.blink && blank.overline);
    let plain = &tab.cells[2];
    assert!(!plain.bold && !plain.italic && !plain.blink && !plain.overline);
    assert!(first.fg.r < plain.fg.r);
    let hidden = &tab.cells[3];
    assert!(hidden.text.is_empty() && !hidden.blink && !hidden.overline);
    let reset = &tab.cells[4];
    assert!(!reset.bold && !reset.italic && !reset.blink && !reset.overline);
    let mut bytes = Vec::new();
    ghostty_vt::snapshot::encode_to(&tab.terminal, &mut bytes)?;
    let mut restored = Tab::new(ghostty_vt::snapshot::Decoder::from_slice(&bytes).decode()?)?;
    restored.snapshot()?;
    assert!(restored.cells == tab.cells);
    Ok(())
}

#[test]
fn kitty_animation_clock_advances_frames_and_stops_redrawing() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    resize_terminal(&mut terminal, 80, 24, 10, 20)?;
    let mut tab = Tab::new(terminal)?;
    tab.terminal.vt_write(b"\x1b_Ga=T,q=2,f=24,s=1,v=1,i=1,c=1,r=1;/wAA\x1b\\\x1b_Ga=f,q=2,f=24,s=1,v=1,i=1,z=75;AAD/\x1b\\\x1b_Ga=a,q=2,i=1,r=1,z=50\x1b\\\x1b_Ga=a,q=2,i=1,s=3,v=1\x1b\\");
    assert!(tab.snapshot_at(0)?);
    assert_eq!(tab.animation_deadline, Some(50));
    let red = tab.image_data[0].image_generation;
    tab.snapshot_at(49)?;
    assert_eq!(tab.image_data[0].image_generation, red);
    // Synchronized redraw must not commit a partially updated animation.
    tab.terminal.vt_write(b"\x1b[?2026h");
    assert!(!tab.snapshot_at(50)?);
    assert_eq!(tab.image_data[0].image_generation, red);
    tab.terminal.vt_write(b"\x1b[?2026l");
    tab.snapshot_at(50)?;
    assert_eq!(tab.animation_deadline, Some(125));
    assert_ne!(tab.image_data[0].image_generation, red);
    assert_eq!(
        tab.terminal.kitty_graphics()?.image(1).unwrap().data()?,
        &[0, 0, 255, 255]
    );
    tab.snapshot_at(125)?;
    assert_eq!(tab.animation_deadline, Some(175));
    assert_eq!(
        tab.terminal.kitty_graphics()?.image(1).unwrap().data()?,
        &[255, 0, 0, 255]
    );
    tab.terminal.vt_write(b"\x1b_Ga=a,q=2,i=1,s=1\x1b\\");
    tab.snapshot_at(200)?;
    assert!(tab.animation_deadline.is_none());
    // Client-driven frame selection works while playback is stopped.
    tab.terminal.vt_write(b"\x1b_Ga=a,q=2,i=1,c=2\x1b\\");
    tab.snapshot_at(250)?;
    assert!(tab.animation_deadline.is_none());
    assert_eq!(
        tab.terminal.kitty_graphics()?.image(1).unwrap().data()?,
        &[0, 0, 255, 255]
    );
    tab.terminal
        .vt_write(b"\x1b_Ga=a,q=2,i=1,c=1,s=3,v=2\x1b\\");
    tab.snapshot_at(300)?;
    tab.snapshot_at(350)?;
    tab.snapshot_at(425)?;
    assert!(tab.animation_deadline.is_none());
    tab.terminal.vt_write(b"\x1b_Ga=d,d=A,q=2\x1b\\");
    tab.snapshot_at(500)?;
    assert!(tab.image_data.is_empty());
    assert!(tab.animation_deadline.is_none());
    Ok(())
}

#[test]
fn kitty_loading_animation_resumes_when_another_frame_arrives() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    resize_terminal(&mut terminal, 80, 24, 10, 20)?;
    let mut tab = Tab::new(terminal)?;
    tab.terminal.vt_write(
        b"\x1b_Ga=T,q=2,f=24,s=1,v=1,i=1,c=1,r=1;/wAA\x1b\\\x1b_Ga=a,q=2,i=1,r=1,z=50,s=2\x1b\\",
    );
    tab.snapshot_at(0)?;
    tab.snapshot_at(50)?;
    assert!(tab.animation_deadline.is_none());
    tab.terminal
        .vt_write(b"\x1b_Ga=f,q=2,f=24,s=1,v=1,i=1,z=75;AAD/\x1b\\");
    tab.snapshot_at(100)?;
    assert_eq!(tab.animation_deadline, Some(150));
    tab.snapshot_at(150)?;
    assert_eq!(
        tab.terminal.kitty_graphics()?.image(1).unwrap().data()?,
        &[0, 0, 255, 255]
    );
    tab.snapshot_at(225)?;
    assert!(tab.animation_deadline.is_none());
    Ok(())
}

#[test]
fn mouse_buttons_motion_and_protocols() -> Result<()> {
    use ghostty_vt::mouse::{Action, Button, Event, Position};
    let mut tab = Tab::new(Terminal::new(TerminalOptions::default())?)?;
    let cell = size(px(10.), px(20.));
    let mut event = Event::new()?;
    event
        .set_action(Action::Press)
        .set_button(Some(Button::Left))
        .set_position(Position { x: 15., y: 25. });
    assert!(tab.mouse(&event, cell, true)?.is_empty());
    tab.terminal.vt_write(b"\x1b[?1000h\x1b[?1006h");
    assert_eq!(tab.mouse(&event, cell, true)?, b"\x1b[<0;2;2M");
    event.set_action(Action::Release);
    assert_eq!(tab.mouse(&event, cell, false)?, b"\x1b[<0;2;2m");
    event.set_action(Action::Motion);
    assert!(tab.mouse(&event, cell, true)?.is_empty());
    tab.terminal.vt_write(b"\x1b[?1002h");
    event.set_position(Position { x: 25., y: 25. });
    assert_eq!(tab.mouse(&event, cell, true)?, b"\x1b[<32;3;2M");
    event
        .set_position(Position { x: 35., y: 25. })
        .set_button(None);
    assert!(tab.mouse(&event, cell, false)?.is_empty());
    tab.terminal.vt_write(b"\x1b[?1003h");
    assert_eq!(tab.mouse(&event, cell, false)?, b"\x1b[<35;4;2M");
    event
        .set_action(Action::Press)
        .set_button(Some(Button::Right))
        .set_mods(key::Mods::CTRL | key::Mods::ALT);
    assert_eq!(tab.mouse(&event, cell, true)?, b"\x1b[<26;4;2M");
    event
        .set_button(Some(Button::Middle))
        .set_mods(key::Mods::empty());
    assert_eq!(tab.mouse(&event, cell, true)?, b"\x1b[<1;4;2M");
    event.set_button(Some(Button::Six));
    assert_eq!(tab.mouse(&event, cell, false)?, b"\x1b[<66;4;2M");
    event.set_button(Some(Button::Seven));
    assert_eq!(tab.mouse(&event, cell, false)?, b"\x1b[<67;4;2M");
    tab.terminal.vt_write(b"\x1b[?1006l");
    event
        .set_button(Some(Button::Left))
        .set_position(Position { x: 15., y: 25. });
    assert_eq!(tab.mouse(&event, cell, true)?, b"\x1b[M\x20\x22\x22");
    tab.terminal.vt_write(b"\x1b[?1005h");
    assert_eq!(tab.mouse(&event, cell, true)?, b"\x1b[M\x20\x22\x22");
    tab.terminal.vt_write(b"\x1b[?1005l\x1b[?1015h");
    assert_eq!(tab.mouse(&event, cell, true)?, b"\x1b[32;2;2M");
    tab.terminal.vt_write(b"\x1b[?1015l\x1b[?1016h");
    assert_eq!(tab.mouse(&event, cell, true)?, b"\x1b[<0;15;25M");
    Ok(())
}

#[test]
fn wheel_reports_to_apps_and_shift_keeps_local_scrolling() -> Result<()> {
    let mut tab = Tab::new(Terminal::new(TerminalOptions::default())?)?;
    tab.size = (80, 24, 10, 20);
    let position = ghostty_vt::mouse::Position { x: 15., y: 25. };
    let cell = size(px(10.), px(20.));
    assert!(
        tab.scroll(1, position, cell, key::Mods::empty())?
            .is_empty()
    );
    tab.terminal.vt_write(b"\x1b[?1000h\x1b[?1006h");
    assert_eq!(
        tab.scroll(-2, position, cell, key::Mods::empty())?,
        b"\x1b[<64;2;2M\x1b[<64;2;2M"
    );
    assert_eq!(
        tab.scroll(1, position, cell, key::Mods::empty())?,
        b"\x1b[<65;2;2M"
    );
    assert!(tab.scroll(1, position, cell, key::Mods::SHIFT)?.is_empty());
    assert!(
        tab.scroll(0, position, cell, key::Mods::empty())?
            .is_empty()
    );
    tab.terminal.vt_write(b"\x1b[?1000l\x1b[?1049h\x1b[?1007h");
    assert_eq!(
        tab.scroll(-1, position, cell, key::Mods::empty())?,
        b"\x1b[A"
    );
    tab.terminal.vt_write(b"\x1b[?1h");
    assert_eq!(
        tab.scroll(1, position, cell, key::Mods::empty())?,
        b"\x1bOB"
    );
    tab.terminal.vt_write(b"\x1b[?1007l");
    assert!(
        tab.scroll(1, position, cell, key::Mods::empty())?
            .is_empty()
    );
    Ok(())
}

#[test]
fn wheel_modes_survive_reconnect_snapshot() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    assert!(
        terminal.mode(ghostty_vt::terminal::Mode::ALT_SCROLL)?,
        "alternate scrolling must default on"
    );
    terminal.vt_write(b"\x1b[?1049h\x1b[?1002h\x1b[?1006h");
    let mut bytes = Vec::new();
    ghostty_vt::snapshot::encode_to(&terminal, &mut bytes)?;
    let terminal = ghostty_vt::snapshot::Decoder::from_slice(&bytes).decode()?;
    assert!(terminal.is_mouse_tracking()?);
    let mut tab = Tab::new(terminal)?;
    tab.size = (80, 24, 10, 20);
    assert_eq!(
        tab.scroll(
            -1,
            ghostty_vt::mouse::Position { x: 15., y: 25. },
            size(px(10.), px(20.)),
            key::Mods::empty()
        )?,
        b"\x1b[<64;2;2M"
    );
    Ok(())
}

#[test]
fn terminal_decorations_include_spaces_wide_cells_colors_and_resets() -> Result<()> {
    use ghostty_vt::style::Underline;
    let mut tab = Tab::new(Terminal::new(TerminalOptions::default())?)?;
    tab.terminal.vt_write("\x1b[4;9mA 界\x1b[0mB\r\n\x1b[4:2mD\x1b[4:3mC\x1b[4:4mP\x1b[4:5mH\x1b[24;29mN\r\n\x1b[4;58;2;12;34;56mX\x1b[58;5;1mY\x1b[59mZ\x1b[8;9mI".as_bytes());
    tab.snapshot()?;
    for c in &tab.cells[..4] {
        assert_eq!(c.underline, Underline::Single);
        assert!(c.strikethrough);
    }
    assert_eq!(tab.cells[4].underline, Underline::None);
    assert!(!tab.cells[4].strikethrough);
    let row: Vec<_> = tab
        .cells
        .iter()
        .filter(|c| c.y == 1)
        .take(5)
        .map(|c| c.underline)
        .collect();
    assert_eq!(
        row,
        [
            Underline::Double,
            Underline::Curly,
            Underline::Dotted,
            Underline::Dashed,
            Underline::None
        ]
    );
    let row: Vec<_> = tab.cells.iter().filter(|c| c.y == 2).take(4).collect();
    assert_eq!(row[0].underline_color, rgb(0x0c2238));
    assert_ne!(row[1].underline_color, row[0].underline_color);
    assert_eq!(row[2].underline_color, row[2].fg);
    assert_eq!(row[3].underline, Underline::None);
    assert!(!row[3].strikethrough);
    let mut bytes = Vec::new();
    ghostty_vt::snapshot::encode_to(&tab.terminal, &mut bytes)?;
    let mut restored = Tab::new(ghostty_vt::snapshot::Decoder::from_slice(&bytes).decode()?)?;
    restored.snapshot()?;
    assert!(tab.cells == restored.cells);
    Ok(())
}
#[test]
fn key_encoding_and_sync_redraw() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    let mut encoder = key::Encoder::new()?;
    let mut event = key::Event::new()?;
    event.set_key(terminal_key("escape").unwrap());
    let mut bytes = Vec::new();
    encoder
        .set_options_from_terminal(&terminal)
        .encode_to_vec(&event, &mut bytes)?;
    assert_eq!(bytes, b"\x1b");
    terminal.vt_write(b"\x1b[>1u");
    bytes.clear();
    encoder
        .set_options_from_terminal(&terminal)
        .encode_to_vec(&event, &mut bytes)?;
    assert_eq!(bytes, b"\x1b[27u");
    let mut since = None;
    terminal.vt_write(b"\x1b[?2026hhello");
    assert!(hold_frame(&terminal, &mut since)?);
    since = Some(Instant::now() - Duration::from_secs(2));
    assert!(!hold_frame(&terminal, &mut since)?);
    terminal.vt_write(b"\x1b[?2026l");
    assert!(!hold_frame(&terminal, &mut since)?);
    assert!(since.is_none());
    Ok(())
}
#[test]
fn resize_coalesces_changes_and_waits_for_acknowledgement() -> Result<()> {
    let mut tab = Tab::new(Terminal::new(TerminalOptions::default())?)?;
    let now = Instant::now();
    let first = (80, 24, 8, 16);
    let intermediate = (90, 26, 8, 16);
    let latest = (100, 30, 8, 16);
    assert!(!tab.request_resize(first, now, false));
    assert!(!tab.resize_pending);
    assert!(tab.resize_target.is_none());
    assert!(tab.request_resize(first, now, true));
    assert!(!tab.request_resize(intermediate, now + Duration::from_millis(10), true));
    assert!(!tab.request_resize(latest, now + Duration::from_millis(20), true));
    assert!(!tab.request_resize(latest, now + Duration::from_millis(200), true));
    tab.size = first;
    tab.resize_pending = false;
    // Window/font changes during an outage neither send nor consume a resize acknowledgement.
    assert!(!tab.request_resize(latest, now + Duration::from_millis(200), false));
    assert!(!tab.resize_pending);
    assert!(tab.request_resize(latest, now + Duration::from_millis(200), true));
    tab.size = latest;
    tab.resize_pending = false;
    assert!(!tab.request_resize(latest, now + Duration::from_millis(210), true));
    assert!(!tab.request_resize(intermediate, now + Duration::from_millis(220), true));
    assert!(!tab.request_resize(intermediate, now + Duration::from_millis(250), true));
    assert!(tab.request_resize(intermediate, now + Duration::from_millis(300), true));
    Ok(())
}

#[test]
fn application_cursor_shapes_and_blink_survive_snapshots() -> Result<()> {
    use ghostty_vt::render::CursorVisualStyle::{Bar, Block, Underline};
    let mut tab = Tab::new(Terminal::new(TerminalOptions::default())?)?;
    for (code, style, blinking) in [
        (1, Block, true),
        (2, Block, false),
        (3, Underline, true),
        (4, Underline, false),
        (5, Bar, true),
        (6, Bar, false),
    ] {
        tab.terminal.vt_write(format!("\x1b[{code} q").as_bytes());
        tab.snapshot()?;
        let cursor = tab.cursor_state.unwrap();
        assert_eq!(cursor.visual_style, style);
        assert_eq!(cursor.blinking, blinking);
        let mut bytes = Vec::new();
        ghostty_vt::snapshot::encode_to(&tab.terminal, &mut bytes)?;
        let mut restored = Tab::new(ghostty_vt::snapshot::Decoder::from_slice(&bytes).decode()?)?;
        restored.snapshot()?;
        assert_eq!(restored.cursor_state, tab.cursor_state);
    }
    tab.terminal.vt_write(b"\x1b[?25l");
    tab.snapshot()?;
    assert!(tab.cursor.is_none());
    Ok(())
}

#[test]
fn application_cursor_visibility_and_restore() -> Result<()> {
    let mut tab = Tab::new(Terminal::new(TerminalOptions::default())?)?;
    for bytes in [b"\x1b[?25l".as_slice(), b"text", b"\x1b[?1049h"] {
        tab.terminal.vt_write(bytes);
        tab.snapshot()?;
        assert!(tab.cursor.is_none());
    }
    tab.terminal.vt_write(b"\x1b[?1049l\x1b[?25h");
    for _ in 0..3 {
        tab.snapshot()?;
        assert!(tab.cursor.is_some());
    }
    tab.terminal.vt_write(b"\x1b[H\x1b[?25l\x1b[7m \x1b[0m");
    tab.snapshot()?;
    assert!(tab.cursor.is_none());
    assert!(
        tab.cells[0].bg.is_some(),
        "software cursor on a blank cell must retain its background"
    );
    for visible in [false, true] {
        tab.terminal
            .vt_write(if visible { b"\x1b[?25h" } else { b"\x1b[?25l" });
        let mut bytes = Vec::new();
        ghostty_vt::snapshot::encode_to(&tab.terminal, &mut bytes)?;
        let terminal = ghostty_vt::snapshot::Decoder::from_slice(&bytes).decode()?;
        let mut restored = Tab::new(terminal)?;
        restored.snapshot()?;
        assert_eq!(restored.cursor.is_some(), visible);
    }
    Ok(())
}
#[test]
fn osc_default_colors_update_cached_cells_and_reset() -> Result<()> {
    use ghostty_vt::style::RgbColor;
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    terminal.set_default_fg_color(Some(RgbColor {
        r: 0xdc,
        g: 0xdc,
        b: 0xdc,
    }))?;
    terminal.set_default_bg_color(Some(RgbColor { r: 0, g: 0, b: 0 }))?;
    terminal.set_default_cursor_color(Some(RgbColor {
        r: 255,
        g: 255,
        b: 255,
    }))?;
    terminal.vt_write(b"A\x1b[7mB\x1b[0m");
    let mut tab = Tab::new(terminal)?;
    tab.snapshot()?;
    assert_eq!(tab.cells[0].fg, rgb(0xdcdcdc));
    tab.terminal
        .vt_write(b"\x1b]10;#112233\x07\x1b]11;#445566\x07\x1b]12;#778899\x07");
    tab.snapshot()?;
    assert_eq!(tab.cells[0].fg, rgb(0x112233));
    assert_eq!(tab.cells[1].fg, rgb(0x445566));
    assert_eq!(tab.cells[1].bg, Some(rgb(0x112233)));
    assert_eq!(tab.cursor_color, rgb(0x778899));
    assert_eq!(color(tab.terminal.bg_color()?, rgb(0)), rgb(0x445566));
    let mut bytes = Vec::new();
    ghostty_vt::snapshot::encode_to(&tab.terminal, &mut bytes)?;
    let mut tab = Tab::new(ghostty_vt::snapshot::Decoder::from_slice(&bytes).decode()?)?;
    tab.snapshot()?;
    assert_eq!(tab.cells[0].fg, rgb(0x112233));
    assert_eq!(tab.cursor_color, rgb(0x778899));
    tab.terminal
        .vt_write(b"\x1b]110\x07\x1b]111\x07\x1b]112\x07");
    tab.snapshot()?;
    assert_eq!(tab.cells[0].fg, rgb(0xdcdcdc));
    assert_eq!(tab.cells[1].fg, rgb(0));
    assert_eq!(tab.cursor_color, rgb(0xffffff));
    assert_eq!(color(tab.terminal.bg_color()?, rgb(0)), rgb(0));
    tab.terminal.vt_write(b"\x1b[?2026h\x1b]11;#123456\x07");
    assert_eq!(tab.background()?, rgb(0));
    tab.snapshot()?;
    assert_eq!(tab.background()?, rgb(0));
    tab.terminal.vt_write(b"\x1b[?2026l");
    assert_eq!(tab.background()?, rgb(0x123456));
    Ok(())
}
#[test]
fn incremental_cells_match_full_rebuilds() -> Result<()> {
    fn compare(tab: &mut Tab) -> Result<()> {
        tab.snapshot()?;
        let cells = tab.cells.clone();
        let cursor = tab.cursor;
        tab.render = RenderState::new()?;
        tab.snapshot()?;
        assert!(
            tab.cells == cells,
            "incremental cells differ from a full rebuild"
        );
        assert_eq!(tab.cursor, cursor);
        Ok(())
    }
    let mut tab = Tab::new(Terminal::new(TerminalOptions {
        cols: 12,
        rows: 4,
        ..Default::default()
    })?)?;
    for bytes in [
        "first\r\nsecond\r\nthird\r\nfourth",
        "\x1b[2;3Hé界\x1b[31mred\x1b[0m",
        "\x1b[1;1H\x1b[7mX\x1b[0m",
        "\r\nnew line\r\nmore history",
        "\x1b[?25l",
        "\x1b[?1049halternate",
        "\x1b[?1049l\x1b[?25h",
        "\x1b[2J",
    ] {
        tab.terminal.vt_write(bytes.as_bytes());
        compare(&mut tab)?;
    }
    tab.terminal.scroll_viewport(ScrollViewport::Top);
    compare(&mut tab)?;
    let start = tab
        .terminal
        .grid_ref(TerminalPoint::Viewport(PointCoordinate { x: 0, y: 0 }))?;
    let end = tab
        .terminal
        .grid_ref(TerminalPoint::Viewport(PointCoordinate { x: 5, y: 1 }))?;
    tab.terminal
        .set_selection(Some(&Selection::new(start, end, false)))?;
    compare(&mut tab)?;
    tab.terminal.set_selection(None)?;
    compare(&mut tab)?;
    // Same cell count, different geometry must invalidate the whole cache.
    resize_terminal(&mut tab.terminal, 8, 6, 80, 120)?;
    compare(&mut tab)?;
    compare(&mut tab)?;
    Ok(())
}

#[test]
#[ignore = "manual snapshot timing; run with --ignored --nocapture"]
fn snapshot_timing() -> Result<()> {
    let mut tab = Tab::new(Terminal::new(TerminalOptions {
        cols: 120,
        rows: 40,
        ..Default::default()
    })?)?;
    tab.terminal.vt_write(&b"abcdef ".repeat(600));
    tab.snapshot()?;
    let start = Instant::now();
    for _ in 0..500 {
        tab.snapshot()?;
    }
    eprintln!("500 idle snapshots: {:?}", start.elapsed());
    let start = Instant::now();
    for _ in 0..500 {
        tab.terminal.vt_write(b"\x1b[1;1HX");
        tab.snapshot()?;
    }
    eprintln!("500 single-row snapshots: {:?}", start.elapsed());
    Ok(())
}

#[test]
fn unicode_graphemes_keep_terminal_widths_copy_and_reconnect() -> Result<()> {
    for (text, width) in [
        ("e\u{301}", 1),
        ("a\u{301}\u{308}", 1),
        ("界", 2),
        ("👩🏽‍💻", 2),
        ("🇵🇱", 2),
        ("1️⃣", 2),
        ("♥︎", 1),
        ("♥️", 2),
        ("कि", 2),
        ("क्ष", 2),
        ("بِ", 1),
        ("שָׁ", 1),
        ("한", 2),
    ] {
        let mut tab = Tab::new(Terminal::new(TerminalOptions::default())?)?;
        // A PTY may split a UTF-8 scalar or a grapheme across arbitrary reads.
        for byte in text.as_bytes() {
            tab.terminal.vt_write(&[*byte]);
            tab.snapshot()?;
        }
        tab.terminal.vt_write(b"X");
        tab.snapshot()?;
        assert_eq!(tab.cells[0].text, text, "{text:?}");
        assert_eq!(tab.cells[0].width, width, "{text:?}");
        assert_eq!(tab.cells[usize::from(width)].text, "X", "{text:?}");
        if width == 2 {
            assert_eq!(tab.cells[1].width, 0);
            assert!(tab.cells[1].text.is_empty());
        }
        let start = tab
            .terminal
            .grid_ref(TerminalPoint::Viewport(PointCoordinate { x: 0, y: 0 }))?;
        let end = tab
            .terminal
            .grid_ref(TerminalPoint::Viewport(PointCoordinate {
                x: u16::from(width) - 1,
                y: 0,
            }))?;
        tab.terminal
            .set_selection(Some(&Selection::new(start, end, false)))?;
        assert_eq!(selected_text(&tab.terminal)?.as_deref(), Some(text));
        tab.terminal.set_selection(None)?;
        let mut bytes = Vec::new();
        ghostty_vt::snapshot::encode_to(&tab.terminal, &mut bytes)?;
        let mut restored = Tab::new(ghostty_vt::snapshot::Decoder::from_slice(&bytes).decode()?)?;
        restored.snapshot()?;
        assert!(restored.cells == tab.cells, "{text:?}");
    }
    Ok(())
}

#[test]
fn wide_wrap_padding_is_not_a_glyph_or_copied_space() -> Result<()> {
    let mut tab = Tab::new(Terminal::new(TerminalOptions {
        cols: 4,
        rows: 3,
        ..Default::default()
    })?)?;
    tab.terminal.vt_write("abc界X".as_bytes());
    tab.snapshot()?;
    assert_eq!(tab.cells[3].width, 0);
    assert!(tab.cells[3].text.is_empty());
    assert_eq!((tab.cells[4].text.as_str(), tab.cells[4].width), ("界", 2));
    assert_eq!(tab.cells[5].width, 0);
    assert!(tab.cells[5].text.is_empty());
    assert_eq!(tab.cells[6].text, "X");
    let start = tab
        .terminal
        .grid_ref(TerminalPoint::Viewport(PointCoordinate { x: 0, y: 0 }))?;
    let end = tab
        .terminal
        .grid_ref(TerminalPoint::Viewport(PointCoordinate { x: 2, y: 1 }))?;
    tab.terminal
        .set_selection(Some(&Selection::new(start, end, false)))?;
    assert_eq!(selected_text(&tab.terminal)?.as_deref(), Some("abc界X"));
    resize_terminal(&mut tab.terminal, 8, 3, 9, 18)?;
    tab.snapshot()?;
    assert_eq!(tab.cells[3].text, "界");
    assert_eq!(tab.cells[3].width, 2);
    assert_eq!(tab.cells[4].width, 0);
    assert_eq!(tab.cells[5].text, "X");
    assert_eq!(selected_text(&tab.terminal)?.as_deref(), Some("abc界X"));
    tab.terminal.vt_write(b"\r\x1b[2Kx");
    tab.snapshot()?;
    assert!(tab.cells.iter().all(|c| c.width == 1));
    Ok(())
}

#[test]
fn selection_and_snapshot() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions {
        cols: 5,
        rows: 3,
        ..Default::default()
    })?;
    terminal.vt_write("héllo world".as_bytes());
    let start = terminal.grid_ref(TerminalPoint::Viewport(PointCoordinate { x: 0, y: 0 }))?;
    let end = terminal.grid_ref(TerminalPoint::Viewport(PointCoordinate { x: 4, y: 1 }))?;
    terminal.set_selection(Some(&Selection::new(start, end, false)))?;
    assert_eq!(selected_text(&terminal)?.as_deref(), Some("héllo worl"));
    assert_eq!(
        selection_point(
            point(px(-5.), px(99.)),
            Bounds::new(point(px(0.), px(0.)), size(px(50.), px(30.))),
            size(px(10.), px(10.)),
            5,
            3
        ),
        TerminalPoint::Viewport(PointCoordinate { x: 0, y: 2 })
    );
    let mut tab = Tab::new(terminal)?;
    tab.snapshot()?;
    assert!(tab.cells.iter().any(|c| c.text == "é"));
    Ok(())
}
#[test]
fn focus_reporting_is_negotiated_deduplicated_and_resets() -> Result<()> {
    let mut tab = Tab::new(Terminal::new(TerminalOptions::default())?)?;
    assert!(tab.focus(true)?.is_none());
    tab.terminal.vt_write(b"\x1b[?1004h");
    assert_eq!(tab.focus(true)?, Some(b"\x1b[I".as_slice()));
    assert!(tab.focus(true)?.is_none());
    assert_eq!(tab.focus(false)?, Some(b"\x1b[O".as_slice()));
    assert!(tab.focus(false)?.is_none());
    tab.terminal.vt_write(b"\x1b[?1004l");
    assert!(tab.focus(false)?.is_none());
    tab.terminal.vt_write(b"\x1b[?1004h");
    assert_eq!(tab.focus(false)?, Some(b"\x1b[O".as_slice()));
    tab.exited = true;
    assert!(tab.focus(true)?.is_none());
    Ok(())
}

#[test]
fn resize_preserves_scrollback_position() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions::default())?;
    for _ in 0..100 {
        terminal.vt_write(b"output\r\n");
    }
    resize_terminal(&mut terminal, 40, 12, 8, 16)?;
    let scroll = terminal.scrollbar()?;
    assert_eq!(scroll.offset + scroll.len, scroll.total);
    terminal.scroll_viewport(ScrollViewport::Top);
    resize_terminal(&mut terminal, 50, 15, 8, 16)?;
    let scroll = terminal.scrollbar()?;
    assert!(scroll.offset + scroll.len < scroll.total);
    Ok(())
}
