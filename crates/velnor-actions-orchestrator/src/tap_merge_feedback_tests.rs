use super::*;

fn review(id: u64, user: u64, date: &str, state: &str) -> Value {
    serde_json::json!({
        "id": id,
        "user": {"id": user},
        "submitted_at": date,
        "state": state
    })
}

#[test]
fn latest_actor_review_state_controls_outstanding_change_requests() {
    let reviews = vec![
        review(1, 10, "2026-10-01T00:00:00Z", "CHANGES_REQUESTED"),
        review(2, 10, "2026-10-02T00:00:00Z", "APPROVED"),
        review(3, 11, "2026-10-01T00:00:00Z", "CHANGES_REQUESTED"),
        review(4, 11, "2026-10-02T00:00:00Z", "DISMISSED"),
        review(5, 12, "2026-10-01T00:00:00Z", "CHANGES_REQUESTED"),
        review(6, 12, "2026-10-02T00:00:00Z", "COMMENTED"),
        review(7, 13, "2026-10-03T00:00:00Z", "PENDING"),
    ];
    assert_eq!(
        outstanding_change_requests(&reviews).expect("review states"),
        0
    );
}

#[test]
fn newer_change_request_remains_outstanding() {
    let reviews = vec![
        review(1, 10, "2026-10-01T00:00:00Z", "APPROVED"),
        review(2, 10, "2026-10-02T00:00:00Z", "CHANGES_REQUESTED"),
    ];
    assert_eq!(
        outstanding_change_requests(&reviews).expect("review states"),
        1
    );
}
