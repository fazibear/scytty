#[test]
fn indeterminate_segment_moves_back_and_forth_without_becoming_a_percentage() {
    let positions = [0., 0.5, 1., 1.5, 2.].map(|time| super::indeterminate_offset(time, 72.));
    assert_eq!(positions, [0., 36., 72., 36., 0.]);
    for frame in 0..600 {
        let x = super::indeterminate_offset(f64::from(frame) / 30., 72.);
        assert!((0. ..=72.).contains(&x));
    }
    assert_eq!(super::indeterminate_offset(0.5, 0.), 0.);
}
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
