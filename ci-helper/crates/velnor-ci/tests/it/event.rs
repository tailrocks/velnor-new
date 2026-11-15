// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::event::{Event, check_default_branch, parse_event};

#[test]
fn the_event_payload_gives_the_default_branch() {
    let event = parse_event(
        "{\"repository\":{\"default_branch\":\"trunk\"},\"pull_request\":{\"number\":7}}",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(event.default_branch.as_deref(), Some("trunk"));
    assert!(parse_event("[]").is_err());
}

#[test]
fn a_configured_default_branch_that_differs_from_the_repositorys_is_refused_naming_both() {
    let event = |branch: Option<&str>| Event {
        default_branch: branch.map(str::to_owned),
        ..Event::default()
    };
    assert_eq!(check_default_branch("main", &event(Some("main"))), Ok(()));
    let message = check_default_branch("main", &event(Some("trunk")))
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(
        message.contains("\"main\"") && message.contains("\"trunk\""),
        "{message}"
    );
    assert!(check_default_branch("main", &event(None)).is_err());
}
