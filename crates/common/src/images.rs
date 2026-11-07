use ghostty_vt::{
    Terminal,
    alloc::{Allocator, Bytes},
    kitty::graphics::{DecodePng, DecodedImage, set_png_decoder},
};

pub const STORAGE_LIMIT: u64 = 16 * 1024 * 1024;
const MAX_DIMENSION: u32 = 4096;

/// Apply identical graphics policy on the server and the rendering thread.
pub fn configure(terminal: &mut Terminal) -> anyhow::Result<()> {
    use anyhow::Context;
    set_png_decoder(Some(Box::new(PngDecoder))).context("install PNG decoder")?;
    terminal
        .set_kitty_image_storage_limit(STORAGE_LIMIT)
        .context("set image storage limit")?;
    terminal
        .set_apc_max_bytes_kitty(Some(STORAGE_LIMIT as usize))
        .context("set image APC limit")?;
    // PTY output is untrusted; never read files or shared memory named by applications.
    terminal
        .set_kitty_image_from_file_allowed(false)
        .context("disable image files")?;
    // Temporary files are disabled by Ghostty's default direct-only policy.
    // Its Rust 0.8 temp-file setter has the wrong ABI, so do not call it.
    terminal
        .set_kitty_image_from_shared_mem_allowed(false)
        .context("disable image shared memory")?;
    Ok(())
}

struct PngDecoder;
impl DecodePng for PngDecoder {
    fn decode_png(&mut self, alloc: &Allocator, data: &[u8]) -> Option<DecodedImage> {
        let mut decoder = png::Decoder::new_with_limits(
            std::io::Cursor::new(data),
            png::Limits {
                bytes: STORAGE_LIMIT as usize,
            },
        );
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().ok()?;
        let (width, height) = (reader.info().width, reader.info().height);
        let size = rgba_size(width, height)?;
        let mut pixels = vec![0; reader.output_buffer_size()?.min(STORAGE_LIMIT as usize)];
        let info = reader.next_frame(&mut pixels).ok()?;
        let mut rgba = Bytes::new_with_alloc(alloc, size).ok()?;
        let channels = info.color_type.samples();
        for (src, dst) in pixels[..info.buffer_size()]
            .chunks_exact(channels)
            .zip(rgba.as_chunks_mut::<4>().0)
        {
            match info.color_type {
                png::ColorType::Rgb => dst.copy_from_slice(&[src[0], src[1], src[2], 255]),
                png::ColorType::Rgba => dst.copy_from_slice(src),
                png::ColorType::Grayscale => dst.copy_from_slice(&[src[0], src[0], src[0], 255]),
                png::ColorType::GrayscaleAlpha => {
                    dst.copy_from_slice(&[src[0], src[0], src[0], src[1]])
                }
                png::ColorType::Indexed => return None,
            }
        }
        reader.finish().ok()?;
        Some(DecodedImage {
            width,
            height,
            data: rgba,
        })
    }
}

fn rgba_size(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return None;
    }
    let size = u64::from(width) * u64::from(height) * 4;
    (size <= STORAGE_LIMIT).then_some(size as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_survive_snapshots_and_reject_file_media() -> anyhow::Result<()> {
        let mut terminal = Terminal::new(Default::default())?;
        configure(&mut terminal)?;
        terminal.resize(80, 24, 8, 16)?;
        terminal.vt_write(b"\x1b_Ga=T,f=24,s=1,v=1,i=7,c=2,r=2;/wAA\x1b\\");
        let mut bytes = Vec::new();
        ghostty_vt::snapshot::encode_to(&terminal, &mut bytes)?;
        let mut restored = ghostty_vt::snapshot::Decoder::from_slice(&bytes).decode()?;
        configure(&mut restored)?;
        let graphics = restored.kitty_graphics()?;
        assert_eq!(graphics.image(7).unwrap().data()?, [255, 0, 0]);
        let mut iter = ghostty_vt::kitty::graphics::PlacementIterator::new()?;
        let mut placements = iter.update(&graphics)?;
        assert_eq!(placements.next().unwrap().image_id()?, 7);
        for medium in ['f', 't', 's'] {
            restored.vt_write(
                format!("\x1b_Ga=q,t={medium},f=24,s=1,v=1,i=8;L3RtcC9ub3Qtc2FmZQ==\x1b\\")
                    .as_bytes(),
            );
            let replies: Vec<_> = restored
                .events()
                .filter_map(|event| match event {
                    ghostty_vt::Event::PtyWrite(bytes) => Some(bytes),
                    _ => None,
                })
                .collect();
            assert!(
                replies
                    .iter()
                    .any(|reply| String::from_utf8_lossy(reply).contains("unsupported medium")),
                "{medium}: {replies:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn png_formats_and_limits() {
        for (color, bytes, expected) in [
            (png::ColorType::Rgb, vec![255, 0, 0], [255, 0, 0, 255]),
            (png::ColorType::Rgba, vec![255, 0, 0, 128], [255, 0, 0, 128]),
            (png::ColorType::Grayscale, vec![42], [42, 42, 42, 255]),
            (
                png::ColorType::GrayscaleAlpha,
                vec![42, 128],
                [42, 42, 42, 128],
            ),
        ] {
            let mut data = Vec::new();
            let mut encoder = png::Encoder::new(&mut data, 1, 1);
            encoder.set_color(color);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&bytes)
                .unwrap();
            use base64::Engine;
            let mut terminal = Terminal::new(Default::default()).unwrap();
            configure(&mut terminal).unwrap();
            let payload = base64::engine::general_purpose::STANDARD.encode(&data);
            terminal.vt_write(format!("\x1b_Ga=T,f=100,i=1;{payload}\x1b\\").as_bytes());
            let graphics = terminal.kitty_graphics().unwrap();
            let image = graphics.image(1).unwrap();
            assert_eq!(image.data().unwrap(), &expected);
        }
        assert_eq!(rgba_size(4096, 1024), Some(STORAGE_LIMIT as usize));
        assert_eq!(rgba_size(4096, 4096), None);
        assert_eq!(rgba_size(u32::MAX, u32::MAX), None);
        assert_eq!(rgba_size(0, 1), None);
        let mut terminal = Terminal::new(Default::default()).unwrap();
        configure(&mut terminal).unwrap();
        terminal.vt_write(b"\x1b_Ga=T,f=100,i=1;bm90IFBORw==\x1b\\");
        assert!(terminal.kitty_graphics().unwrap().image(1).is_none());
    }
}
