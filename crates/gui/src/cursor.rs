use crate::tab::Tab;
use ghostty_vt::render::CursorVisualStyle;
use gpui::{
    App, Bounds, ContentMask, Font, Pixels, Point, Size, TextRun, Window, fill, point, px, size,
};
use std::time::Duration;

fn visible(blinking: bool, elapsed: Duration) -> bool {
    !blinking || elapsed.as_millis() % 1000 < 500
}

fn rectangles(
    style: CursorVisualStyle,
    cell: Size<Pixels>,
    thickness: Pixels,
) -> Vec<Bounds<Pixels>> {
    let thickness = thickness.min(cell.width).min(cell.height);
    match style {
        CursorVisualStyle::Bar => vec![Bounds::new(
            point(px(0.), px(0.)),
            size(thickness, cell.height),
        )],
        CursorVisualStyle::Underline => vec![Bounds::new(
            point(px(0.), cell.height - thickness),
            size(cell.width, thickness),
        )],
        CursorVisualStyle::BlockHollow => vec![
            Bounds::new(point(px(0.), px(0.)), size(cell.width, thickness)),
            Bounds::new(
                point(px(0.), cell.height - thickness),
                size(cell.width, thickness),
            ),
            Bounds::new(point(px(0.), px(0.)), size(thickness, cell.height)),
            Bounds::new(
                point(cell.width - thickness, px(0.)),
                size(thickness, cell.height),
            ),
        ],
        _ => vec![Bounds::new(point(px(0.), px(0.)), cell)],
    }
}

pub(super) fn paint(
    tab: &Tab,
    origin: Point<Pixels>,
    cell: Size<Pixels>,
    font: &Font,
    font_size: Pixels,
    window: &mut Window,
    cx: &mut App,
) -> anyhow::Result<()> {
    let (Some((x, y)), Some(state)) = (tab.cursor, tab.cursor_state) else {
        return Ok(());
    };
    let active = window.is_window_active();
    if active && !visible(state.blinking, tab.cursor_since.elapsed()) {
        return Ok(());
    }
    let x =
        usize::from(x).saturating_sub(usize::from(state.viewport.is_some_and(|p| p.at_wide_tail)));
    let c = tab.cells.iter().find(|c| c.x == x && c.y == usize::from(y));
    let width = if c.is_some_and(|c| !c.narrow) {
        cell.width * 2.
    } else {
        cell.width
    };
    let bounds = Bounds::new(
        origin + point(cell.width * x as f32, cell.height * f32::from(y)),
        size(width, cell.height),
    );
    let style = if active {
        state.visual_style
    } else {
        CursorVisualStyle::BlockHollow
    };
    let thickness = px((2. * window.scale_factor()).round().max(1.) / window.scale_factor());
    for rect in rectangles(style, bounds.size, thickness) {
        window.paint_quad(fill(
            Bounds::new(bounds.origin + rect.origin, rect.size),
            tab.cursor_color,
        ));
    }
    // A solid cursor must not obscure the character beneath it.
    if style == CursorVisualStyle::Block
        && let Some(c) = c
        && !c.text.is_empty()
    {
        let color = tab.default_colors.map_or(gpui::rgb(0), |(_, bg)| {
            gpui::rgb((u32::from(bg.r) << 16) | (u32::from(bg.g) << 8) | u32::from(bg.b))
        });
        let run = TextRun {
            len: c.text.len(),
            font: font.clone(),
            color: color.into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window.text_system().shape_line(
            c.text.clone().into(),
            font_size,
            &[run],
            Some(cell.width),
        );
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            line.paint(bounds.origin, cell.height, window, cx)
        })?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/cursor.rs"]
mod tests;
