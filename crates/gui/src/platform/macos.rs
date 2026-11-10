use anyhow::Result;
use gpui::{prelude::*, *};
use std::process::Command;

pub const OVERLAY_SCROLLBAR: bool = true;

thread_local! {
    static DOCK_PROGRESS: std::cell::RefCell<(Option<WindowId>, Option<scytty_common::protocol::Progress>)> = const { std::cell::RefCell::new((None, None)) };
}

pub fn set_progress(
    progress: Option<&scytty_common::protocol::Progress>,
    window: &Window,
) -> Result<()> {
    let id = window.window_handle().window_id();
    DOCK_PROGRESS.with_borrow_mut(|state| {
        if state.0.is_some_and(|owner| owner != id) && !window.is_window_active() {
            return;
        }
        if state.0 == Some(id) && state.1.as_ref() == progress {
            return;
        }
        *state = (Some(id), progress.cloned());
        draw_dock_progress(progress);
    });
    Ok(())
}

#[allow(deprecated, unexpected_cfgs)]
pub fn tick_progress(window: WindowId) {
    DOCK_PROGRESS.with_borrow(|state| {
        if state.0 == Some(window) && state.1.as_ref().is_some_and(|p| p.state == "indeterminate") {
            use objc::{msg_send, runtime::Object, sel, sel_impl};
            unsafe {
                let dock: *mut Object = msg_send![cocoa::appkit::NSApp(), dockTile];
                let _: () = msg_send![dock, display];
            }
        }
    });
}

pub fn clear_progress(window: WindowId) {
    DOCK_PROGRESS.with_borrow_mut(|state| {
        if state.0 == Some(window) {
            *state = (None, None);
            draw_dock_progress(None);
        }
    });
}

#[allow(deprecated, unexpected_cfgs)]
fn draw_dock_progress(progress: Option<&scytty_common::protocol::Progress>) {
    use cocoa::{
        appkit::NSApp,
        foundation::{NSPoint, NSRect, NSSize},
    };
    use objc::{
        class, msg_send,
        runtime::{NO, Object, YES},
        sel, sel_impl,
    };
    unsafe {
        let app = NSApp();
        let dock: *mut Object = msg_send![app, dockTile];
        let nil = std::ptr::null_mut::<Object>();
        let Some(progress) = progress else {
            let _: () = msg_send![dock, setContentView: nil];
            let _: () = msg_send![dock, display];
            return;
        };
        let dimensions: NSSize = msg_send![dock, size];
        let bounds = NSRect::new(NSPoint::new(0., 0.), dimensions);
        let view: *mut Object = msg_send![class!(NSView), alloc];
        let view: *mut Object = msg_send![view, initWithFrame: bounds];
        let image: *mut Object = msg_send![class!(NSImageView), alloc];
        let image: *mut Object = msg_send![image, initWithFrame: bounds];
        let icon: *mut Object = msg_send![app, applicationIconImage];
        let _: () = msg_send![image, setImage: icon];
        let _: () = msg_send![image, setImageScaling: 3usize];
        let _: () = msg_send![view, addSubview: image];
        let _: () = msg_send![image, release];
        let frame = NSRect::new(
            NSPoint::new(dimensions.width * 0.08, dimensions.height * 0.04),
            NSSize::new(dimensions.width * 0.84, dimensions.height * 0.10),
        );
        let bar: *mut Object = msg_send![class!(NSProgressIndicator), alloc];
        let bar: *mut Object = msg_send![bar, initWithFrame: frame];
        let indeterminate = progress.state == "indeterminate";
        let _: () = msg_send![bar, setIndeterminate: if indeterminate { YES } else { NO }];
        let _: () = msg_send![bar, setMinValue: 0f64];
        let _: () = msg_send![bar, setMaxValue: 100f64];
        let _: () =
            msg_send![bar, setDoubleValue: f64::from(progress.percent.unwrap_or(0).min(100))];
        let _: () = msg_send![view, addSubview: bar];
        if indeterminate {
            let _: () = msg_send![bar, startAnimation: nil];
        }
        let _: () = msg_send![dock, setContentView: view];
        let _: () = msg_send![dock, display];
        let _: () = msg_send![bar, release];
        let _: () = msg_send![view, release];
    }
}

pub fn init_notifications() -> Result<()> {
    allow_foreground_notifications()?;
    notify_rust::set_application("app.scytty.terminal")?;
    Ok(())
}

fn allow_foreground_notifications() -> Result<()> {
    use objc::{
        Encode,
        runtime::{self, BOOL, Class, Object, Sel, YES},
    };

    extern "C" fn should_present(_: *mut Object, _: Sel, _: *mut Object, _: *mut Object) -> BOOL {
        YES
    }

    // shortcut: targets notify-rust's legacy delegate; revisit when its macOS backend changes.
    let class = Class::get("NotificationCenterDelegate")
        .ok_or_else(|| anyhow::anyhow!("Notification delegate is unavailable"))?;
    let selector = Sel::register("userNotificationCenter:shouldPresentNotification:");
    if class.instance_method(selector).is_none() {
        let encoding = std::ffi::CString::new(format!("{}@:@@", BOOL::encode().as_str()))?;
        // The method has the Objective-C BOOL return type and two object arguments.
        let added = unsafe {
            runtime::class_addMethod(
                class as *const Class as *mut Class,
                selector,
                std::mem::transmute::<
                    extern "C" fn(*mut Object, Sel, *mut Object, *mut Object) -> BOOL,
                    runtime::Imp,
                >(should_present),
                encoding.as_ptr(),
            )
        };
        anyhow::ensure!(added == YES, "Could not enable foreground notifications");
    }
    Ok(())
}

pub fn configure_notification(_: &mut notify_rust::Notification) {}

pub fn command_modifier(modifiers: Modifiers) -> bool {
    modifiers.platform
}

pub fn clipboard_modifier(modifiers: Modifiers) -> bool {
    command_modifier(modifiers)
}

pub fn zoom_modifier(modifiers: Modifiers) -> bool {
    !modifiers.alt && (modifiers.control || modifiers.platform)
}

pub fn titlebar() -> Stateful<Div> {
    super::titlebar_base().pl(px(78.)).pr(px(8.)).on_mouse_down(
        MouseButton::Left,
        |event, window, _| {
            if event.click_count == 2 {
                window.zoom_window();
            } else {
                window.start_window_move();
            }
        },
    )
}

pub fn window_controls(bar: Stateful<Div>) -> Stateful<Div> {
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
    crate::transport::bridge(command)
}

pub fn hide_console(_command: &mut Command) {}

#[cfg(test)]
mod notification_tests {
    #[test]
    fn foreground_presentation_callback_is_installed_and_idempotent() -> anyhow::Result<()> {
        use objc::runtime::{BOOL, Class, Object, Sel, YES};
        super::allow_foreground_notifications()?;
        super::allow_foreground_notifications()?;
        let selector = Sel::register("userNotificationCenter:shouldPresentNotification:");
        let method = Class::get("NotificationCenterDelegate")
            .unwrap()
            .instance_method(selector)
            .unwrap();
        let callback: extern "C" fn(*mut Object, Sel, *mut Object, *mut Object) -> BOOL =
            unsafe { std::mem::transmute(method.implementation()) };
        assert_eq!(
            callback(
                std::ptr::null_mut(),
                selector,
                std::ptr::null_mut(),
                std::ptr::null_mut()
            ),
            YES
        );
        Ok(())
    }
}
