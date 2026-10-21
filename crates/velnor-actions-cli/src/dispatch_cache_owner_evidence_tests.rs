use super::*;

#[test]
fn restore_facts_require_exact_action_result_and_key() {
    let hit =
        restore_facts_from_outputs(Some("cache-key"), "hit", true, Some("cache-key".to_owned()))
            .expect("matching hit");
    assert_eq!(hit.result, QualificationCacheRestoreResult::Hit);
    assert_eq!(hit.matched_key.as_deref(), Some("cache-key"));
    let miss =
        restore_facts_from_outputs(Some("cache-key"), "miss", false, None).expect("matching miss");
    assert_eq!(miss.result, QualificationCacheRestoreResult::Miss);
    assert!(
        restore_facts_from_outputs(Some("cache-key"), "hit", true, Some("other".to_owned()))
            .is_err()
    );
    assert!(restore_facts_from_outputs(None, "miss", false, None).is_err());
    assert!(
        restore_facts_from_outputs(
            Some("cache-key"),
            "miss",
            true,
            Some("cache-key".to_owned())
        )
        .is_err()
    );
}

#[test]
fn save_facts_preserve_only_action_result_and_key() {
    for (export_result, expected, requested_key, action, wanted) in [
        (
            "not_required",
            None,
            false,
            None,
            QualificationCacheSaveActionResult::Disabled,
        ),
        (
            "not_required",
            Some("cache-key"),
            false,
            None,
            QualificationCacheSaveActionResult::NotRequired,
        ),
        (
            "staged",
            Some("cache-key"),
            true,
            Some("success"),
            QualificationCacheSaveActionResult::Succeeded,
        ),
        (
            "staged",
            Some("cache-key"),
            true,
            Some("skipped"),
            QualificationCacheSaveActionResult::Skipped,
        ),
        (
            "staged",
            Some("cache-key"),
            true,
            Some("failure"),
            QualificationCacheSaveActionResult::Failed,
        ),
    ] {
        let facts = save_facts(expected, export_result, requested_key, action)
            .expect("consistent action result");
        assert_eq!(facts.action, wanted);
    }
}

#[test]
fn save_facts_reject_backend_observation_states_and_inconsistent_actions() {
    assert!(save_facts(Some("cache-key"), "staged", false, Some("success")).is_err());
    assert!(save_facts(Some("cache-key"), "not_required", true, None).is_err());
    assert!(save_facts(None, "staged", true, Some("success")).is_err());
    assert!(save_facts(Some("cache-key"), "staged", true, Some("unknown")).is_err());
}

#[test]
fn raw_worker_output_has_no_backend_or_admission_fields() {
    let source = include_str!("dispatch_cache_owner_evidence.rs");
    assert!(!source.contains("backend_admitted"));
    assert!(!source.contains("backend_status"));
    assert!(!source.contains("CENSUS_PREFIX"));
}
