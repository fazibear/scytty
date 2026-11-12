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
            format!("\x1b_Ga=q,t={medium},f=24,s=1,v=1,i=8;L3RtcC9ub3Qtc2FmZQ==\x1b\\").as_bytes(),
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
