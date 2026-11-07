use std::path::PathBuf;

#[cfg(target_os = "linux")]
pub fn process_cwd(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(target_os = "macos")]
pub fn process_cwd(pid: u32) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let pid = i32::try_from(pid).ok()?;
    let mut info = std::mem::MaybeUninit::<libc::proc_vnodepathinfo>::zeroed();
    let size = std::mem::size_of_val(&info) as i32;
    // Read only our user's process metadata; use the struct only after a complete result.
    let count = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if count != size {
        return None;
    }
    let info = unsafe { info.assume_init() };
    let bytes: Vec<u8> = info
        .pvi_cdir
        .vip_path
        .into_iter()
        .flatten()
        .take_while(|b| *b != 0)
        .map(|b| b as u8)
        .collect();
    if bytes.is_empty() {
        return None;
    }
    Some(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn process_cwd(_: u32) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_current_process_directory() {
        assert_eq!(
            super::process_cwd(std::process::id()),
            std::env::current_dir().ok()
        );
        assert!(super::process_cwd(u32::MAX).is_none());
    }
}
