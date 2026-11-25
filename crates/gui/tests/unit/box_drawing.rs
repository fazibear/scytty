use super::*;

fn same_pixel(a: Pixels, b: Pixels) {
    assert!((a - b).abs() < px(0.0001), "{a:?} != {b:?}");
}

#[test]
fn maps_solid_border_connections_and_preserves_other_glyphs() {
    assert_eq!(supported("│"), cfg!(target_os = "windows"));
    assert_eq!(arms("│"), Some([0, 0, 1, 1]));
    assert_eq!(arms("┌"), Some([0, 1, 0, 1]));
    assert_eq!(arms("┼"), Some([1, 1, 1, 1]));
    assert_eq!(arms("╋"), Some([2, 2, 2, 2]));
    assert_eq!(arms("┢"), Some([0, 2, 1, 2]));
    assert_eq!(arms("╾"), Some([2, 1, 0, 0]));
    for text in ["", "a", "||", "│\u{301}", "╱", "╭", "═", "█"] {
        assert!(!supported(text));
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
