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

fn graphics_tab() -> Result<crate::Tab> {
    let mut terminal = Terminal::new(ghostty_vt::TerminalOptions {
        cols: 12,
        rows: 8,
        ..Default::default()
    })?;
    crate::resize_terminal(&mut terminal, 12, 8, 10, 20)?;
    crate::Tab::new(terminal)
}
fn transmit(tab: &mut crate::Tab, options: &str) {
    use base64::Engine;
    let pixels = base64::engine::general_purpose::STANDARD.encode([255u8, 0, 0].repeat(16));
    tab.terminal
        .vt_write(format!("\x1b_Ga=T,q=2,f=24,s=4,v=4,{options};{pixels}\x1b\\").as_bytes());
}
fn frame(tab: &mut crate::Tab) -> Result<()> {
    tab.prepare_frame(
        Bounds::new(point(px(0.), px(0.)), size(px(120.), px(160.))),
        size(px(10.), px(20.)),
    )
}
fn placeholder(tab: &mut crate::Tab, row: char, col: char) {
    tab.terminal
        .vt_write(format!("\u{10eeee}{row}{col}\u{305}").as_bytes());
}

#[test]
fn unicode_placeholders_render_tiles_without_drawing_the_marker() -> Result<()> {
    let mut tab = graphics_tab()?;
    transmit(&mut tab, "i=1,p=1,U=1,c=2,r=2");
    frame(&mut tab)?;
    assert!(tab.placements.is_empty());
    tab.terminal.vt_write(b"\x1b[H\x1b[38;2;0;0;1;58;2;0;0;1m");
    placeholder(&mut tab, '\u{305}', '\u{305}');
    tab.terminal.vt_write("\u{10eeee}".as_bytes());
    tab.terminal.vt_write(b"\r\n");
    placeholder(&mut tab, '\u{30d}', '\u{305}');
    tab.terminal.vt_write("\u{10eeee}".as_bytes());
    frame(&mut tab)?;
    assert!(!tab.placements.is_empty());
    assert!(
        tab.placements
            .iter()
            .all(|p| p.image_id == 1 && p.placement_id == 1)
    );
    assert_eq!(
        tab.placements
            .iter()
            .map(|p| f32::from(p.clip.size.width) * f32::from(p.clip.size.height))
            .sum::<f32>(),
        400.,
        "{:?}",
        tab.placements.iter().map(|p| p.clip).collect::<Vec<_>>()
    );
    // Virtual placements preserve aspect ratio and center within their cell rectangle.
    assert!(
        tab.placements
            .iter()
            .all(|p| p.bounds == Bounds::new(point(px(0.), px(10.)), size(px(20.), px(20.))))
    );
    assert!(
        tab.cells
            .iter()
            .all(|cell| !cell.text.contains('\u{10eeee}'))
    );
    assert!(
        tab.placements
            .iter()
            .all(|p| Arc::ptr_eq(&p.image, &tab.placements[0].image))
    );
    let mut bytes = Vec::new();
    ghostty_vt::snapshot::encode_to(&tab.terminal, &mut bytes)?;
    let mut restored =
        crate::Tab::new(ghostty_vt::snapshot::Decoder::from_slice(&bytes).decode()?)?;
    frame(&mut restored)?;
    assert_eq!(restored.placements.len(), tab.placements.len());
    assert!(
        restored
            .placements
            .iter()
            .zip(&tab.placements)
            .all(|(a, b)| a.clip == b.clip && a.bounds == b.bounds)
    );
    tab.terminal.vt_write(b"\x1b[H\x1b[2J");
    frame(&mut tab)?;
    assert!(tab.placements.is_empty());
    assert!(tab.images.textures.is_empty());
    Ok(())
}

