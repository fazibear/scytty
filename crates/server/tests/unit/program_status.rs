use super::*;
fn report(records: &mut Records, body: &str) -> bool {
    records.apply(format!("7501;{body}").as_bytes(), 7)
}

#[test]
fn replaces_inherits_clears_and_bounds_records() {
    let mut records = Records::default();
    assert!(report(
        &mut records,
        "state=working:app=cargo:progress=40:msg=SGVsbG8="
    ));
    assert_eq!(records.state().as_deref(), Some("working"));
    assert_eq!(records.snapshot()[""].message.as_deref(), Some("Hello"));
    assert!(report(
        &mut records,
        "state=blocked:id=build/test:kind=permission:progress=50:title=VGVzdA"
    ));
    assert_eq!(
        records.snapshot()["build/test"].app.as_deref(),
        Some("cargo")
    );
    assert_eq!(
        records.snapshot()["build/test"].title.as_deref(),
        Some("Test")
    );
    assert!(report(&mut records, "state=done:id=build/test"));
    assert_eq!(records.snapshot()["build/test"].kind, None);
    assert_eq!(records.snapshot()["build/test"].progress, None);
    assert_eq!(records.snapshot()["build/test"].title, None);
    assert!(records.prompt());
    assert_eq!(records.state().as_deref(), Some("done"));
    assert!(report(&mut records, "state=clear:id=build"));
    assert_eq!(records.state(), None);
    for n in 0..256 {
        report(&mut records, &format!("state=working:id=task{n}"));
    }
    assert!(!report(&mut records, "state=working:id=task0"));
    assert!(report(&mut records, "state=working:id=task256"));
    assert_eq!(records.values.len(), 256);
    assert!(records.values.contains_key("task0"));
    assert!(!records.values.contains_key("task1"));
    assert!(report(&mut records, "state=clear"));
    assert!(!records.clear());
}
#[test]
fn clear_is_subtree_scoped_and_latest_state_is_not_an_aggregate() {
    let mut records = Records::default();
    report(&mut records, "state=working:app=cargo");
    report(&mut records, "state=working:id=builder");
    report(&mut records, "state=blocked:id=build/test");
    assert_eq!(records.state().as_deref(), Some("blocked"));
    assert!(report(&mut records, "state=working:app=cargo"));
    assert_eq!(records.state().as_deref(), Some("working"));
    report(&mut records, "state=clear:id=build");
    assert!(records.values.contains_key("builder"));
    assert!(!records.values.contains_key("build/test"));
    assert!(records.values.contains_key(""));
}
#[test]
fn engine_validates_untrusted_reports_and_duplicate_keys() {
    let mut records = Records::default();
    for body in [
        "state=unknown",
        "state=clear:id=bad//id",
        "state=done:msg=A",
        "state=done:msg=AA==",
        "app=cargo",
    ] {
        assert!(!report(&mut records, body), "{body}");
    }
    assert_eq!(records.state(), None);
    assert!(report(
        &mut records,
        "state=working:state=blocked:kind=auth:progress=999:app=cargo"
    ));
    assert_eq!(records.state().as_deref(), Some("blocked"));
    assert_eq!(records.snapshot()[""].kind.as_deref(), Some("auth"));
    assert_eq!(records.snapshot()[""].progress, None);
}
