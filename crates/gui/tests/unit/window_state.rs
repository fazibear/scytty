#[test]
fn valid_and_corrupt_geometry() {
    assert!(matches!(
        super::parse("maximized -100 20 960 640"),
        Some(gpui::WindowBounds::Maximized(_))
    ));
    for source in [
        "",
        "window 0 0 960",
        "window NaN 0 960 640",
        "window 0 0 -1 640",
        "oops 0 0 960 640",
    ] {
        assert!(super::parse(source).is_none());
    }
}
