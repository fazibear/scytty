pub enum Accepted {
    Session,
    Control,
    Restart,
}

#[cfg_attr(windows, path = "connection/windows.rs")]
#[cfg_attr(unix, path = "connection/unix.rs")]
mod platform;
pub use platform::*;
