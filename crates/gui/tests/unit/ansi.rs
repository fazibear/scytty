use super::*;
#[test]
fn prompt_colors_and_control_sequences() {
    let text = "\x1b[32m main \x1b[38;2;1;2;3m✓\x1b[0m done\x1b[K";
    let result = segments(text, None);
    assert_eq!(
        result,
        vec![
            (" main ".into(), Some(indexed(2))),
            ("✓".into(), Some(rgb(0x010203))),
            (" done".into(), None)
        ]
    );
    assert_eq!(
        segments("\x1b[38;5;196mred", None)[0].1,
        Some(rgb(0xff0000))
    );
    assert_eq!(
        segments("\x1b]0;hidden\x07ok\x1b[38;2m!", None),
        vec![("ok".into(), None), ("!".into(), None)]
    );
    assert_eq!(
        segments("\x1b[0mghost", Some(gpui::rgba(0)))[0].1,
        Some(gpui::rgba(0))
    );
}
