//! Font-independent terminal graphics. Platform policy, decoding, geometry,
//! and painting live here so normal text and cursor redraws use the same path.
use gpui::{Bounds, Pixels, Point, Rgba, Size, Window, fill, point, px, size};

mod blocks;
mod box_drawing;
use blocks::{block_bounds, block_rectangles};
mod mosaic_masks;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Glyph {
    Block(char),
    Border(char),
    Braille(u8),
    Shade(u8),
    Mosaic { mask: u8, rows: u8 },
}

struct Shape {
    bounds: Bounds<Pixels>,
    radius: Pixels,
    opacity: f32,
}

impl Glyph {
    pub(super) fn from_text(text: &str) -> Option<Self> {
        Self::decode(text, cfg!(target_os = "windows"))
    }

    fn decode(text: &str, windows: bool) -> Option<Self> {
        if !windows {
            return None;
        }
        let mut chars = text.chars();
        let ch = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        match ch {
            '\u{2580}'..='\u{2590}' | '\u{2594}'..='\u{259f}' => Some(Self::Block(ch)),
            '\u{2500}'..='\u{257f}' => Some(Self::Border(ch)),
            '\u{2800}'..='\u{28ff}' => Some(Self::Braille((ch as u32 - 0x2800) as u8)),
            '\u{2591}'..='\u{2593}' => Some(Self::Shade((ch as u32 - 0x2590) as u8)),
            '\u{1fb00}'..='\u{1fb3b}' => Some(Self::Mosaic {
                mask: mosaic_masks::SEXTANTS[ch as usize - 0x1fb00],
                rows: 3,
            }),
            '\u{1cd00}'..='\u{1cde5}' => Some(Self::Mosaic {
                mask: mosaic_masks::OCTANTS[ch as usize - 0x1cd00],
                rows: 4,
            }),
            // Octants that Unicode assigned outside the main octant range.
            '\u{1cea0}' | '\u{1cea3}' | '\u{1cea8}' | '\u{1ceab}' | '\u{1fbe6}' | '\u{1fbe7}' => {
                Some(Self::Mosaic {
                    mask: match ch {
                        '\u{1cea0}' => 128,
                        '\u{1cea3}' => 64,
                        '\u{1cea8}' => 1,
                        '\u{1ceab}' => 2,
                        '\u{1fbe6}' => 20,
                        _ => 40,
                    },
                    rows: 4,
                })
            }
            _ => None,
        }
    }

    fn shapes(self, origin: Point<Pixels>, cell: Size<Pixels>, scale: f32) -> Vec<Shape> {
        if cell.width <= px(0.) || cell.height <= px(0.) {
            return Vec::new();
        }
        let mut utf8 = [0; 4];
        let (bounds, round) = match self {
            Self::Block(ch) => (
                block_rectangles(ch.encode_utf8(&mut utf8))
                    .unwrap()
                    .into_iter()
                    .map(|rect| block_bounds(origin, cell, rect, scale))
                    .collect(),
                false,
            ),
            Self::Border(ch) => (
                box_drawing::rectangles(ch.encode_utf8(&mut utf8), origin, cell, scale).unwrap(),
                false,
            ),
            Self::Braille(mask) => (braille_dots(mask, origin, cell, scale), true),
            Self::Shade(_) => (vec![block_bounds(origin, cell, (0, 0, 8, 8), scale)], false),
            Self::Mosaic { mask, rows } => (
                (0..rows * 2)
                    .filter(|bit| mask & (1 << bit) != 0)
                    .map(|bit| {
                        grid_bounds(origin, cell, (bit % 2, bit / 2, 1, 1), (2, rows), scale)
                    })
                    .collect(),
                false,
            ),
        };
        bounds
            .into_iter()
            .map(|bounds| Shape {
                radius: if round {
                    bounds.size.width / 2.
                } else {
                    px(0.)
                },
                bounds,
                opacity: if let Self::Shade(level) = self {
                    f32::from(level) / 4.
                } else {
                    1.
                },
            })
            .collect()
    }

    pub(super) fn paint(
        self,
        origin: Point<Pixels>,
        cell: Size<Pixels>,
        color: Rgba,
        window: &mut Window,
    ) {
        for shape in self.shapes(origin, cell, window.scale_factor()) {
            let mut color = color;
            color.a *= shape.opacity;
            window.paint_quad(fill(shape.bounds, color).corner_radii(shape.radius));
        }
    }
}

fn grid_bounds(
    origin: Point<Pixels>,
    cell: Size<Pixels>,
    rect: (u8, u8, u8, u8),
    divisions: (u8, u8),
    scale: f32,
) -> Bounds<Pixels> {
    let snap = |value: Pixels| px((f32::from(value) * scale).round() / scale);
    let (x, y, width, height) = rect;
    let left = snap(origin.x + cell.width * (f32::from(x) / f32::from(divisions.0)));
    let top = snap(origin.y + cell.height * (f32::from(y) / f32::from(divisions.1)));
    let right = snap(origin.x + cell.width * (f32::from(x + width) / f32::from(divisions.0)));
    let bottom = snap(origin.y + cell.height * (f32::from(y + height) / f32::from(divisions.1)));
    Bounds::new(point(left, top), size(right - left, bottom - top))
}

fn braille_dots(
    mask: u8,
    origin: Point<Pixels>,
    cell: Size<Pixels>,
    scale: f32,
) -> Vec<Bounds<Pixels>> {
    let left = (f32::from(origin.x) * scale).round();
    let top = (f32::from(origin.y) * scale).round();
    let right = (f32::from(origin.x + cell.width) * scale).round();
    let bottom = (f32::from(origin.y + cell.height) * scale).round();
    let pitch = ((right - left) / 2.).min((bottom - top) / 4.).floor();
    if pitch < 1. {
        return Vec::new();
    }
    let diameter = (pitch / 2.).floor().max(1.);
    // Snap the complete 2×4 grid once. Rounding each fractional dot center
    // independently alternates the gaps (e.g. 4px, 5px, 4px at Windows DPI).
    let grid_left = ((left + right - pitch - diameter) / 2.).round();
    let grid_top = ((top + bottom - pitch * 3. - diameter) / 2.).round();
    // Unicode dots 1–3 are the upper left column, 4–6 the upper
    // right column, then dots 7 and 8 occupy the bottom row.
    [
        (0, 0),
        (0, 1),
        (0, 2),
        (1, 0),
        (1, 1),
        (1, 2),
        (0, 3),
        (1, 3),
    ]
    .into_iter()
    .enumerate()
    .filter(|(bit, _)| mask & (1 << bit) != 0)
    .map(|(_, (x, y))| {
        Bounds::new(
            point(
                px((grid_left + x as f32 * pitch) / scale),
                px((grid_top + y as f32 * pitch) / scale),
            ),
            size(px(diameter / scale), px(diameter / scale)),
        )
    })
    .collect()
}

#[cfg(test)]
#[path = "../../tests/unit/custom_glyph/mod.rs"]
mod tests;
