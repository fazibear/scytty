use anyhow::Result;
use gpui::{prelude::*, *};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSImageScaling, NSImageView, NSProgressIndicator, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize};
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

pub fn tick_progress(window: WindowId) {
    DOCK_PROGRESS.with_borrow(|state| {
        if state.0 == Some(window) && state.1.as_ref().is_some_and(|p| p.state == "indeterminate") {
            let mtm = MainThreadMarker::new().expect("Dock progress runs on the main thread");
            NSApplication::sharedApplication(mtm).dockTile().display();
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

fn draw_dock_progress(progress: Option<&scytty_common::protocol::Progress>) {
    let mtm = MainThreadMarker::new().expect("Dock progress runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let dock = app.dockTile();
    let Some(progress) = progress else {
        dock.setContentView(None);
        dock.display();
        return;
    };
    let dimensions = dock.size();
    let bounds = NSRect::new(NSPoint::new(0., 0.), dimensions);
    let view = NSView::initWithFrame(NSView::alloc(mtm), bounds);
    let image = NSImageView::initWithFrame(NSImageView::alloc(mtm), bounds);
    image.setImage(app.applicationIconImage().as_deref());
    image.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
    view.addSubview(&image);
    let frame = NSRect::new(
        NSPoint::new(dimensions.width * 0.08, dimensions.height * 0.04),
        NSSize::new(dimensions.width * 0.84, dimensions.height * 0.10),
    );
    let bar = NSProgressIndicator::initWithFrame(NSProgressIndicator::alloc(mtm), frame);
    let indeterminate = progress.state == "indeterminate";
    bar.setIndeterminate(indeterminate);
    bar.setMinValue(0.);
    bar.setMaxValue(100.);
    bar.setDoubleValue(f64::from(progress.percent.unwrap_or(0).min(100)));
    view.addSubview(&bar);
    if indeterminate {
        // No sender is needed for this programmatic action.
        unsafe { bar.startAnimation(None) };
    }
    dock.setContentView(Some(&view));
    dock.display();
}

pub fn init_notifications() -> Result<()> {
    allow_foreground_notifications()?;
    notify_rust::set_application("app.scytty.terminal")?;
    Ok(())
}

fn allow_foreground_notifications() -> Result<()> {
    use objc2::{
        Encode, ffi,
        runtime::{AnyClass, AnyObject, Bool, Imp, Sel},
    };

    extern "C-unwind" fn should_present(
        _: *mut AnyObject,
        _: Sel,
        _: *mut AnyObject,
        _: *mut AnyObject,
    ) -> Bool {
        Bool::YES
    }

    // shortcut: targets notify-rust's legacy delegate; revisit when its macOS backend changes.
    let class = AnyClass::get(c"NotificationCenterDelegate")
        .ok_or_else(|| anyhow::anyhow!("Notification delegate is unavailable"))?;
    let selector = Sel::register(c"userNotificationCenter:shouldPresentNotification:");
    if class.instance_method(selector).is_none() {
        let encoding = std::ffi::CString::new(format!("{}@:@@", Bool::ENCODING))?;
        // The method has the Objective-C BOOL return type and two object arguments.
        let added = unsafe {
            ffi::class_addMethod(
                class as *const AnyClass as *mut AnyClass,
                selector,
                std::mem::transmute::<
                    extern "C-unwind" fn(
                        *mut AnyObject,
                        Sel,
                        *mut AnyObject,
                        *mut AnyObject,
                    ) -> Bool,
                    Imp,
                >(should_present),
                encoding.as_ptr(),
            )
        };
        anyhow::ensure!(added.as_bool(), "Could not enable foreground notifications");
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
        use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
        super::allow_foreground_notifications()?;
        super::allow_foreground_notifications()?;
        let selector = Sel::register(c"userNotificationCenter:shouldPresentNotification:");
        let method = AnyClass::get(c"NotificationCenterDelegate")
            .unwrap()
            .instance_method(selector)
            .unwrap();
        let callback: extern "C-unwind" fn(
            *mut AnyObject,
            Sel,
            *mut AnyObject,
            *mut AnyObject,
        ) -> Bool = unsafe { std::mem::transmute(method.implementation()) };
        assert_eq!(
            callback(
                std::ptr::null_mut(),
                selector,
                std::ptr::null_mut(),
                std::ptr::null_mut()
            ),
            Bool::YES
        );
        Ok(())
    }
}
