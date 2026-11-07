use anyhow::Result;
use eframe::egui::{self, Color32, ColorImage, Rect, TextureHandle, Vec2};
use ghostty_vt::{
    Terminal,
    kitty::graphics::{ImageFormat, PlacementIterator},
};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct Images {
    textures: HashMap<u64, TextureHandle>,
}

pub struct Placement {
    texture: egui::TextureId,
    rect: Rect,
    uv: Rect,
    z: i32,
    image_id: u32,
    placement_id: u32,
}
impl Placement {
    pub fn below_background(&self) -> bool {
        self.z < i32::MIN / 2
    }
    pub fn below_text(&self) -> bool {
        self.z < 0 && !self.below_background()
    }
    pub fn above_text(&self) -> bool {
        self.z >= 0
    }
    pub fn paint(&self, painter: &egui::Painter) {
        painter.image(self.texture, self.rect, self.uv, Color32::WHITE);
    }
}

impl Images {
    pub fn prepare(
        &mut self,
        terminal: &Terminal,
        ui: &egui::Ui,
        viewport: Rect,
        cell: Vec2,
    ) -> Result<Vec<Placement>> {
        let graphics = terminal.kitty_graphics()?;
        let mut iterator = PlacementIterator::new()?;
        let mut placements = iterator.update(&graphics)?;
        let mut visible = HashSet::new();
        let mut result = Vec::new();
        // Ghostty geometry is in the integral pixel units sent in Resize, not UI points.
        let scale = egui::vec2(
            cell.x / (terminal.width_px()? / u32::from(terminal.cols()?)).max(1) as f32,
            cell.y / (terminal.height_px()? / u32::from(terminal.rows()?)).max(1) as f32,
        );
        while let Some(placement) = placements.next() {
            let image_id = placement.image_id()?;
            let Some(image) = graphics.image(image_id) else {
                continue;
            };
            let Some(pos) = placement.viewport_pos(&image, terminal)? else {
                continue;
            };
            let size = placement.pixel_size(&image, terminal)?;
            let rect = Rect::from_min_size(
                viewport.min
                    + egui::vec2(pos.col as f32 * cell.x, pos.row as f32 * cell.y)
                    + egui::vec2(placement.x_offset()? as f32, placement.y_offset()? as f32)
                        * scale,
                egui::vec2(size.width as f32, size.height as f32) * scale,
            );
            if !rect.intersects(viewport) {
                continue;
            }
            let (width, height) = (image.width()? as usize, image.height()? as usize);
            let max_side = ui.input(|i| i.max_texture_side);
            if width == 0 || height == 0 || width > max_side || height > max_side {
                continue;
            }
            let generation = image.generation()?;
            if let std::collections::hash_map::Entry::Vacant(entry) =
                self.textures.entry(generation)
            {
                let Some(pixels) = color_image(width, height, image.format()?, image.data()?)
                else {
                    continue;
                };
                entry.insert(ui.ctx().load_texture(
                    format!("kitty-{generation}"),
                    pixels,
                    egui::TextureOptions::LINEAR,
                ));
            }
            visible.insert(generation);
            let source = placement.source_rect(&image)?;
            result.push(Placement {
                texture: self.textures[&generation].id(),
                rect,
                uv: Rect::from_min_max(
                    egui::pos2(
                        source.x as f32 / width as f32,
                        source.y as f32 / height as f32,
                    ),
                    egui::pos2(
                        (source.x + source.width) as f32 / width as f32,
                        (source.y + source.height) as f32 / height as f32,
                    ),
                ),
                z: placement.z()?,
                image_id,
                placement_id: placement.placement_id()?,
            });
        }
        self.textures
            .retain(|generation, _| visible.contains(generation));
        result.sort_by_key(|p| (p.z, p.image_id, p.placement_id));
        Ok(result)
    }
}

fn color_image(
    width: usize,
    height: usize,
    format: ImageFormat,
    data: &[u8],
) -> Option<ColorImage> {
    let channels = match format {
        ImageFormat::Rgb => 3,
        ImageFormat::Rgba => 4,
        ImageFormat::Gray => 1,
        ImageFormat::GrayAlpha => 2,
        _ => return None,
    };
    if data.len() != width.checked_mul(height)?.checked_mul(channels)? {
        return None;
    }
    let pixels = data
        .chunks_exact(channels)
        .map(|p| match format {
            ImageFormat::Rgb => Color32::from_rgb(p[0], p[1], p[2]),
            ImageFormat::Rgba => Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]),
            ImageFormat::Gray => Color32::from_gray(p[0]),
            ImageFormat::GrayAlpha => Color32::from_rgba_unmultiplied(p[0], p[0], p[0], p[1]),
            _ => unreachable!(),
        })
        .collect();
    Some(ColorImage::new([width, height], pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_formats_are_validated() {
        assert_eq!(
            color_image(1, 1, ImageFormat::Rgb, &[255, 0, 0])
                .unwrap()
                .pixels,
            [Color32::RED]
        );
        assert_eq!(
            color_image(1, 1, ImageFormat::Rgba, &[0, 0, 0, 0])
                .unwrap()
                .pixels,
            [Color32::TRANSPARENT]
        );
        assert_eq!(
            color_image(1, 1, ImageFormat::Gray, &[42]).unwrap().pixels,
            [Color32::from_gray(42)]
        );
        assert!(color_image(2, 1, ImageFormat::Rgba, &[0; 4]).is_none());
    }

    #[test]
    fn textures_follow_placements_and_deletion() -> Result<()> {
        let mut terminal = Terminal::new(Default::default())?;
        scytty_common::images::configure(&mut terminal)?;
        terminal.resize(80, 24, 8, 16)?;
        terminal.vt_write(b"\x1b_Ga=T,f=24,s=1,v=1,i=1,c=2,r=2;/wAA\x1b\\");
        let ctx = egui::Context::default();
        let mut images = Images::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 384.0));
            let placements = images
                .prepare(&terminal, ui, rect, egui::vec2(8.0, 16.0))
                .unwrap();
            assert_eq!(placements.len(), 1);
            assert_eq!(placements[0].rect.size(), egui::vec2(16.0, 32.0));
            assert_eq!(images.textures.len(), 1);
            terminal.vt_write(b"\x1b_Ga=d,d=A\x1b\\");
            assert!(
                images
                    .prepare(&terminal, ui, rect, egui::vec2(8.0, 16.0))
                    .unwrap()
                    .is_empty()
            );
            assert!(images.textures.is_empty());
        });
        output.textures_delta.clear();
        Ok(())
    }
}
