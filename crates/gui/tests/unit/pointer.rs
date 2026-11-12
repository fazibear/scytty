use super::*;
#[test]
fn application_shapes_and_default_reset() {
    assert_eq!(cursor(None), CursorStyle::IBeam);
    assert_eq!(cursor(Some("default")), CursorStyle::Arrow);
    assert_eq!(cursor(Some("pointer")), CursorStyle::PointingHand);
    assert_eq!(cursor(Some("text")), CursorStyle::IBeam);
    assert_eq!(
        cursor(Some("not-allowed")),
        CursorStyle::OperationNotAllowed
    );
    assert_eq!(cursor(Some("wait")), CursorStyle::Arrow);
}
