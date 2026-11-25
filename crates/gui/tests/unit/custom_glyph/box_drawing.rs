use super::*;

fn same_pixel(a: Pixels, b: Pixels) {
    assert!((a - b).abs() < px(0.0001), "{a:?} != {b:?}");
}

#[test]
fn entire_box_drawing_range_uses_the_windows_renderer() {
    for code in 0x2500..=0x257f {
        let text = char::from_u32(code).unwrap().to_string();
        assert!(arms(&text).is_some(), "missing {text}");
        for scale in [1., 1.25, 1.5, 1.75, 2.] {
            let rects = rectangles(
                &text,
                point(px(2.3), px(3.7)),
                size(px(9.3), px(18.7)),
                scale,
            )
            .unwrap();
            assert!(!rects.is_empty(), "empty {text}");
            for rect in rects {
                assert!(rect.size.width > px(0.) && rect.size.height > px(0.));
                for edge in [rect.left(), rect.right(), rect.top(), rect.bottom()] {
                    let physical = f32::from(edge) * scale;
                    assert!((physical - physical.round()).abs() < 0.0001);
                }
            }
        }
    }
}

#[test]
fn double_and_mixed_junctions_match_neighboring_rails() {
    let cell = size(px(9.3), px(18.7));
    let origin = point(px(2.3), px(3.7));
    for scale in [1., 1.25, 1.5, 1.75, 2.] {
        let left = (f32::from(origin.x) * scale).round() as i32;
        let top = (f32::from(origin.y) * scale).round() as i32;
        let right = (f32::from(origin.x + cell.width) * scale).round() as i32;
        let bottom = (f32::from(origin.y + cell.height) * scale).round() as i32;
        let edge_pixels = |text: &str, direction| {
            let rects = rectangles(text, origin, cell, scale).unwrap();
            let mut pixels = Vec::new();
            for y in top..bottom {
                for x in left..right {
                    let edge = match direction {
                        0 => x == left,
                        1 => x == right - 1,
                        2 => y == top,
                        _ => y == bottom - 1,
                    };
                    if edge
                        && rects.iter().any(|rect| {
                            let px = x as f32 + 0.5;
                            let py = y as f32 + 0.5;
                            px >= f32::from(rect.left()) * scale
                                && px < f32::from(rect.right()) * scale
                                && py >= f32::from(rect.top()) * scale
                                && py < f32::from(rect.bottom()) * scale
                        })
                    {
                        pixels.push((x, y));
                    }
                }
            }
            pixels
        };
        for code in 0x2550..=0x256c {
            let text = char::from_u32(code).unwrap().to_string();
            for (direction, weight) in arms(&text).unwrap().into_iter().enumerate() {
                let actual = edge_pixels(&text, direction);
                if weight == 0 {
                    assert!(actual.is_empty(), "unexpected edge on {text}");
                } else {
                    let straight = match (direction < 2, weight) {
                        (true, 3) => "═",
                        (false, 3) => "║",
                        (true, _) => "─",
                        (false, _) => "│",
                    };
                    assert_eq!(
                        actual,
                        edge_pixels(straight, direction),
                        "{text} direction {direction} at {scale}"
                    );
                }
            }
        }
    }
}

#[test]
fn maps_solid_border_connections_and_preserves_other_glyphs() {
    assert_eq!(arms("│"), Some([0, 0, 1, 1]));
    assert_eq!(arms("┌"), Some([0, 1, 0, 1]));
    assert_eq!(arms("┼"), Some([1, 1, 1, 1]));
    assert_eq!(arms("╋"), Some([2, 2, 2, 2]));
    assert_eq!(arms("┢"), Some([0, 2, 1, 2]));
    assert_eq!(arms("╾"), Some([2, 1, 0, 0]));
    for text in ["", "a", "||", "│\u{301}", "█"] {
        assert!(arms(text).is_none());
    }
}

#[test]
fn borders_connect_at_physical_pixel_edges() {
    let cell = size(px(9.3), px(18.7));
    let origin = point(px(2.3), px(3.7));
    for scale in [1., 1.25, 1.5, 1.75, 2.] {
        for (horizontal, vertical, corner) in [("─", "│", "┌"), ("━", "┃", "┏")] {
            let here = rectangles(vertical, origin, cell, scale).unwrap();
            let below =
                rectangles(vertical, origin + point(px(0.), cell.height), cell, scale).unwrap();
            same_pixel(here[1].bottom(), below[0].top());
            same_pixel(here[1].left(), below[0].left());
            same_pixel(here[1].right(), below[0].right());
            let corner = rectangles(corner, origin, cell, scale).unwrap();
            let next =
                rectangles(horizontal, origin + point(cell.width, px(0.)), cell, scale).unwrap();
            same_pixel(corner[0].right(), next[0].left());
            same_pixel(corner[0].top(), next[0].top());
            same_pixel(corner[0].bottom(), next[0].bottom());
            for rect in here.iter().chain(&below).chain(&corner).chain(&next) {
                for value in [rect.left(), rect.right(), rect.top(), rect.bottom()] {
                    let physical = f32::from(value) * scale;
                    assert!((physical - physical.round()).abs() < 0.0001);
                }
            }
        }
    }
}

#[test]
fn rounded_corners_match_straight_border_endpoints() {
    use std::collections::BTreeSet;
    let origin = point(px(2.3), px(3.7));
    for scale in [1., 1.25, 1.5, 1.75, 2.] {
        for cell in [size(px(9.3), px(18.7)), size(px(16.), px(32.))] {
            let left = (f32::from(origin.x) * scale).round() as i32;
            let top = (f32::from(origin.y) * scale).round() as i32;
            let right = (f32::from(origin.x + cell.width) * scale).round() as i32;
            let bottom = (f32::from(origin.y + cell.height) * scale).round() as i32;
            let pixels = |text| {
                let mut pixels = BTreeSet::new();
                for rect in rectangles(text, origin, cell, scale).unwrap() {
                    for y in (f32::from(rect.top()) * scale).round() as i32
                        ..(f32::from(rect.bottom()) * scale).round() as i32
                    {
                        for x in (f32::from(rect.left()) * scale).round() as i32
                            ..(f32::from(rect.right()) * scale).round() as i32
                        {
                            assert!(x >= left && x < right && y >= top && y < bottom);
                            pixels.insert((x, y));
                        }
                    }
                }
                pixels
            };
            for (rounded, square) in [("╭", "┌"), ("╮", "┐"), ("╰", "└"), ("╯", "┘")]
            {
                let curved = pixels(rounded);
                let square = pixels(square);
                assert!(!curved.is_empty());
                assert_ne!(curved, square, "preserve rounded appearance");
                let edges = |pixels: &BTreeSet<(i32, i32)>| {
                    pixels
                        .iter()
                        .copied()
                        .filter(|&(x, y)| {
                            x == left || x == right - 1 || y == top || y == bottom - 1
                        })
                        .collect::<BTreeSet<_>>()
                };
                assert_eq!(edges(&curved), edges(&square), "{rounded} at {scale}");
                let mut remaining = curved.clone();
                let mut pending = vec![*remaining.first().unwrap()];
                while let Some((x, y)) = pending.pop() {
                    if remaining.remove(&(x, y)) {
                        for dx in -1..=1 {
                            for dy in -1..=1 {
                                if remaining.contains(&(x + dx, y + dy)) {
                                    pending.push((x + dx, y + dy));
                                }
                            }
                        }
                    }
                }
                assert!(remaining.is_empty(), "disconnected {rounded} at {scale}");
            }
        }
    }
}
