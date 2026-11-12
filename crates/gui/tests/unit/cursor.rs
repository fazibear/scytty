use super::*;
#[test]
fn cursor_geometry_and_blink() {
    let cell = size(px(8.), px(20.));
    assert_eq!(
        rectangles(CursorVisualStyle::Bar, cell, px(2.))[0].size,
        size(px(2.), px(20.))
    );
    let underline = rectangles(CursorVisualStyle::Underline, cell, px(2.))[0];
    assert_eq!(underline.origin.y, px(18.));
    assert_eq!(underline.size, size(px(8.), px(2.)));
    assert_eq!(
        rectangles(CursorVisualStyle::Block, cell, px(2.))[0].size,
        cell
    );
    assert_eq!(
        rectangles(CursorVisualStyle::BlockHollow, cell, px(2.)).len(),
        4
    );
    for (ms, expected) in [
        (0, true),
        (499, true),
        (500, false),
        (999, false),
        (1000, true),
    ] {
        assert_eq!(visible(true, Duration::from_millis(ms)), expected);
        assert!(visible(false, Duration::from_millis(ms)));
    }
}
