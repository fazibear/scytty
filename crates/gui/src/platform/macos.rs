use anyhow::Result;
use gpui::{prelude::*, *};
use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained};
use objc2_app_kit::{
    NSApplication, NSBox, NSBoxType, NSColor, NSImageScaling, NSImageView, NSProgressIndicator,
    NSTitlePosition, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::{
    process::Command,
    time::{Duration, Instant},
};

pub const OVERLAY_SCROLLBAR: bool = true;
pub const WINDOWS_PATHS: bool = false;

#[derive(Default)]
struct DockProgress {
    owner: Option<WindowId>,
    value: Option<scytty_common::protocol::Progress>,
    animation: Option<Task<()>>,
}
thread_local! {
    static DOCK_PROGRESS: std::cell::RefCell<DockProgress> = std::cell::RefCell::default();
}

pub fn set_progress(
    progress: Option<&scytty_common::protocol::Progress>,
    window: &Window,
    cx: &mut App,
) -> Result<()> {
    let id = window.window_handle().window_id();
    DOCK_PROGRESS.with_borrow_mut(|state| {
        if state.owner.is_some_and(|owner| owner != id) && !window.is_window_active() {
            return;
        }
        if state.owner == Some(id) && state.value.as_ref() == progress {
            return;
        }
        state.animation = None;
        state.owner = Some(id);
        state.value = progress.cloned();
        if let Some((segment, frame)) = draw_dock_progress(progress) {
            let started = Instant::now();
            let mtm = MainThreadMarker::new().expect("Dock progress runs on the main thread");
            let dock = NSApplication::sharedApplication(mtm).dockTile();
            state.animation = Some(cx.spawn(async move |cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(33))
                        .await;
                    if cx
                        .update(|_| {
                            let x = indeterminate_offset(
                                started.elapsed().as_secs_f64(),
                                frame.size.width - segment.frame().size.width,
                            );
                            segment
                                .setFrameOrigin(NSPoint::new(frame.origin.x + x, frame.origin.y));
                            dock.display();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }
    });
    Ok(())
}

pub fn clear_progress(window: WindowId) {
    DOCK_PROGRESS.with_borrow_mut(|state| {
        if state.owner == Some(window) {
            *state = DockProgress::default();
            let _ = draw_dock_progress(None);
        }
    });
}

fn indeterminate_offset(seconds: f64, available: f64) -> f64 {
    let phase = seconds.rem_euclid(2.);
    available.max(0.) * if phase < 1. { phase } else { 2. - phase }
}

fn dock_box(frame: NSRect, color: &NSColor, mtm: MainThreadMarker) -> Retained<NSBox> {
    let view = NSBox::initWithFrame(NSBox::alloc(mtm), frame);
    view.setBoxType(NSBoxType::Custom);
    view.setTitlePosition(NSTitlePosition::NoTitle);
    view.setBorderWidth(0.);
    view.setCornerRadius(frame.size.height / 2.);
    view.setFillColor(color);
    view
}

fn draw_dock_progress(
    progress: Option<&scytty_common::protocol::Progress>,
) -> Option<(Retained<NSBox>, NSRect)> {
    let mtm = MainThreadMarker::new().expect("Dock progress runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let dock = app.dockTile();
    let Some(progress) = progress else {
        dock.setContentView(None);
        dock.display();
        return None;
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
    if progress.state == "indeterminate" {
        // Dock content has no window to drive NSProgressIndicator's animation.
        let track = dock_box(frame, &NSColor::colorWithWhite_alpha(0., 0.4), mtm);
        let segment_frame = NSRect::new(
            frame.origin,
            NSSize::new(frame.size.width * 0.28, frame.size.height),
        );
        let segment = dock_box(segment_frame, &NSColor::controlAccentColor(), mtm);
        view.addSubview(&track);
        view.addSubview(&segment);
        dock.setContentView(Some(&view));
        dock.display();
        return Some((segment, frame));
    }
    let bar = NSProgressIndicator::initWithFrame(NSProgressIndicator::alloc(mtm), frame);
    bar.setIndeterminate(false);
    bar.setMinValue(0.);
    bar.setMaxValue(100.);
    bar.setDoubleValue(f64::from(progress.percent.unwrap_or(0).min(100)));
    view.addSubview(&bar);
    dock.setContentView(Some(&view));
    dock.display();
    None
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

pub fn hide_console(_command: &mut Command) {}

#[cfg(test)]
#[path = "../../tests/unit/platform/macos.rs"]
mod notification_tests;
