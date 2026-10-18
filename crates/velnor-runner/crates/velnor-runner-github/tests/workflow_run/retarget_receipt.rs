use super::*;

#[test]
fn retarget_receipt_needs_exactly_one_appended_final_event() -> Result<(), String> {
    let before = history("main", Vec::new());
    let after = history(
        "sea/integration",
        vec![event(
            "acknowledged-event",
            "2026-10-17T00:00:00Z",
            "main",
            "sea/integration",
        )],
    );
    assert_eq!(
        new_retarget_event(&before, &after, "main", "sea/integration")
            .map_err(|error| error.to_string())?
            .id,
        "acknowledged-event"
    );
    let later = history(
        "main",
        vec![
            event(
                "acknowledged-event",
                "2026-10-17T00:00:00Z",
                "main",
                "sea/integration",
            ),
            event(
                "later-event",
                "2026-10-17T00:00:01Z",
                "sea/integration",
                "main",
            ),
        ],
    );
    assert_eq!(
        new_retarget_event(&before, &later, "main", "sea/integration"),
        Err(WorkflowRunError::Malformed)
    );
    let changed_head = PullRequestBaseHistory {
        head_sha: "cccccccccccccccccccccccccccccccccccccccc".to_owned(),
        ..after
    };
    assert_eq!(
        new_retarget_event(&before, &changed_head, "main", "sea/integration"),
        Err(WorkflowRunError::Malformed)
    );
    Ok(())
}
