use super::grid_bounds;
use gpui::{Bounds, Pixels, Point, Size};

// Eighth-cell rectangles for Unicode block elements. Draw these against the
// grid rather than using platform font bearings and fallback glyph metrics.
pub(super) fn block_rectangles(text: &str) -> Option<Vec<(u8, u8, u8, u8)>> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let rects = match ch {
        '▀' => vec![(0, 0, 8, 4)],
        '▁'..='█' => {
            let height = (ch as u32 - '▁' as u32 + 1) as u8;
            vec![(0, 8 - height, 8, height)]
        }
        '▉'..='▏' => vec![(0, 0, ('▏' as u32 - ch as u32 + 1) as u8, 8)],
        '▐' => vec![(4, 0, 4, 8)],
        '▔' => vec![(0, 0, 8, 1)],
        '▕' => vec![(7, 0, 1, 8)],
        '▖'..='▟' => {
            let quadrants = [4, 8, 1, 13, 9, 7, 11, 2, 6, 14][ch as usize - '▖' as usize];
            (0..4)
                .filter(|q| quadrants & (1 << q) != 0)
                .map(|q| ((q % 2) * 4, (q / 2) * 4, 4, 4))
                .collect()
        }
        _ => return None,
    };
    Some(rects)
}

pub(super) fn block_bounds(
    origin: Point<Pixels>,
    cell: Size<Pixels>,
    rect: (u8, u8, u8, u8),
    scale: f32,
) -> Bounds<Pixels> {
    grid_bounds(origin, cell, rect, (8, 8), scale)
}

#[cfg(test)]
#[path = "../../tests/unit/custom_glyph/blocks.rs"]
mod tests;
