use super::*;
#[test]
fn endpoint_acl_rejects_public_access() -> Result<()> {
    let directory = std::env::temp_dir().join(format!(
        "scytty-acl-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    private_path(&directory, true)?;
    let path = directory.join("endpoint");
    std::fs::write(&path, b"test")?;
    private_path(&path, false)?;
    unsafe {
        let sddl = wide(std::ffi::OsStr::new("D:P(A;;FA;;;WD)"));
        let mut descriptor = null_mut();
        assert_ne!(
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                null_mut()
            ),
            0
        );
        let _descriptor = LocalMemory(descriptor);
        assert_ne!(
            SetFileSecurityW(
                wide(path.as_os_str()).as_ptr(),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                descriptor
            ),
            0
        );
    }
    assert!(private_path(&path, false).is_err());
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn authentication_proofs_are_role_and_nonce_specific() {
    let secret = [1; 32];
    let nonce = [2; 32];
    let expected = proof(&secret, &nonce, b"server");
    assert!(
        authenticator(&secret, &nonce, b"server")
            .verify_slice(&expected)
            .is_ok()
    );
    assert!(
        authenticator(&secret, &nonce, b"client")
            .verify_slice(&expected)
            .is_err()
    );
    assert!(
        authenticator(&secret, &[3; 32], b"server")
            .verify_slice(&expected)
            .is_err()
    );
    assert!(
        authenticator(&[4; 32], &nonce, b"server")
            .verify_slice(&expected)
            .is_err()
    );
}
