#[test]
fn reads_current_process_directory() {
    assert_eq!(
        super::process_cwd(std::process::id()),
        std::env::current_dir().ok()
    );
    assert!(super::process_cwd(u32::MAX).is_none());
}
