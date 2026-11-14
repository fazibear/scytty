use super::*;

#[test]
fn native_composition_replacement_keeps_valid_scalars_and_clamps_selection() {
    let mut marked = "A😀Z".to_owned();
    assert_eq!(replace_marked(&mut marked, Some(2..3), "X", None), 2..2);
    assert_eq!(marked, "AXZ");
    assert_eq!(
        replace_marked(&mut marked, Some(1..2), "e\u{301}", Some(0..2)),
        1..3
    );
    assert_eq!(marked, "Ae\u{301}Z");
    let emoji = "👩🏽‍💻";
    let end = 1 + emoji.encode_utf16().count();
    assert_eq!(
        replace_marked(&mut marked, Some(1..1), emoji, Some(100..200)),
        end..end
    );
    assert_eq!(marked, format!("A{emoji}e\u{301}Z"));
    assert_eq!(replace_marked(&mut marked, None, "日本", Some(0..1)), 0..1);
    assert_eq!(marked, "日本");
}

#[test]
fn native_ranges_preserve_non_bmp_scalars_and_report_actual_ranges() {
    let text = "a😀e\u{301}界";
    for start in 0..=10 {
        for end in 0..=10 {
            let bytes = byte_range(text, start..end);
            assert!(text.is_char_boundary(bytes.start));
            assert!(text.is_char_boundary(bytes.end));
            assert!(bytes.start <= bytes.end);
            let units = utf16_range(text, bytes.clone());
            assert_eq!(byte_range(text, units), bytes);
        }
    }
    assert_eq!(&text[byte_range(text, 1..2)], "😀");
    assert_eq!(&text[byte_range(text, 2..3)], "😀");
    assert_eq!(byte_range(text, 2..2), 1..1);
    assert_eq!(&text[byte_range(text, 3..5)], "e\u{301}");
    assert_eq!(byte_range(text, 100..200), text.len()..text.len());
}
