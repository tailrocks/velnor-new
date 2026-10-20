use super::*;

fn not_queried() -> QualificationCacheBackendObservation {
    QualificationCacheBackendObservation::NotQueried
}

#[test]
fn staged_save_records_only_a_successful_action_as_succeeded() {
    let save = save_observation(
        Some("cache-key".to_owned()),
        "staged",
        true,
        QualificationCacheBackendObservation::Absent,
        QualificationCacheBackendObservation::Found(QualificationCacheBackendEntry {
            id: 8,
            key: "cache-key".to_owned(),
            git_ref: "refs/heads/main".to_owned(),
            version: "a".repeat(64),
            size_bytes: 3,
        }),
        Some("success"),
    )
    .expect("valid save evidence");
    assert_eq!(save.action, QualificationCacheSaveActionResult::Succeeded);
    assert_eq!(save.requested_key.as_deref(), Some("cache-key"));
}

#[test]
fn unchanged_and_disabled_saves_do_not_claim_backend_access() {
    let unchanged = save_observation(
        Some("cache-key".to_owned()),
        "not_required",
        false,
        not_queried(),
        not_queried(),
        None,
    )
    .expect("unchanged state");
    assert_eq!(
        unchanged.action,
        QualificationCacheSaveActionResult::NotRequired
    );
    assert!(unchanged.requested_key.is_none());

    let disabled = save_observation(
        None,
        "not_required",
        false,
        not_queried(),
        not_queried(),
        None,
    )
    .expect("disabled policy");
    assert_eq!(
        disabled.action,
        QualificationCacheSaveActionResult::Disabled
    );
}

#[test]
fn inconsistent_save_action_and_export_facts_are_rejected() {
    assert_eq!(
        save_observation(
            Some("cache-key".to_owned()),
            "staged",
            true,
            not_queried(),
            not_queried(),
            Some("skipped"),
        )
        .expect("action outcome is still recorded")
        .action,
        QualificationCacheSaveActionResult::Skipped
    );
    assert!(
        save_observation(
            Some("cache-key".to_owned()),
            "staged",
            false,
            not_queried(),
            not_queried(),
            Some("success"),
        )
        .is_err()
    );
}

#[test]
fn unchanged_export_does_not_require_or_accept_save_census() {
    let save = save_evidence(
        Some("cache-key".to_owned()),
        "not_required",
        false,
        None,
        None,
        None,
    )
    .expect("unchanged state does not query backend");
    assert_eq!(save.action, QualificationCacheSaveActionResult::NotRequired);
    assert!(save.requested_key.is_none());
    assert_eq!(save.before, not_queried());
    assert_eq!(save.after, not_queried());
    assert!(
        save_evidence(
            Some("cache-key".to_owned()),
            "not_required",
            false,
            Some(QualificationCacheBackendObservation::Absent),
            None,
            None,
        )
        .is_err()
    );
    assert!(
        save_evidence(
            Some("cache-key".to_owned()),
            "not_required",
            false,
            None,
            None,
            Some("success"),
        )
        .is_err()
    );
}
