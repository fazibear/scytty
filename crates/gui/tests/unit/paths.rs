use super::*;

#[test]
fn paths_are_quoted_for_the_destination_shell() -> Result<()> {
    let path = Path::new("C:\\Research & Development\\it's (new).txt");
    assert_eq!(
        dropped(&[path], true, Some("cmd.exe"))?,
        "\"C:\\Research & Development\\it's (new).txt\" "
    );
    assert_eq!(
        dropped(
            &[path],
            true,
            Some("C:\\Program Files\\PowerShell\\pwsh.exe")
        )?,
        "'C:\\Research & Development\\it''s (new).txt' "
    );
    assert_eq!(
        dropped(&[Path::new("a'b"), Path::new("c d")], false, None)?,
        "'a'\\''b' 'c d' "
    );
    assert_eq!(
        dropped(&[Path::new("a'b")], true, Some("bash.exe"))?,
        "'a'\\''b' "
    );
    for path in ["%TEMP%\\file", "a!NAME!", "a\"b", "a\nb"] {
        assert!(dropped(&[Path::new(path)], true, Some("cmd.exe")).is_err());
    }
    assert!(dropped(&[path], true, Some("unknown.exe")).is_err());
    Ok(())
}
