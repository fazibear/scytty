use super::*;

#[test]
fn native_progress_follows_live_active_tab_and_validates_percent() {
    let mut tabs = vec![
        TabInfo {
            id: 1,
            progress: Some(scytty_common::protocol::Progress {
                state: "normal".into(),
                percent: Some(42),
            }),
            ..Default::default()
        },
        TabInfo {
            id: 2,
            ..Default::default()
        },
    ];
    assert_eq!(active_progress(&tabs, 1).unwrap().percent, Some(42));
    assert!(active_progress(&tabs, 2).is_none());
    assert!(active_progress(&tabs, 99).is_none());
    tabs[0].progress.as_mut().unwrap().percent = Some(255);
    assert_eq!(active_progress(&tabs, 1).unwrap().percent, Some(100));
    tabs[0].exited = true;
    assert!(active_progress(&tabs, 1).is_none());
    tabs[0].exited = false;
    tabs[0].progress.as_mut().unwrap().state = "invalid".into();
    assert!(active_progress(&tabs, 1).is_none());
}
use core::prelude::v1::test;
#[test]
fn status_icons_expand_only_into_a_following_space() {
    let red = Some(rgb(0xff0000));
    assert_eq!(
        status_runs(vec![
            ("a\u{301} \u{e0b0}main\u{f0001}".into(), red),
            (" next\u{e0b0}".into(), None),
        ]),
        vec![
            ("a\u{301} \u{e0b0}main".into(), red, false),
            ("\u{f0001}".into(), red, true),
            ("next\u{e0b0}".into(), None, false),
        ]
    );
    assert_eq!(
        status_runs(vec![("\u{e0b0}  x".into(), None)]),
        vec![("\u{e0b0}".into(), None, true), (" x".into(), None, false),]
    );
    assert!(status_runs(vec![]).is_empty());
}

#[test]
fn zoom_shortcuts_and_limits() {
    let stroke = |key: &str, control| Keystroke {
        key: key.into(),
        key_char: None,
        modifiers: Modifiers {
            control,
            ..Default::default()
        },
    };
    assert_eq!(zoomed_font_size(16., 18., &stroke("+", true)), Some(17.));
    assert_eq!(zoomed_font_size(16., 18., &stroke("=", true)), Some(17.));
    assert_eq!(zoomed_font_size(16., 18., &stroke("-", true)), Some(15.));
    assert_eq!(zoomed_font_size(6., 18., &stroke("-", true)), Some(6.));
    assert_eq!(zoomed_font_size(96., 18., &stroke("+", true)), Some(96.));
    assert_eq!(zoomed_font_size(16., 18., &stroke("+", false)), None);
    assert_eq!(zoomed_font_size(16., 18., &stroke("c", true)), None);
    assert_eq!(zoomed_font_size(24., 18., &stroke("0", true)), Some(18.));
    assert_eq!(zoomed_font_size(24., 18., &stroke("0", false)), None);
    let mut command_zero = stroke("0", false);
    command_zero.modifiers.platform = true;
    assert_eq!(
        zoomed_font_size(24., 18., &command_zero),
        cfg!(target_os = "macos").then_some(18.)
    );
}

#[test]
fn row_height_preserves_descenders_and_joins_full_height_glyphs() {
    // FiraCode: 1950 units/em, ascent 1800, descent -600 (font-kit/CoreText).
    for font_size in [6., 9., 10., 11., 12., 16., 24.] {
        let ascent = px(font_size * 1800. / 1950.);
        let descent = px(font_size * 600. / 1950.);
        let natural = f32::from(ascent + descent);
        for scale in [1., 1.25, 1.5, 2.] {
            let height = terminal_cell_height(ascent, -descent, scale);
            assert_eq!(height, terminal_cell_height(ascent, descent, scale));
            let height = f32::from(height);
            assert!(height > font_size);
            assert!(height <= natural);
            assert!(natural - height < 1.001 / scale);
            let physical = height * scale;
            assert!((physical - physical.round()).abs() < 0.001);
        }
    }
    assert_eq!(
        terminal_cell_height(px(1800. * 16. / 1950.), px(-600. * 16. / 1950.), 2.),
        px(19.5)
    );
}
#[test]
fn fractional_trackpad_scroll_accumulates() {
    let mut remainder = 0.;
    assert_eq!(scroll_rows(&mut remainder, 0.25), 0);
    assert_eq!(scroll_rows(&mut remainder, 0.75), -1);
    assert_eq!(scroll_rows(&mut remainder, -0.5), 0);
    assert_eq!(scroll_rows(&mut remainder, -1.5), 2);
}

#[test]
fn paths_icons_and_navigation() -> Result<()> {
    assert_eq!(
        paths::dropped(&[std::path::Path::new("/tmp/it's;$x.png")], false, None)?,
        "'/tmp/it'\\''s;$x.png' "
    );
    assert!(expand_nerd_icon("\u{e0b0}", " "));
    assert!(!expand_nerd_icon("\u{e0b0}", "x"));
    let tabs: Vec<_> = [4, 8, 12]
        .into_iter()
        .map(|id| TabInfo {
            id,
            ..Default::default()
        })
        .collect();
    assert_eq!(adjacent_tab(&tabs, 4, -1), Some(12));
    assert_eq!(adjacent_tab(&tabs, 12, 1), Some(4));
    assert_eq!(adjacent_tab(&[], 0, 1), None);
    Ok(())
}
