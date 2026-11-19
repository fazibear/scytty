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
fn selected_tab_keeps_neighboring_tabs_visible() {
    let bounds = |x, width| Bounds::new(point(px(x), px(0.)), size(px(width), px(28.)));
    let viewport = bounds(50., 120.);
    // Active is the middle of the three 40px tabs, initially at the right edge.
    assert_eq!(
        tab_group_offset(viewport, px(0.), bounds(130., 40.), bounds(210., 40.)),
        px(-80.)
    );
    // The same group at the left edge must reveal its predecessor.
    assert_eq!(
        tab_group_offset(viewport, px(-120.), bounds(130., 40.), bounds(210., 40.)),
        px(-80.)
    );
    assert_eq!(
        tab_group_offset(viewport, px(-80.), bounds(130., 40.), bounds(210., 40.)),
        px(-80.)
    );
    // First/last selected tabs only need their one existing neighbor.
    assert_eq!(
        tab_group_offset(viewport, px(-40.), bounds(50., 40.), bounds(90., 40.)),
        px(0.)
    );
    assert_eq!(
        tab_group_offset(viewport, px(0.), bounds(170., 40.), bounds(210., 40.)),
        px(-80.)
    );
    // A stale pre-resize layout must not alternate between opposite edges.
    assert_eq!(
        tab_group_offset(viewport, px(-40.), bounds(50., 80.), bounds(210., 80.)),
        px(-40.)
    );
}

#[test]
fn bar_and_menu_icons_use_shared_scaling_runs_and_preserve_colors() {
    for color in [Some(rgb(0xcba6f7)), Some(rgba(0))] {
        assert_eq!(
            status_runs(vec![(" New tab".into(), color)]),
            vec![("".into(), color, true), ("New tab".into(), color, false)]
        );
        assert_eq!(
            status_runs(vec![(" ".into(), color)]),
            vec![("".into(), color, true)]
        );
        assert_eq!(
            status_runs(vec![(" ".into(), color)]),
            vec![("".into(), color, true)]
        );
        assert_eq!(
            status_runs(vec![("".into(), color)]),
            vec![("".into(), color, false)]
        );
    }
}

#[test]
fn menu_fits_longest_label_and_keeps_both_padding_margins() {
    assert_eq!(
        menu_width([px(40.), px(250.), px(90.)].into_iter(), px(800.)),
        px(294.)
    );
    assert_eq!(menu_width([px(250.)].into_iter(), px(200.)), px(200.));
    assert_eq!(menu_width([px(40.)].into_iter(), px(800.)), px(84.));
    assert_eq!(menu_width(std::iter::empty(), px(800.)), px(44.));
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
