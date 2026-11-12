use super::*;
#[test]
fn windows_shell_uses_comspec_without_unix_login_flags() {
    let shell = shell();
    assert_eq!(shell.get_argv().len(), 1);
}
