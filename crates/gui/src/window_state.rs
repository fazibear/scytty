use anyhow::{Context, Result};
use gpui::{Bounds, WindowBounds, point, px, size};
use std::path::PathBuf;

fn path() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|home| PathBuf::from(home).join(".local/state"))
        })
        .map(|base| base.join("scytty/window"))
}
pub fn load(cx: &gpui::App) -> Option<WindowBounds> {
    let bounds = parse(&std::fs::read_to_string(path()?).ok()?)?;
    cx.displays()
        .iter()
        .any(|display| display.bounds().intersects(&bounds.get_bounds()))
        .then_some(bounds)
}
pub fn save(bounds: WindowBounds) -> Result<()> {
    let path = path().context("HOME, USERPROFILE or XDG_STATE_HOME must be set")?;
    std::fs::create_dir_all(path.parent().context("invalid state path")?)?;
    let mode = match bounds {
        WindowBounds::Windowed(_) => "window",
        WindowBounds::Maximized(_) => "maximized",
        WindowBounds::Fullscreen(_) => "fullscreen",
    };
    let bounds = bounds.get_bounds();
    std::fs::write(
        path,
        format!(
            "{mode} {} {} {} {}\n",
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height)
        ),
    )?;
    Ok(())
}
fn parse(source: &str) -> Option<WindowBounds> {
    let mut words = source.split_whitespace();
    let mode = words.next()?;
    let values: Vec<f32> = words.map(str::parse).collect::<Result<_, _>>().ok()?;
    let [x, y, width, height] = *values.as_slice() else {
        return None;
    };
    if !values.iter().all(|n| n.is_finite())
        || !(200.0..=16384.0).contains(&width)
        || !(100.0..=16384.0).contains(&height)
    {
        return None;
    }
    let bounds = Bounds::new(point(px(x), px(y)), size(px(width), px(height)));
    match mode {
        "window" => Some(WindowBounds::Windowed(bounds)),
        "maximized" => Some(WindowBounds::Maximized(bounds)),
        "fullscreen" => Some(WindowBounds::Fullscreen(bounds)),
        _ => None,
    }
}
#[cfg(test)]
#[path = "../tests/unit/window_state.rs"]
mod tests;
