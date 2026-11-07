use ghostty_vt::terminal::Scrollbar;
use gpui::{Bounds, Pixels, Point, Window, fill, point, px, rgb, rgba, size};

pub const WIDTH: f32 = 10.;
pub const RESERVED_WIDTH: f32 = if cfg!(target_os = "macos") { 0. } else { WIDTH };

pub struct ScrollBar {
    pub track: Bounds<Pixels>,
    pub thumb: Bounds<Pixels>,
    max_offset: u64,
}
impl ScrollBar {
    pub fn new(viewport: Bounds<Pixels>, scroll: Scrollbar) -> Self {
        let height = f32::from(viewport.size.height).max(0.);
        let thumb_height = (height * scroll.len as f32 / scroll.total.max(1) as f32)
            .max(24.)
            .min(height);
        let max_offset = scroll.total.saturating_sub(scroll.len);
        let top = if max_offset == 0 {
            0.
        } else {
            (height - thumb_height) * scroll.offset.min(max_offset) as f32 / max_offset as f32
        };
        let track = Bounds::new(
            point(viewport.right() - px(WIDTH), viewport.top()),
            size(px(WIDTH), px(height)),
        );
        let thumb = Bounds::new(
            track.origin + point(px(2.), px(top)),
            size(px(WIDTH - 4.), px(thumb_height)),
        );
        Self {
            track,
            thumb,
            max_offset,
        }
    }
    pub fn grab_offset(&self, position: Point<Pixels>) -> Pixels {
        if self.thumb.contains(&position) {
            position.y - self.thumb.top()
        } else {
            self.thumb.size.height / 2.
        }
    }
    pub fn row(&self, y: Pixels, grab_offset: Pixels) -> usize {
        let travel = f32::from(self.track.size.height - self.thumb.size.height);
        if travel <= 0. {
            return 0;
        }
        let fraction = (f32::from(y - self.track.top() - grab_offset) / travel).clamp(0., 1.);
        (fraction as f64 * self.max_offset as f64)
            .round()
            .min(usize::MAX as f64) as usize
    }
    pub fn paint(&self, window: &mut Window, opacity: f32) {
        if cfg!(target_os = "macos") {
            if self.max_offset == 0 {
                return;
            }
            let mut color = rgba(0xffffff70);
            color.a *= opacity;
            let mut thumb = fill(self.thumb, color);
            thumb.corner_radii = px((WIDTH - 4.) / 2.).into();
            window.paint_quad(thumb);
        } else {
            window.paint_quad(fill(self.track, rgb(0x171717)));
            window.paint_quad(fill(self.thumb, rgb(0x686868)));
        }
    }
}

pub fn opacity(elapsed: std::time::Duration) -> f32 {
    (1. - (elapsed.as_secs_f32() - 0.7) / 0.3).clamp(0., 1.)
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
