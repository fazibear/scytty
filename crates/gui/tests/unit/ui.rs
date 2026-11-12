use super::*;
use core::prelude::v1::test;
#[test]
fn message_rows_preserve_unicode_and_blank_lines() {
    assert_eq!(
        message_line_rows("hello 世界", [6].into_iter()),
        ["hello", "世界"]
    );
    assert_eq!(message_line_rows("", std::iter::empty()), [""]);
    assert_eq!(message_line_rows("short", std::iter::empty()), ["short"]);
}
