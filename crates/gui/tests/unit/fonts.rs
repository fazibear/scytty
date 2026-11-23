#[test]
fn missing_family_falls_back_to_system_monospace() {
    let default = super::resolve("monospace").unwrap();
    assert!(!default.is_empty());
    assert_eq!(super::resolve("nonexistent-scytty-font").unwrap(), default);
    assert_eq!(super::resolve(&default).unwrap(), default);
}
