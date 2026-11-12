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
#[path = "../tests/unit/images.rs"]
mod tests;
