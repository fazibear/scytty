use super::*;
#[test]
fn bundled_error_reaper_loads_and_is_cached() {
    assert!(Arc::ptr_eq(&error_reaper(), &error_reaper()));
}

#[test]
fn synchronized_redraw_keeps_images_and_text_in_the_same_frame() -> Result<()> {
    let mut terminal = Terminal::new(ghostty_vt::TerminalOptions::default())?;
    crate::resize_terminal(&mut terminal, 80, 24, 10, 20)?;
    let mut tab = crate::Tab::new(terminal)?;
    let bounds = Bounds::new(point(px(0.), px(32.)), size(px(800.), px(480.)));
    let cell = size(px(10.), px(20.));
    tab.terminal
        .vt_write(b"A\x1b_Ga=T,q=2,f=24,s=1,v=1,i=1,c=2,r=2;/wAA\x1b\\");
    tab.prepare_frame(bounds, cell)?;
    assert_eq!(tab.placements.len(), 1);
    let old = tab.placements[0].clone();
    let old_cells = tab.cells.clone();
    // Pi redraws delete/recreate image placements across many output chunks.
    for chunk in [
        b"\x1b[?2026h".as_slice(),
        b"\x1b_Ga=d,d=A,q=2\x1b\\",
        b"\x1b[3;1H\x1b_Ga=T,q=2,f=24,s=1,v=1,i=2,c=2,r=2;",
        b"AAD/\x1b\\B",
        b"\x1b[?2026",
    ] {
        tab.terminal.vt_write(chunk);
        tab.prepare_frame(bounds, cell)?;
        assert!(tab.cells == old_cells);
        assert_eq!(tab.placements.len(), 1);
        assert_eq!(tab.placements[0].image_id, 1);
        assert!(Arc::ptr_eq(&tab.placements[0].image, &old.image));
        assert_eq!(tab.placements[0].bounds, old.bounds);
    }
    tab.terminal.vt_write(b"l");
    tab.prepare_frame(bounds, cell)?;
    assert!(tab.cells != old_cells);
    assert_eq!(tab.placements.len(), 1);
    assert_eq!(tab.placements[0].image_id, 2);
    assert!(!Arc::ptr_eq(&tab.placements[0].image, &old.image));
    // Placement-only updates still commit when text cells are clean.
    let previous = tab.placements[0].clone();
    tab.terminal
        .vt_write(b"\x1b[6;1H\x1b_Ga=p,i=2,p=7,q=2,c=2,r=2\x1b\\");
    tab.prepare_frame(bounds, cell)?;
    assert!(
        tab.placements
            .iter()
            .any(|placement| placement.placement_id == 7 && placement.bounds != previous.bounds)
    );
    Ok(())
}

#[test]
fn pixel_formats_are_validated() {
    assert_eq!(
        rgba_pixels(1, 1, ImageFormat::GrayAlpha, &[40, 128])
            .unwrap()
            .as_raw(),
        &[40, 40, 40, 128]
    );
    assert_eq!(
        rgba_pixels(1, 1, ImageFormat::Rgb, &[1, 2, 3])
            .unwrap()
            .as_raw(),
        &[1, 2, 3, 255]
    );
    assert!(rgba_pixels(2, 1, ImageFormat::Rgba, &[0; 4]).is_none());
    assert!(rgba_pixels(4097, 1, ImageFormat::Gray, &[0; 4097]).is_none());
}