#[test]
fn relative_placements_follow_fixed_and_virtual_parents() -> Result<()> {
    let mut tab = graphics_tab()?;
    transmit(&mut tab, "i=1,p=1,c=2,r=2,C=1");
    transmit(&mut tab, "i=2,p=1,P=1,Q=1,H=1,V=1,c=1,r=1,C=1");
    frame(&mut tab)?;
    assert_eq!(tab.placements.len(), 2);
    let child = tab.placements.iter().find(|p| p.image_id == 2).unwrap();
    assert_eq!(
        child.clip,
        Bounds::new(point(px(10.), px(20.)), size(px(10.), px(20.)))
    );
    // A virtual parent's anchor is the minimum x and y across its placeholder occurrences.
    tab.terminal.vt_write(b"\x1b_Ga=d,d=A,q=2\x1b\\");
    transmit(&mut tab, "i=1,p=1,U=1,c=2,r=2");
    transmit(&mut tab, "i=2,p=1,P=1,Q=1,H=1,V=1,c=1,r=1,C=1");
    tab.terminal
        .vt_write(b"\x1b[3;4H\x1b[38;2;0;0;1;58;2;0;0;1m");
    placeholder(&mut tab, '\u{305}', '\u{305}');
    frame(&mut tab)?;
    let child = tab.placements.iter().find(|p| p.image_id == 2).unwrap();
    assert_eq!(child.clip.origin, point(px(40.), px(60.)));
    tab.terminal.vt_write(b"\x1b[5;2H");
    placeholder(&mut tab, '\u{305}', '\u{305}');
    transmit(&mut tab, "i=3,p=1,P=2,Q=1,H=-1,V=1,c=1,r=1,C=1");
    frame(&mut tab)?;
    let children: Vec<_> = tab.placements.iter().filter(|p| p.image_id == 2).collect();
    assert_eq!(children.len(), 1);
    assert!(
        children
            .iter()
            .any(|p| p.clip.origin == point(px(20.), px(60.)))
    );
    let grandchildren: Vec<_> = tab.placements.iter().filter(|p| p.image_id == 3).collect();
    assert_eq!(grandchildren.len(), 1);
    assert!(
        grandchildren
            .iter()
            .any(|p| p.clip.origin == point(px(10.), px(80.)))
    );
    let mut bytes = Vec::new();
    ghostty_vt::snapshot::encode_to(&tab.terminal, &mut bytes)?;
    let mut restored =
        crate::Tab::new(ghostty_vt::snapshot::Decoder::from_slice(&bytes).decode()?)?;
    frame(&mut restored)?;
    assert_eq!(restored.placements.len(), tab.placements.len());
    assert!(
        restored
            .placements
            .iter()
            .zip(&tab.placements)
            .all(|(a, b)| a.clip == b.clip && a.bounds == b.bounds)
    );
    tab.terminal.vt_write(b"\x1b_Ga=d,d=I,i=1,q=2\x1b\\");
    frame(&mut tab)?;
    assert!(
        tab.placements
            .iter()
            .all(|p| p.image_id != 1 && p.image_id != 2 && p.image_id != 3)
    );
    Ok(())
}

#[test]
fn relative_child_remains_visible_when_its_parent_scrolls_offscreen() -> Result<()> {
    let mut tab = graphics_tab()?;
    transmit(&mut tab, "i=1,p=1,c=1,r=1,C=1");
    transmit(&mut tab, "i=2,p=1,P=1,Q=1,H=0,V=3,c=1,r=1,C=1,z=-1");
    tab.terminal.vt_write(b"\x1b[8;1H\r\n\r\n\r\n");
    frame(&mut tab)?;
    assert_eq!(tab.placements.len(), 1);
    let child = &tab.placements[0];
    assert_eq!(child.image_id, 2);
    assert_eq!(child.z, -1);
    assert_eq!(
        child.clip,
        Bounds::new(point(px(0.), px(0.)), size(px(10.), px(20.)))
    );
    tab.prepare_frame(
        Bounds::new(point(px(0.), px(0.)), size(px(240.), px(320.))),
        size(px(20.), px(40.)),
    )?;
    assert_eq!(tab.placements[0].clip.size, size(px(20.), px(40.)));
    assert_eq!(tab.images.textures.len(), 1);
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
