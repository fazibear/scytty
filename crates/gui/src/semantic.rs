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
#[path = "../tests/unit/semantic.rs"]
mod tests;
