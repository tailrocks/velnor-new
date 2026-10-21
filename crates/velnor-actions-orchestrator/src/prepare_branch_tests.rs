//! Branch parsing from the local `origin/HEAD` symbolic ref and the event payload.

use super::{branch_from_event_payload, branch_from_origin_head};

#[test]
fn origin_head_accepts_only_supported_branch_names() {
    for (input, expected) in [
        ("origin/main", "main"),
        ("origin/release/1.2", "release/1.2"),
        ("feature/x_y-z", "feature/x_y-z"),
        ("refs/heads/main", "refs/heads/main"),
    ] {
        assert_eq!(branch_from_origin_head(input).as_deref(), Some(expected));
    }
    for input in [
        "origin/HEAD",
        "origin/-main",
        "origin/a/../b",
        "origin/main;evil",
        "origin/main\non: [push]",
    ] {
        assert_eq!(branch_from_origin_head(input), None, "{input:?}");
    }
}

#[test]
fn event_payload_names_the_default_branch_only_when_valid() {
    let payload = |branch: &str| format!("{{\"repository\":{{\"default_branch\":\"{branch}\"}}}}");
    assert_eq!(
        branch_from_event_payload(&payload("trunk")).as_deref(),
        Some("trunk")
    );
    for bad in ["", "-main", "a/../b", "main;evil"] {
        assert_eq!(branch_from_event_payload(&payload(bad)), None, "{bad:?}");
    }
    for bad in [
        "not json",
        "{}",
        "{\"repository\":{}}",
        "{\"repository\":{\"default_branch\":7}}",
    ] {
        assert_eq!(branch_from_event_payload(bad), None, "{bad:?}");
    }
}
