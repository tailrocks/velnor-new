//! Request writer provenance tests for baseline publishing.

use super::*;

#[test]
fn publish_request_writer_records_push_refs() {
    let head = "a".repeat(40);
    let temp = tempfile::tempdir().expect("tempdir");
    let anchor = temp.path().join("anchor");
    fs::create_dir_all(&anchor).expect("anchor");
    let path = anchor.join(format!("{PUBLISH_OP}-request.json"));
    write_publish_request(
        &path,
        "push",
        &push_payload(&head),
        Some(&head),
        Some("o/r"),
        &anchor,
    )
    .expect("write");
    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("json");
    assert_eq!(written["schema"], 1);
    assert_eq!(written["op"], PUBLISH_OP);
    assert_eq!(written["event"], "push");
    assert_eq!(written["head"], head);
    assert_eq!(written["repository"], "o/r");
    assert_eq!(written["git_ref"], "refs/heads/testmain");
    assert_eq!(written["default_branch"], "testmain");
    let again = write_publish_request(
        &path,
        "push",
        &push_payload(&head),
        Some(&head),
        Some("o/r"),
        &anchor,
    );
    assert!(again.is_err(), "pre-existing requests never overwrite");
    let bad_payload = anchor.join("bad-request.json");
    assert!(
        write_publish_request(
            &bad_payload,
            "push",
            "nope",
            Some(&head),
            Some("o/r"),
            &anchor
        )
        .expect_err("malformed")
        .to_string()
        .contains("malformed_event_payload")
    );
    let bad_event = anchor.join("bad-event.json");
    assert!(
        write_publish_request(
            &bad_event,
            "issue_comment",
            &push_payload(&head),
            Some(&head),
            Some("o/r"),
            &anchor,
        )
        .expect_err("event")
        .to_string()
        .contains("unsupported_event")
    );
}
