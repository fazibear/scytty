use super::*;
use gpui::{point, px, size};

#[test]
fn block_elements_cover_the_expected_cell_regions() {
    assert_eq!(block_rectangles("▀"), Some(vec![(0, 0, 8, 4)]));
    assert_eq!(block_rectangles("▄"), Some(vec![(0, 4, 8, 4)]));
    assert_eq!(block_rectangles("█"), Some(vec![(0, 0, 8, 8)]));
    assert_eq!(block_rectangles("▌"), Some(vec![(0, 0, 4, 8)]));
    assert_eq!(block_rectangles("▐"), Some(vec![(4, 0, 4, 8)]));
    for ch in '▁'..='▏' {
        assert!(block_rectangles(&ch.to_string()).is_some());
    }
    let expected_areas = [16, 16, 16, 48, 32, 48, 48, 16, 32, 48];
    for (ch, area) in ('▖'..='▟').zip(expected_areas) {
        let rects = block_rectangles(&ch.to_string()).unwrap();
        assert_eq!(rects.iter().map(|r| r.2 * r.3).sum::<u8>(), area);
        assert!(rects.iter().all(|r| r.0 + r.2 <= 8 && r.1 + r.3 <= 8));
    }
    for text in ["a", "//", "─", "░", "█\u{301}", ""] {
        assert!(block_rectangles(text).is_none());
    }
}

#[test]
fn block_edges_meet_at_fractional_windows_scale_factors() {
    let origin = point(px(2.3), px(3.7));
    let cell = size(px(9.3), px(18.7));
    for scale in [1., 1.25, 1.5, 1.75, 2.] {
        let full = block_bounds(origin, cell, (0, 0, 8, 8), scale);
        let upper = block_bounds(origin, cell, (0, 0, 8, 4), scale);
        let lower = block_bounds(origin, cell, (0, 4, 8, 4), scale);
        assert_eq!(upper.origin.y + upper.size.height, lower.origin.y);
        assert_eq!(upper.size.height + lower.size.height, full.size.height);
        let next = block_bounds(
            origin + point(cell.width, px(0.)),
            cell,
            (0, 0, 8, 8),
            scale,
        );
        assert_eq!(full.origin.x + full.size.width, next.origin.x);
        for value in [
            full.origin.x,
            full.origin.y,
            full.size.width,
            full.size.height,
        ] {
            let physical = f32::from(value) * scale;
            assert!((physical - physical.round()).abs() < 0.0001);
        }
    }
}
