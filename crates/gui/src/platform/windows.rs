use anyhow::Result;
use gpui::{prelude::*, *};
use std::{os::windows::process::CommandExt, process::Command};

pub const OVERLAY_SCROLLBAR: bool = true;

pub fn set_progress(
    progress: Option<&scytty_common::protocol::Progress>,
    window: &Window,
) -> Result<()> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{
        Foundation::HWND,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        },
        UI::Shell::{
            ITaskbarList3, TBPF_ERROR, TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL,
            TBPF_PAUSED, TaskbarList,
        },
    };
    let RawWindowHandle::Win32(handle) = HasWindowHandle::window_handle(window)?.as_raw() else {
        anyhow::bail!("Window has no HWND")
    };
    let hwnd = HWND(handle.hwnd.get() as *mut std::ffi::c_void);
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let result = (|| -> Result<()> {
            let taskbar: ITaskbarList3 =
                CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)?;
            taskbar.HrInit()?;
            let state = match progress.map(|p| p.state.as_str()) {
                Some("normal") => TBPF_NORMAL,
                Some("error") => TBPF_ERROR,
                Some("paused") => TBPF_PAUSED,
                Some("indeterminate") => TBPF_INDETERMINATE,
                _ => TBPF_NOPROGRESS,
            };
            if let Some(progress) = progress
                && state != TBPF_NOPROGRESS
                && state != TBPF_INDETERMINATE
            {
                taskbar.SetProgressValue(
                    hwnd,
                    u64::from(progress.percent.unwrap_or(0).min(100)),
                    100,
                )?;
            }
            taskbar.SetProgressState(hwnd, state)?;
            Ok(())
        })();
        if initialized {
            CoUninitialize();
        }
        result
    }
}

pub fn clear_progress(_window: WindowId) {}
pub fn tick_progress(_window: WindowId) {}

pub fn init_notifications() -> Result<()> {
    // Unpackaged Windows apps need an identity registered for toast delivery.
    let mut command = Command::new("reg.exe");
    command.args([
        "add",
        r"HKCU\Software\Classes\AppUserModelId\app.scytty.terminal",
        "/v",
        "DisplayName",
        "/t",
        "REG_SZ",
        "/d",
        "Scytty",
        "/f",
    ]);
    hide_console(&mut command);
    let output = command.output()?;
    anyhow::ensure!(
        output.status.success(),
        "Could not register notification identity: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

pub fn configure_notification(notification: &mut notify_rust::Notification) {
    notification.app_id("app.scytty.terminal");
}

pub fn command_modifier(modifiers: Modifiers) -> bool {
    modifiers.control
}

pub fn clipboard_modifier(modifiers: Modifiers) -> bool {
    command_modifier(modifiers) && modifiers.shift
}

pub fn zoom_modifier(modifiers: Modifiers) -> bool {
    !modifiers.alt && modifiers.control
}

pub fn titlebar() -> Stateful<Div> {
    super::titlebar_base()
        .pl(px(8.))
        // GPUI's start_window_move is a no-op on Windows; use native caption hit-testing.
        .window_control_area(WindowControlArea::Drag)
        .on_mouse_down(MouseButton::Left, |event, window, _| {
            if event.click_count == 2 {
                window.zoom_window();
            }
        })
}

pub fn window_controls(mut bar: Stateful<Div>) -> Stateful<Div> {
    for (i, label) in ["−", "□", "×"].into_iter().enumerate() {
        bar = bar.child(
            div()
                .id(("control", i))
                .w(px(46.))
                .flex_shrink_0()
                .text_center()
                .child(label)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, _| match i {
                    0 => window.minimize_window(),
                    1 => window.zoom_window(),
                    _ => window.remove_window(),
                }),
        );
    }
    bar
}

pub fn paint_scrollbar(
    window: &mut Window,
    _track: Bounds<Pixels>,
    thumb: Bounds<Pixels>,
    has_history: bool,
    opacity: f32,
) {
    if !has_history {
        return;
    }
    let mut color = rgba(0xffffff70);
    color.a *= opacity;
    let mut thumb = fill(thumb, color);
    thumb.corner_radii = (thumb.bounds.size.width / 2.).into();
    window.paint_quad(thumb);
}

pub fn connect_local() -> Result<crate::transport::Connection> {
    let mut command = Command::new(scytty_common::connection::server_executable()?);
    command.arg("--stdio");
    hide_console(&mut command);
    crate::transport::bridge(command)
}

pub fn hide_console(command: &mut Command) {
    // ssh.exe otherwise creates a visible console window for each GUI child.
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}
