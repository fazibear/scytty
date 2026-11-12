use super::*;
#[test]
fn output_at_bottom_does_not_count_as_scrolling() {
    for total in [100, 101, 1000] {
        assert_eq!(
            distance_from_bottom(Scrollbar {
                total,
                len: 24,
                offset: total - 24
            }),
            0
        );
    }
    assert_eq!(
        distance_from_bottom(Scrollbar {
            total: 100,
            len: 24,
            offset: 75
        }),
        1
    );
}
#[test]
fn fades_after_inactivity() {
    use std::time::Duration;
    assert_eq!(opacity(Duration::ZERO), 1.);
    assert_eq!(opacity(Duration::from_millis(700)), 1.);
    assert!((opacity(Duration::from_millis(850)) - 0.5).abs() < 0.001);
    assert_eq!(opacity(Duration::from_secs(1)), 0.);
    assert_eq!(opacity(Duration::from_secs(10)), 0.);
}
#[test]
fn thumb_position_and_drag_rows() {
    let viewport = Bounds::new(point(px(0.), px(32.)), size(px(800.), px(400.)));
    let bar = ScrollBar::new(
        viewport,
        Scrollbar {
            total: 1000,
            len: 100,
            offset: 900,
        },
    );
    assert_eq!(bar.track.right(), viewport.right());
    assert_eq!(bar.thumb.size.height, px(40.));
    assert_eq!(bar.thumb.bottom(), viewport.bottom());
    assert_eq!(bar.row(px(32.), px(0.)), 0);
    assert_eq!(bar.row(px(212.), px(0.)), 450);
    assert_eq!(bar.row(px(1000.), px(0.)), 900);
    assert_eq!(bar.row(px(-100.), px(0.)), 0);
    let position = bar.thumb.origin + point(px(1.), px(5.));
    assert_eq!(bar.grab_offset(position), px(5.));
    assert_eq!(bar.row(position.y, bar.grab_offset(position)), 900);
}
#[test]
fn no_history_or_tiny_viewport() {
    for height in [0., 10., 400.] {
        let bar = ScrollBar::new(
            Bounds::new(point(px(0.), px(0.)), size(px(800.), px(height))),
            Scrollbar {
                total: 24,
                len: 24,
                offset: 0,
            },
        );
        assert_eq!(bar.thumb.size.height, px(height));
        assert_eq!(bar.row(px(999.), px(0.)), 0);
    }
}
