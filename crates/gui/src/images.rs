use anyhow::Result;
use ghostty_vt::{Terminal, kitty::graphics::ImageFormat, render::ImagePlacement};
use gpui::{Bounds, ContentMask, Pixels, RenderImage, Window, point, px, size};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, OnceLock},
};

pub fn error_reaper() -> Arc<RenderImage> {
    static IMAGE: OnceLock<Arc<RenderImage>> = OnceLock::new();
    IMAGE
        .get_or_init(|| {
            render_image(
                image::load_from_memory(include_bytes!("../../../assets/error-reaper.png"))
                    .expect("bundled error reaper is a valid PNG")
                    .into_rgba8(),
            )
        })
        .clone()
}

#[derive(Default)]
pub struct Images {
    textures: HashMap<u64, Arc<RenderImage>>,
}

#[derive(Clone)]
pub struct Placement {
    image: Arc<RenderImage>,
    bounds: Bounds<Pixels>,
    clip: Bounds<Pixels>,
    pub z: i32,
    image_id: u32,
    placement_id: u32,
}
impl Placement {
    pub fn paint(&self, window: &mut Window) -> Result<()> {
        window.with_content_mask(Some(ContentMask { bounds: self.clip }), |window| {
            window.paint_image(
                self.bounds,
                Default::default(),
                self.image.clone(),
                0,
                false,
            )
        })
    }
}
impl Images {
    pub fn prepare(
        &mut self,
        terminal: &Terminal,
        placements: &[ImagePlacement],
        viewport: Bounds<Pixels>,
        cell: gpui::Size<Pixels>,
    ) -> Result<Vec<Placement>> {
        let graphics = terminal.kitty_graphics()?;
        let mut visible = HashSet::new();
        let mut result = Vec::new();
        let scale_x = f32::from(cell.width)
            / (terminal.width_px()? / u32::from(terminal.cols()?)).max(1) as f32;
        let scale_y = f32::from(cell.height)
            / (terminal.height_px()? / u32::from(terminal.rows()?)).max(1) as f32;
        for placement in placements {
            let image_id = placement.image_id;
            let Some(image) = graphics.image(image_id) else {
                continue;
            };
            let clip = Bounds::new(
                viewport.origin
                    + point(
                        cell.width * placement.viewport_col as f32
                            + px(placement.cell_offset_x as f32 * scale_x),
                        cell.height * placement.viewport_row as f32
                            + px(placement.cell_offset_y as f32 * scale_y),
                    ),
                size(
                    px(placement.pixel_width as f32 * scale_x),
                    px(placement.pixel_height as f32 * scale_y),
                ),
            );
            if !clip.intersects(&viewport) {
                continue;
            }
            let generation = placement.image_generation;
            if let std::collections::hash_map::Entry::Vacant(entry) =
                self.textures.entry(generation)
            {
                let Some(pixels) = rgba_pixels(
                    image.width()?,
                    image.height()?,
                    image.format()?,
                    image.data()?,
                ) else {
                    continue;
                };
                entry.insert(render_image(pixels));
            }
            visible.insert(generation);
            if placement.source_width == 0 || placement.source_height == 0 {
                continue;
            }
            let sx = clip.size.width / placement.source_width as f32;
            let sy = clip.size.height / placement.source_height as f32;
            result.push(Placement {
                image: self.textures[&generation].clone(),
                bounds: Bounds::new(
                    clip.origin
                        - point(
                            sx * placement.source_x as f32,
                            sy * placement.source_y as f32,
                        ),
                    size(sx * image.width()? as f32, sy * image.height()? as f32),
                ),
                clip,
                z: placement.z,
                image_id,
                placement_id: placement.placement_id,
            });
        }
        self.textures
            .retain(|generation, _| visible.contains(generation));
        result.sort_by_key(|p| (p.z, p.image_id, p.placement_id));
        Ok(result)
    }
}
fn render_image(mut pixels: image::RgbaImage) -> Arc<RenderImage> {
    // GPUI's renderer expects BGRA image frames.
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]))
}

fn rgba_pixels(
    width: u32,
    height: u32,
    format: ImageFormat,
    data: &[u8],
) -> Option<image::RgbaImage> {
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return None;
    }
    let channels = match format {
        ImageFormat::Rgb => 3,
        ImageFormat::Rgba => 4,
        ImageFormat::Gray => 1,
        ImageFormat::GrayAlpha => 2,
        _ => return None,
    };
    if data.len()
        != (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(channels)?
    {
        return None;
    }
    let pixels = data
        .chunks_exact(channels)
        .flat_map(|p| match format {
            ImageFormat::Rgb => [p[0], p[1], p[2], 255],
            ImageFormat::Rgba => [p[0], p[1], p[2], p[3]],
            ImageFormat::Gray => [p[0], p[0], p[0], 255],
            ImageFormat::GrayAlpha => [p[0], p[0], p[0], p[1]],
            _ => unreachable!(),
        })
        .collect();
    image::RgbaImage::from_raw(width, height, pixels)
}
#[cfg(test)]
#[path = "../tests/unit/images.rs"]
mod tests;
