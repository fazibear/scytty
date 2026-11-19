use super::*;
use core::prelude::v1::test;
#[test]
fn tab_overflow_tracks_clipped_sides_and_nearest_hidden_tab() {
    let viewport = Bounds::new(point(px(50.), px(10.)), size(px(100.), px(28.)));
    let tabs = || {
        [40., 70., 30., 60.]
            .into_iter()
            .scan(50., |x, width| {
                let bounds = Bounds::new(point(px(*x), px(10.)), size(px(width), px(28.)));
                *x += width;
                Some(bounds)
            })
            .enumerate()
    };
    assert_eq!(hidden_tabs(viewport, px(0.), tabs()), (None, Some(1)));
    assert_eq!(hidden_tabs(viewport, px(-40.), tabs()), (Some(0), Some(3)));
    assert_eq!(hidden_tabs(viewport, px(-70.), tabs()), (Some(1), Some(3)));
    assert_eq!(hidden_tabs(viewport, px(-100.), tabs()), (Some(1), None));
    assert_eq!(hidden_tabs(viewport, px(0.), tabs().take(1)), (None, None));
    assert_eq!(
        hidden_tabs(viewport, px(0.), std::iter::empty()),
        (None, None)
    );
    let wider = Bounds::new(viewport.origin, size(px(200.), viewport.size.height));
    assert_eq!(hidden_tabs(wider, px(0.), tabs()), (None, None));
    assert_eq!(
        hidden_tabs(viewport, px(-0.1), tabs().take(1)),
        (None, None)
    );
}

#[test]
fn message_rows_preserve_unicode_and_blank_lines() {
    assert_eq!(
        message_line_rows("hello 世界", [6].into_iter()),
        ["hello", "世界"]
    );
    assert_eq!(message_line_rows("", std::iter::empty()), [""]);
    assert_eq!(message_line_rows("short", std::iter::empty()), ["short"]);
}
