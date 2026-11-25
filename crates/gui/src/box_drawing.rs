use gpui::{Bounds, Pixels, Point, Size, point, px, size};

// Solid box-drawing arms, ordered left, right, up, down. Font weight and
// italic styles must not change these grid connections.
fn arms(text: &str) -> Option<[u8; 4]> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(match ch {
        '─' => [1, 1, 0, 0],
        '━' => [2, 2, 0, 0],
        '│' => [0, 0, 1, 1],
        '┃' => [0, 0, 2, 2],
        '┌' => [0, 1, 0, 1],
        '┍' => [0, 2, 0, 1],
        '┎' => [0, 1, 0, 2],
        '┏' => [0, 2, 0, 2],
        '┐' => [1, 0, 0, 1],
        '┑' => [2, 0, 0, 1],
        '┒' => [1, 0, 0, 2],
        '┓' => [2, 0, 0, 2],
        '└' => [0, 1, 1, 0],
        '┕' => [0, 2, 1, 0],
        '┖' => [0, 1, 2, 0],
        '┗' => [0, 2, 2, 0],
        '┘' => [1, 0, 1, 0],
        '┙' => [2, 0, 1, 0],
        '┚' => [1, 0, 2, 0],
        '┛' => [2, 0, 2, 0],
        '├' => [0, 1, 1, 1],
        '┝' => [0, 2, 1, 1],
        '┞' => [0, 1, 2, 1],
        '┟' => [0, 1, 1, 2],
        '┠' => [0, 1, 2, 2],
        '┡' => [0, 2, 2, 1],
        '┢' => [0, 2, 1, 2],
        '┣' => [0, 2, 2, 2],
        '┤' => [1, 0, 1, 1],
        '┥' => [2, 0, 1, 1],
        '┦' => [1, 0, 2, 1],
        '┧' => [1, 0, 1, 2],
        '┨' => [1, 0, 2, 2],
        '┩' => [2, 0, 2, 1],
        '┪' => [2, 0, 1, 2],
        '┫' => [2, 0, 2, 2],
        '┬' => [1, 1, 0, 1],
        '┭' => [2, 1, 0, 1],
        '┮' => [1, 2, 0, 1],
        '┯' => [2, 2, 0, 1],
        '┰' => [1, 1, 0, 2],
        '┱' => [2, 1, 0, 2],
        '┲' => [1, 2, 0, 2],
        '┳' => [2, 2, 0, 2],
        '┴' => [1, 1, 1, 0],
        '┵' => [2, 1, 1, 0],
        '┶' => [1, 2, 1, 0],
        '┷' => [2, 2, 1, 0],
        '┸' => [1, 1, 2, 0],
        '┹' => [2, 1, 2, 0],
        '┺' => [1, 2, 2, 0],
        '┻' => [2, 2, 2, 0],
        '┼' => [1, 1, 1, 1],
        '┽' => [2, 1, 1, 1],
        '┾' => [1, 2, 1, 1],
        '┿' => [2, 2, 1, 1],
        '╀' => [1, 1, 2, 1],
        '╁' => [1, 1, 1, 2],
        '╂' => [1, 1, 2, 2],
        '╃' => [2, 1, 2, 1],
        '╄' => [1, 2, 2, 1],
        '╅' => [2, 1, 1, 2],
        '╆' => [1, 2, 1, 2],
        '╇' => [2, 2, 2, 1],
        '╈' => [2, 2, 1, 2],
        '╉' => [2, 1, 2, 2],
        '╊' => [1, 2, 2, 2],
        '╋' => [2, 2, 2, 2],
        '╴' => [1, 0, 0, 0],
        '╵' => [0, 0, 1, 0],
        '╶' => [0, 1, 0, 0],
        '╷' => [0, 0, 0, 1],
        '╸' => [2, 0, 0, 0],
        '╹' => [0, 0, 2, 0],
        '╺' => [0, 2, 0, 0],
        '╻' => [0, 0, 0, 2],
        '╼' => [1, 2, 0, 0],
        '╽' => [0, 0, 1, 2],
        '╾' => [2, 1, 0, 0],
        '╿' => [0, 0, 2, 1],
        _ => return None,
    })
}

pub(super) fn supported(text: &str) -> bool {
    // Windows fallback font metrics can leave gaps between border cells.
    // Keep native font rendering on the other platforms.
    cfg!(target_os = "windows") && arms(text).is_some()
}

pub(super) fn rectangles(
    text: &str,
    origin: Point<Pixels>,
    cell: Size<Pixels>,
    scale: f32,
) -> Option<Vec<Bounds<Pixels>>> {
    let arms = arms(text)?;
    let snap = |value: Pixels| (f32::from(value) * scale).round();
    let left = snap(origin.x);
    let top = snap(origin.y);
    let right = snap(origin.x + cell.width);
    let bottom = snap(origin.y + cell.height);
    let light = (f32::from(cell.height) * scale / 16.).round().max(1.);
    let stroke = |weight: u8| {
        (light * f32::from(weight))
            .min(right - left)
            .min(bottom - top)
    };
    let band = |start: f32, end: f32, width: f32| {
        let first = ((start + end - width) / 2.)
            .round()
            .clamp(start, end - width);
        (first, first + width)
    };
    let horizontal = stroke(arms[0].max(arms[1]));
    let vertical = stroke(arms[2].max(arms[3]));
    let (center_left, center_right) = band(left, right, vertical.max(light).min(right - left));
    let (center_top, center_bottom) = band(top, bottom, horizontal.max(light).min(bottom - top));
    let mut rects = Vec::new();
    for (direction, weight) in arms.into_iter().enumerate() {
        if weight == 0 {
            continue;
        }
        let width = stroke(weight);
        let (x, y, end_x, end_y) = match direction {
            0 | 1 => {
                let (y, end_y) = band(top, bottom, width);
                if direction == 0 {
                    (left, y, center_right, end_y)
                } else {
                    (center_left, y, right, end_y)
                }
            }
            _ => {
                let (x, end_x) = band(left, right, width);
                if direction == 2 {
                    (x, top, end_x, center_bottom)
                } else {
                    (x, center_top, end_x, bottom)
                }
            }
        };
        rects.push(Bounds::new(
            point(px(x / scale), px(y / scale)),
            size(px((end_x - x) / scale), px((end_y - y) / scale)),
        ));
    }
    Some(rects)
}

#[cfg(test)]
#[path = "../tests/unit/box_drawing.rs"]
mod tests;
