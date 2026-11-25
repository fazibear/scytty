use super::*;

#[test]
fn shades_and_legacy_mosaics_use_windows_graphics_with_font_fallback_elsewhere() {
    for (text, expected) in [
        ("░", Glyph::Shade(1)),
        ("▒", Glyph::Shade(2)),
        ("▓", Glyph::Shade(3)),
        ("\u{1fb00}", Glyph::Mosaic { mask: 1, rows: 3 }),
        ("\u{1fb3b}", Glyph::Mosaic { mask: 62, rows: 3 }),
        ("\u{1cd00}", Glyph::Mosaic { mask: 4, rows: 4 }),
        ("\u{1cde5}", Glyph::Mosaic { mask: 254, rows: 4 }),
        ("\u{1cea0}", Glyph::Mosaic { mask: 128, rows: 4 }),
        ("\u{1fbe6}", Glyph::Mosaic { mask: 20, rows: 4 }),
    ] {
        assert_eq!(Glyph::decode(text, true), Some(expected));
        assert_eq!(Glyph::decode(text, false), None);
        assert_eq!(Glyph::decode(&format!("{text}\u{301}"), true), None);
    }
    let cell = size(px(9.3), px(18.7));
    let origin = point(px(2.3), px(3.7));
    for scale in [1., 1.25, 1.5, 1.75, 2.] {
        for level in 1..=3 {
            let shape = Glyph::Shade(level)
                .shapes(origin, cell, scale)
                .pop()
                .unwrap();
            assert_eq!(
                shape.bounds,
                block_bounds(origin, cell, (0, 0, 8, 8), scale)
            );
            assert_eq!(shape.opacity, f32::from(level) / 4.);
        }
        for rows in [3, 4] {
            let full = Glyph::Mosaic { mask: 255, rows }.shapes(origin, cell, scale);
            for pair in full.chunks_exact(2) {
                assert!((pair[0].bounds.right() - pair[1].bounds.left()).abs() < px(0.0001));
            }
            for y in 0..rows - 1 {
                assert!(
                    (full[y as usize * 2].bounds.bottom()
                        - full[(y as usize + 1) * 2].bounds.top())
                    .abs()
                        < px(0.0001)
                );
            }
        }
        for code in (0x1fb00..=0x1fb3b).chain(0x1cd00..=0x1cde5) {
            let glyph = Glyph::decode(&char::from_u32(code).unwrap().to_string(), true).unwrap();
            let shapes = glyph.shapes(origin, cell, scale);
            assert!(!shapes.is_empty());
            for shape in shapes {
                assert_eq!(shape.opacity, 1.);
                for edge in [
                    shape.bounds.left(),
                    shape.bounds.right(),
                    shape.bounds.top(),
                    shape.bounds.bottom(),
                ] {
                    let physical = f32::from(edge) * scale;
                    assert!((physical - physical.round()).abs() < 0.0001);
                }
            }
        }
    }
}

#[test]
fn shared_decoder_preserves_platform_policy_and_font_fallback() {
    for windows in [false, true] {
        for text in ["█", "▀", "▐", "▟"] {
            assert_eq!(
                Glyph::decode(text, windows),
                windows.then_some(Glyph::Block(text.chars().next().unwrap()))
            );
        }
        for code in 0x2500..=0x257f {
            let ch = char::from_u32(code).unwrap();
            let text = ch.to_string();
            assert_eq!(
                Glyph::decode(&text, windows),
                windows.then_some(Glyph::Border(ch))
            );
        }
        for text in ["a", " ", "", "█\u{301}", "╭╮", "⣿\u{301}"] {
            assert_eq!(Glyph::decode(text, windows), None);
        }
    }
    for text in ["█", "╭", "⣿", "a"] {
        assert_eq!(
            Glyph::from_text(text),
            Glyph::decode(text, cfg!(target_os = "windows"))
        );
    }
}

#[test]
fn shared_geometry_preserves_blocks_border_strokes_and_round_dots() {
    let origin = point(px(2.3), px(3.7));
    let cell = size(px(9.3), px(18.7));
    for scale in [1., 1.25, 1.5, 1.75, 2.] {
        for text in ["█", "▟", "╔", "╭", "╳", "⣿"] {
            let glyph = Glyph::decode(text, true).unwrap();
            let shapes = glyph.shapes(origin, cell, scale);
            assert!(!shapes.is_empty());
            for shape in shapes {
                assert_eq!(shape.radius > px(0.), matches!(glyph, Glyph::Braille(_)));
                for edge in [
                    shape.bounds.left(),
                    shape.bounds.right(),
                    shape.bounds.top(),
                    shape.bounds.bottom(),
                ] {
                    let physical = f32::from(edge) * scale;
                    assert!((physical - physical.round()).abs() < 0.0001);
                }
            }
            assert!(
                glyph
                    .shapes(origin, size(px(0.), cell.height), scale)
                    .is_empty()
            );
        }
        assert!(Glyph::Braille(0).shapes(origin, cell, scale).is_empty());
    }
}

#[test]
fn braille_indicators_have_upright_pixel_aligned_dot_columns() {
    let origin = point(px(2.3), px(3.7));
    let cell = size(px(9.3), px(18.7));
    for scale in [1., 1.25, 1.5, 1.75, 2.] {
        let dots = braille_dots(255, origin, cell, scale);
        for column in [[0, 1, 2, 6], [3, 4, 5, 7]] {
            for pair in column.windows(2) {
                assert_eq!(dots[pair[0]].origin.x, dots[pair[1]].origin.x);
                assert!(dots[pair[0]].bottom() < dots[pair[1]].top());
            }
        }
        for (a, b) in [(0, 3), (1, 4), (2, 5), (6, 7)] {
            assert_eq!(dots[a].top(), dots[b].top());
            assert!(dots[a].right() < dots[b].left());
        }
        for dot in &dots {
            for edge in [dot.left(), dot.right(), dot.top(), dot.bottom()] {
                let physical = f32::from(edge) * scale;
                assert!((physical - physical.round()).abs() < 0.0001);
            }
        }
        for mask in 0..=255u8 {
            let text = char::from_u32(0x2800 + u32::from(mask))
                .unwrap()
                .to_string();
            assert_eq!(Glyph::decode(&text, true), Some(Glyph::Braille(mask)));
            let selected = braille_dots(mask, origin, cell, scale);
            let expected: Vec<_> = dots
                .iter()
                .enumerate()
                .filter(|(bit, _)| mask & (1 << bit) != 0)
                .map(|(_, dot)| *dot)
                .collect();
            assert_eq!(selected, expected);
        }
    }
    for text in ["", "a", "⣿⣿", "⣿\u{301}"] {
        assert_eq!(Glyph::decode(text, true), None);
    }
}
