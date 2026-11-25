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
        '─' | '┄' | '┈' | '╌' => [1, 1, 0, 0],
        '━' | '┅' | '┉' | '╍' => [2, 2, 0, 0],
        '│' | '┆' | '┊' | '╎' => [0, 0, 1, 1],
        '┃' | '┇' | '┋' | '╏' => [0, 0, 2, 2],
        '┌' | '╭' => [0, 1, 0, 1],
        '┍' => [0, 2, 0, 1],
        '┎' => [0, 1, 0, 2],
        '┏' => [0, 2, 0, 2],
        '┐' | '╮' => [1, 0, 0, 1],
        '┑' => [2, 0, 0, 1],
        '┒' => [1, 0, 0, 2],
        '┓' => [2, 0, 0, 2],
        '└' | '╰' => [0, 1, 1, 0],
        '┕' => [0, 2, 1, 0],
        '┖' => [0, 1, 2, 0],
        '┗' => [0, 2, 2, 0],
        '┘' | '╯' => [1, 0, 1, 0],
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
        '╱' | '╲' | '╳' => [0, 0, 0, 0],
        '═' => [3, 3, 0, 0],
        '║' => [0, 0, 3, 3],
        '╒' => [0, 3, 0, 1],
        '╓' => [0, 1, 0, 3],
        '╔' => [0, 3, 0, 3],
        '╕' => [3, 0, 0, 1],
        '╖' => [1, 0, 0, 3],
        '╗' => [3, 0, 0, 3],
        '╘' => [0, 3, 1, 0],
        '╙' => [0, 1, 3, 0],
        '╚' => [0, 3, 3, 0],
        '╛' => [3, 0, 1, 0],
        '╜' => [1, 0, 3, 0],
        '╝' => [3, 0, 3, 0],
        '╞' => [0, 3, 1, 1],
        '╟' => [0, 1, 3, 3],
        '╠' => [0, 3, 3, 3],
        '╡' => [3, 0, 1, 1],
        '╢' => [1, 0, 3, 3],
        '╣' => [3, 0, 3, 3],
        '╤' => [3, 3, 0, 1],
        '╥' => [1, 1, 0, 3],
        '╦' => [3, 3, 0, 3],
        '╧' => [3, 3, 1, 0],
        '╨' => [1, 1, 3, 0],
        '╩' => [3, 3, 3, 0],
        '╪' => [3, 3, 1, 1],
        '╫' => [1, 1, 3, 3],
        '╬' => [3, 3, 3, 3],
        _ => return None,
    })
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
    let dash_count = match text {
        "┄" | "┅" | "┆" | "┇" => 3,
        "┈" | "┉" | "┊" | "┋" => 4,
        "╌" | "╍" | "╎" | "╏" => 2,
        _ => 0,
    };
    if dash_count != 0 {
        let horizontal = arms[0] != 0;
        let (start, end) = if horizontal {
            (left, right)
        } else {
            (top, bottom)
        };
        let width = stroke(arms.into_iter().max().unwrap());
        let (cross_start, cross_end) = if horizontal {
            band(top, bottom, width)
        } else {
            band(left, right, width)
        };
        for dash in 0..dash_count {
            let a = (start + (end - start) * dash as f32 / dash_count as f32).round();
            let b = (start + (end - start) * (dash as f32 + 0.6) / dash_count as f32).round();
            if b > a {
                let (x, y, w, h) = if horizontal {
                    (a, cross_start, b - a, cross_end - cross_start)
                } else {
                    (cross_start, a, cross_end - cross_start, b - a)
                };
                rects.push(Bounds::new(
                    point(px(x / scale), px(y / scale)),
                    size(px(w / scale), px(h / scale)),
                ));
            }
        }
        return Some(rects);
    }
    if matches!(text, "╱" | "╲" | "╳") {
        // Sample both diagonals as one mask so crossing pixels are painted once.
        for y in top as i32..bottom as i32 {
            let t = (y as f32 + 0.5 - top) / (bottom - top);
            for x in left as i32..right as i32 {
                let descending = left + (right - left) * t;
                let ascending = right - (right - left) * t;
                if (text != "╱" && (x as f32 + 0.5 - descending).abs() <= light / 2.)
                    || (text != "╲" && (x as f32 + 0.5 - ascending).abs() <= light / 2.)
                {
                    rects.push(Bounds::new(
                        point(px(x as f32 / scale), px(y as f32 / scale)),
                        size(px(1. / scale), px(1. / scale)),
                    ));
                }
            }
        }
        return Some(rects);
    }
    if arms.contains(&3) {
        // A double arm has two light rails. At each quadrant, the neighboring
        // arm decides whether a rail turns inward, outward, or meets a single
        // stroke. These same rules cover corners, tees, and crossings.
        let width = stroke(1);
        let (xl, xr) = band(left, right, width);
        let (yt, yb) = band(top, bottom, width);
        let gap = width;
        let mut add = |x: f32, y: f32, r: f32, b: f32| {
            let (x, y, r, b) = (x.max(left), y.max(top), r.min(right), b.min(bottom));
            if r > x && b > y {
                rects.push(Bounds::new(
                    point(px(x / scale), px(y / scale)),
                    size(px((r - x) / scale), px((b - y) / scale)),
                ));
            }
        };
        for (direction, weight) in arms.into_iter().enumerate() {
            if weight == 0 {
                continue;
            }
            let sign = if direction == 0 || direction == 2 {
                -1.
            } else {
                1.
            };
            let horizontal = direction < 2;
            let offsets: &[f32] = if weight == 3 { &[-1., 1.] } else { &[0.] };
            for &side in offsets {
                let neighbor = if horizontal {
                    if side < 0. { 2 } else { 3 }
                } else if side < 0. {
                    0
                } else {
                    1
                };
                let opposite = neighbor ^ 1;
                let turn = if side == 0. {
                    let perpendicular = if horizontal { &arms[2..4] } else { &arms[0..2] };
                    if perpendicular.contains(&3) {
                        -sign * gap
                    } else {
                        0.
                    }
                } else if arms[neighbor] == 3 {
                    sign * gap
                } else if arms[opposite] == 3 {
                    -sign * gap
                } else {
                    0.
                };
                if horizontal {
                    let y = yt + side * gap;
                    if direction == 0 {
                        add(left, y, xr + turn, y + width);
                    } else {
                        add(xl + turn, y, right, y + width);
                    }
                } else {
                    let x = xl + side * gap;
                    if direction == 2 {
                        add(x, top, x + width, yb + turn);
                    } else {
                        add(x, yt + turn, x + width, bottom);
                    }
                }
            }
        }
        return Some(rects);
    }
    if matches!(text, "╭" | "╮" | "╯" | "╰") {
        // Rasterize the quarter-circle and its two legs on the same physical
        // pixel bands as straight borders. Font bearings cannot align these.
        let cx = (center_left + center_right) / 2.;
        let cy = (center_top + center_bottom) / 2.;
        let sx = if arms[1] != 0 { 1. } else { -1. };
        let sy = if arms[3] != 0 { 1. } else { -1. };
        let radius = ((right - left).min(bottom - top) / 4.).floor().max(1.);
        let half = stroke(1) / 2.;
        for y in top as i32..bottom as i32 {
            let mut start = None;
            let mut end = 0;
            for x in left as i32..right as i32 {
                let u = (x as f32 + 0.5 - cx) * sx;
                let v = (y as f32 + 0.5 - cy) * sy;
                let horizontal = u >= radius && v >= -half && v < half;
                let vertical = v >= radius && u >= -half && u < half;
                let curve = u < radius
                    && v < radius
                    && ((u - radius).hypot(v - radius) - radius).abs() < half;
                if horizontal || vertical || curve {
                    start.get_or_insert(x);
                    end = x + 1;
                }
            }
            if let Some(start) = start {
                rects.push(Bounds::new(
                    point(px(start as f32 / scale), px(y as f32 / scale)),
                    size(px((end - start) as f32 / scale), px(1. / scale)),
                ));
            }
        }
        return Some(rects);
    }
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
#[path = "../../tests/unit/custom_glyph/box_drawing.rs"]
mod tests;
