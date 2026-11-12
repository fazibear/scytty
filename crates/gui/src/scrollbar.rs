use ghostty_vt::terminal::Scrollbar;
use gpui::{Bounds, Pixels, Point, Window, point, px, size};

pub const WIDTH: f32 = 10.;
pub const RESERVED_WIDTH: f32 = if crate::platform::OVERLAY_SCROLLBAR {
    0.
} else {
    WIDTH
};

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
        crate::platform::paint_scrollbar(
            window,
            self.track,
            self.thumb,
            self.max_offset > 0,
            opacity,
        );
    }
}

pub fn distance_from_bottom(scroll: Scrollbar) -> u64 {
    scroll
        .total
        .saturating_sub(scroll.len)
        .saturating_sub(scroll.offset)
}

pub fn opacity(elapsed: std::time::Duration) -> f32 {
    (1. - (elapsed.as_secs_f32() - 0.7) / 0.3).clamp(0., 1.)
}

#[cfg(test)]
#[path = "../tests/unit/scrollbar.rs"]
mod tests;
