use super::*;

#[test]
fn program_status_fragmentation_queries_prompts_and_reset() {
    let mut metadata = Metadata::default();
    assert!(!metadata.feed(b"\x1b]7501;state=wor"));
    assert!(metadata.feed(b"king:app=cargo:progress=40\x1b\\"));
    assert_eq!(metadata.program_status.state().as_deref(), Some("working"));
    assert!(!metadata.feed(b"\x1b]7501;state=error\x18\x07"));
    assert_eq!(metadata.program_status.state().as_deref(), Some("working"));
    assert!(!metadata.feed(b"\x1b]7501;?\x07\x1b]7501;?\x1b\\"));
    assert_eq!(metadata.replies, b"\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x07\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x1b\\");
    assert!(metadata.feed(b"\x1b]133;A\x07"));
    assert_eq!(metadata.program_status.state(), None);
    assert!(metadata.feed(b"\x1b]7501;state=done\x07"));
    assert!(!metadata.feed(b"\x1b]133;A\x07"));
    assert!(metadata.feed(b"\x1bc"));
    assert_eq!(metadata.program_status.state(), None);
}
#[test]
fn mouse_shapes_are_bounded_fragmented_and_resettable() {
    let mut metadata = Metadata::default();
    assert!(!metadata.feed(b"\x1b]22;poi"));
    assert!(metadata.feed(b"nter\x1b\\"));
    assert_eq!(metadata.mouse_shape.as_deref(), Some("pointer"));
    assert!(!metadata.feed(b"\x1b]22;pointer\x07"));
    assert!(!metadata.feed(b"\x1b]22;invalid\x07"));
    assert!(!metadata.feed(b"\x1b]22;text\x18\x07"));
    assert_eq!(metadata.mouse_shape.as_deref(), Some("pointer"));
    assert!(metadata.feed(b"\x1b]22;\x07"));
    assert_eq!(metadata.mouse_shape, None);
    assert!(metadata.feed(b"\x1b]22;default\x07"));
    assert_eq!(metadata.mouse_shape.as_deref(), Some("default"));
    assert!(metadata.feed(b"\x1bc"));
    assert_eq!(metadata.mouse_shape, None);
    let mut oversized = b"\x1b]22;".to_vec();
    oversized.extend(std::iter::repeat_n(b'x', 9000));
    oversized.extend_from_slice(b"\x07");
    assert!(!metadata.feed(&oversized));
    assert!(metadata.feed(b"\x1b]22;crosshair\x07"));
}
#[test]
fn split_sequences_and_validation() {
    let mut vars = Metadata::default();
    let bytes = b"text\x1b]1337;SetUserVar=right_status=aGVsbG8=\x1b\\";
    for &b in bytes {
        vars.feed(&[b]);
    }
    assert_eq!(vars.values["right_status"], "hello");
    assert!(!vars.feed(bytes));
    assert!(vars.feed(b"\x1b]1337;SetUserVar=right_status_timestamp=MTc5NDAw\r\nOTM3MA==\r\x07"));
    assert_eq!(vars.values["right_status_timestamp"], "1794009370");
    assert!(vars.feed(b"\x1b]1337;SetUserVar=right_status=\x07"));
    assert_eq!(vars.values["right_status"], "");
    assert!(!vars.feed(b"\x1b]1337;SetUserVar=bad=***\x07"));
    vars.feed(&[b' '; 10000]);
    assert!(vars.feed(b"\x1b]1337;SetUserVar=x=eQ==\x07"));
    assert_eq!(vars.values["x"], "y");
    let mut oversized = b"\x1b]1337;SetUserVar=x=".to_vec();
    oversized.extend([b'A'; 10000]);
    oversized.push(7);
    vars.feed(&oversized);
    assert_eq!(vars.values["x"], "y");
    assert!(vars.feed(b"\x1b]1337;SetUserVar=x=eg==\x07"));
}
