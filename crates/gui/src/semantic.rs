use anyhow::Result;
use ghostty_vt::{
    Terminal,
    screen::RowSemanticPrompt,
    terminal::{Point, PointCoordinate},
};

/// Prompt rows are engine-owned, so navigation survives scrolling, resizing and reattachment.
pub fn prompt_row(terminal: &Terminal, older: bool) -> Result<Option<usize>> {
    let bar = terminal.scrollbar()?;
    let offset = usize::try_from(bar.offset)?;
    let total = usize::try_from(bar.total)?;
    let is_prompt = |row: usize| -> Result<bool> {
        let y = u32::try_from(row)?;
        Ok(terminal
            .grid_ref(Point::Screen(PointCoordinate { x: 0, y }))?
            .row()?
            .semantic_prompt()?
            == RowSemanticPrompt::Prompt)
    };
    if older {
        for row in (0..offset).rev() {
            if is_prompt(row)? {
                return Ok(Some(row));
            }
        }
    } else {
        for row in offset.saturating_add(1)..total {
            if is_prompt(row)? {
                return Ok(Some(row));
            }
        }
        return Ok(Some(usize::try_from(bar.total.saturating_sub(bar.len))?));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
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
}
