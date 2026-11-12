use super::*;
use ghostty_vt::{TerminalOptions, terminal::ScrollViewport};

#[test]
fn prompt_navigation_and_output_selection_survive_snapshot() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions {
        cols: 40,
        rows: 3,
        ..Default::default()
    })?;
    terminal.vt_write(b"\x1b]133;A\x07$ \x1b]133;B\x07one\r\n\x1b]133;C\x07first output\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ \x1b]133;B\x07two\r\n\x1b]133;C\x07second output\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ \x1b]133;B\x07");
    let mut bytes = Vec::new();
    ghostty_vt::snapshot::encode_to(&terminal, &mut bytes)?;
    let mut terminal = ghostty_vt::snapshot::Decoder::from_slice(&bytes).decode()?;
    assert_eq!(prompt_row(&terminal, true)?, Some(0));
    terminal.scroll_viewport(ScrollViewport::Top);
    assert_eq!(prompt_row(&terminal, false)?, Some(2));
    let selection = terminal
        .select_output(terminal.grid_ref(Point::Screen(PointCoordinate { x: 2, y: 1 }))?)?
        .expect("OSC 133 output");
    terminal.set_selection(Some(&selection))?;
    let text = terminal
        .format_selection_alloc(Default::default())?
        .expect("selected output text");
    assert_eq!(std::str::from_utf8(&text)?.trim_end(), "first output");
    Ok(())
}
