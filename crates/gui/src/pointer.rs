use gpui::CursorStyle;

pub fn cursor(shape: Option<&str>) -> CursorStyle {
    match shape {
        None | Some("text") => CursorStyle::IBeam,
        Some("pointer") => CursorStyle::PointingHand,
        Some("context-menu") => CursorStyle::ContextualMenu,
        Some("cell" | "crosshair") => CursorStyle::Crosshair,
        Some("vertical-text") => CursorStyle::IBeamCursorForVerticalLayout,
        Some("alias") => CursorStyle::DragLink,
        Some("copy") => CursorStyle::DragCopy,
        Some("no-drop" | "not-allowed") => CursorStyle::OperationNotAllowed,
        Some("grab" | "move" | "all-scroll") => CursorStyle::OpenHand,
        Some("grabbing") => CursorStyle::ClosedHand,
        Some("col-resize") => CursorStyle::ResizeColumn,
        Some("row-resize") => CursorStyle::ResizeRow,
        Some("n-resize") => CursorStyle::ResizeUp,
        Some("e-resize") => CursorStyle::ResizeRight,
        Some("s-resize") => CursorStyle::ResizeDown,
        Some("w-resize") => CursorStyle::ResizeLeft,
        Some("ne-resize" | "sw-resize" | "nesw-resize") => CursorStyle::ResizeUpLeftDownRight,
        Some("nw-resize" | "se-resize" | "nwse-resize") => CursorStyle::ResizeUpRightDownLeft,
        Some("ew-resize") => CursorStyle::ResizeLeftRight,
        Some("ns-resize") => CursorStyle::ResizeUpDown,
        // shortcut: GPUI has no help, busy or zoom cursors; use an arrow until it does.
        _ => CursorStyle::Arrow,
    }
}

#[cfg(test)]
mod tests {
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
}
