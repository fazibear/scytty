#[test]
fn installed_fonts_only() {
    assert!(!super::resolve("monospace").unwrap().is_empty());
    assert!(super::resolve("nonexistent-scytty-font").is_err());
}
